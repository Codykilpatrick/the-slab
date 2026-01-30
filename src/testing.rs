use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use crate::config::Config;
use crate::ollama::{ChatRequest, Message, ModelOptions, OllamaClient};

/// A test case for validating LLM responses
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestCase {
    /// Test name
    pub name: String,

    /// The prompt to send
    pub prompt: String,

    /// Optional system prompt override
    #[serde(default)]
    pub system_prompt: Option<String>,

    /// Optional model override
    #[serde(default)]
    pub model: Option<String>,

    /// Assertions to check against the response
    pub assertions: Vec<Assertion>,

    /// Timeout in seconds (default: 60)
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,

    /// Tags for filtering tests
    #[serde(default)]
    pub tags: Vec<String>,
}

fn default_timeout() -> u64 {
    60
}

/// An assertion to validate against LLM response
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Assertion {
    /// Response must contain this string
    Contains { value: String },

    /// Response must not contain this string
    NotContains { value: String },

    /// Response must match this regex pattern
    Regex { pattern: String },

    /// Response must not match this regex pattern
    NotRegex { pattern: String },

    /// Response time must be under this many milliseconds
    MaxLatency { ms: u64 },

    /// Response must be valid JSON
    ValidJson,

    /// Response length must be between min and max characters
    LengthBetween { min: usize, max: usize },
}

impl Assertion {
    /// Check if the assertion passes for the given response
    pub fn check(&self, response: &str, latency_ms: u64) -> AssertionResult {
        match self {
            Assertion::Contains { value } => {
                if response.contains(value) {
                    AssertionResult::Pass
                } else {
                    AssertionResult::Fail(format!(
                        "Response does not contain: '{}'",
                        truncate(value, 50)
                    ))
                }
            }
            Assertion::NotContains { value } => {
                if !response.contains(value) {
                    AssertionResult::Pass
                } else {
                    AssertionResult::Fail(format!(
                        "Response contains forbidden text: '{}'",
                        truncate(value, 50)
                    ))
                }
            }
            Assertion::Regex { pattern } => {
                match Regex::new(pattern) {
                    Ok(re) => {
                        if re.is_match(response) {
                            AssertionResult::Pass
                        } else {
                            AssertionResult::Fail(format!(
                                "Response does not match pattern: {}",
                                truncate(pattern, 50)
                            ))
                        }
                    }
                    Err(e) => AssertionResult::Fail(format!("Invalid regex: {}", e)),
                }
            }
            Assertion::NotRegex { pattern } => {
                match Regex::new(pattern) {
                    Ok(re) => {
                        if !re.is_match(response) {
                            AssertionResult::Pass
                        } else {
                            AssertionResult::Fail(format!(
                                "Response matches forbidden pattern: {}",
                                truncate(pattern, 50)
                            ))
                        }
                    }
                    Err(e) => AssertionResult::Fail(format!("Invalid regex: {}", e)),
                }
            }
            Assertion::MaxLatency { ms } => {
                if latency_ms <= *ms {
                    AssertionResult::Pass
                } else {
                    AssertionResult::Fail(format!(
                        "Response took {}ms, max allowed: {}ms",
                        latency_ms, ms
                    ))
                }
            }
            Assertion::ValidJson => {
                match serde_json::from_str::<serde_json::Value>(response) {
                    Ok(_) => AssertionResult::Pass,
                    Err(e) => AssertionResult::Fail(format!("Invalid JSON: {}", e)),
                }
            }
            Assertion::LengthBetween { min, max } => {
                let len = response.len();
                if len >= *min && len <= *max {
                    AssertionResult::Pass
                } else {
                    AssertionResult::Fail(format!(
                        "Response length {} not in range [{}, {}]",
                        len, min, max
                    ))
                }
            }
        }
    }

    /// Get a short description of the assertion
    pub fn description(&self) -> String {
        match self {
            Assertion::Contains { value } => format!("contains '{}'", truncate(value, 30)),
            Assertion::NotContains { value } => format!("not contains '{}'", truncate(value, 30)),
            Assertion::Regex { pattern } => format!("matches /{}/", truncate(pattern, 30)),
            Assertion::NotRegex { pattern } => format!("not matches /{}/", truncate(pattern, 30)),
            Assertion::MaxLatency { ms } => format!("latency <= {}ms", ms),
            Assertion::ValidJson => "valid JSON".to_string(),
            Assertion::LengthBetween { min, max } => format!("length in [{}, {}]", min, max),
        }
    }
}

#[derive(Debug, Clone)]
pub enum AssertionResult {
    Pass,
    Fail(String),
}

/// Result of running a single test
#[derive(Debug, Clone)]
pub struct TestResult {
    pub name: String,
    pub passed: bool,
    pub latency_ms: u64,
    pub assertion_results: Vec<(String, AssertionResult)>,
    pub response_preview: String,
    pub error: Option<String>,
}

/// Load test cases from a directory
pub fn load_tests_from_directory(dir: &Path) -> Vec<TestCase> {
    let mut tests = Vec::new();

    if !dir.exists() || !dir.is_dir() {
        return tests;
    }

    for ext in &["yaml", "yml"] {
        let pattern = dir.join(format!("**/*.{}", ext));
        if let Ok(entries) = glob::glob(&pattern.to_string_lossy()) {
            for entry in entries.flatten() {
                match load_test_file(&entry) {
                    Ok(mut file_tests) => tests.append(&mut file_tests),
                    Err(e) => eprintln!("Warning: Failed to load test file {:?}: {}", entry, e),
                }
            }
        }
    }

    tests
}

/// Load test cases from a single file
fn load_test_file(path: &Path) -> Result<Vec<TestCase>, String> {
    let content = fs::read_to_string(path)
        .map_err(|e| format!("Failed to read file: {}", e))?;

    // Try parsing as a single test
    if let Ok(test) = serde_yaml::from_str::<TestCase>(&content) {
        return Ok(vec![test]);
    }

    // Try parsing as a list of tests
    if let Ok(tests) = serde_yaml::from_str::<Vec<TestCase>>(&content) {
        return Ok(tests);
    }

    Err("Failed to parse as test case or test list".to_string())
}

/// Test runner that executes tests against Ollama
pub struct TestRunner {
    client: OllamaClient,
    config: Config,
    default_model: String,
    verbose: bool,
}

impl TestRunner {
    pub fn new(client: OllamaClient, config: Config, default_model: String, verbose: bool) -> Self {
        Self {
            client,
            config,
            default_model,
            verbose,
        }
    }

    /// Run all tests and return results
    pub async fn run_tests(&self, tests: &[TestCase], filter: Option<&str>, model_override: Option<&str>) -> Vec<TestResult> {
        let mut results = Vec::new();

        // Filter tests if pattern provided
        let tests_to_run: Vec<&TestCase> = tests
            .iter()
            .filter(|t| {
                if let Some(pattern) = filter {
                    t.name.contains(pattern) || t.tags.iter().any(|tag| tag.contains(pattern))
                } else {
                    true
                }
            })
            .collect();

        if tests_to_run.is_empty() {
            println!("{}", style("No tests to run.").yellow());
            return results;
        }

        println!(
            "{} Running {} test(s)...\n",
            style("→").cyan(),
            tests_to_run.len()
        );

        let progress = ProgressBar::new(tests_to_run.len() as u64);
        progress.set_style(
            ProgressStyle::default_bar()
                .template("{prefix:.bold} [{bar:40.cyan/blue}] {pos}/{len} {msg}")
                .unwrap()
                .progress_chars("━━─"),
        );
        progress.set_prefix("Testing");

        for test in tests_to_run {
            progress.set_message(truncate(&test.name, 30));

            let result = self.run_single_test(test, model_override).await;
            results.push(result);

            progress.inc(1);
        }

        progress.finish_and_clear();

        results
    }

    async fn run_single_test(&self, test: &TestCase, model_override: Option<&str>) -> TestResult {
        let model = model_override
            .map(|s| s.to_string())
            .or_else(|| test.model.clone())
            .unwrap_or_else(|| self.default_model.clone());

        let mut messages = Vec::new();

        // Add system prompt if specified
        if let Some(system) = &test.system_prompt {
            messages.push(Message::system(system.clone()));
        }

        messages.push(Message::user(&test.prompt));

        let request = ChatRequest {
            model: model.clone(),
            messages,
            stream: Some(false),
            options: Some(ModelOptions {
                temperature: Some(0.7),
                top_p: Some(0.9),
                num_ctx: Some(self.config.context_limit),
            }),
        };

        let start = Instant::now();

        // Run with timeout
        let timeout = Duration::from_secs(test.timeout_secs);
        let response_result = tokio::time::timeout(timeout, self.client.chat(request)).await;

        let latency_ms = start.elapsed().as_millis() as u64;

        match response_result {
            Ok(Ok(response)) => {
                // Check all assertions
                let mut assertion_results = Vec::new();
                let mut all_passed = true;

                for assertion in &test.assertions {
                    let result = assertion.check(&response, latency_ms);
                    let passed = matches!(result, AssertionResult::Pass);
                    if !passed {
                        all_passed = false;
                    }
                    assertion_results.push((assertion.description(), result));
                }

                TestResult {
                    name: test.name.clone(),
                    passed: all_passed,
                    latency_ms,
                    assertion_results,
                    response_preview: truncate(&response, 200),
                    error: None,
                }
            }
            Ok(Err(e)) => TestResult {
                name: test.name.clone(),
                passed: false,
                latency_ms,
                assertion_results: Vec::new(),
                response_preview: String::new(),
                error: Some(format!("Request failed: {}", e)),
            },
            Err(_) => TestResult {
                name: test.name.clone(),
                passed: false,
                latency_ms,
                assertion_results: Vec::new(),
                response_preview: String::new(),
                error: Some(format!("Test timed out after {}s", test.timeout_secs)),
            },
        }
    }

    /// Print test results in a table format
    pub fn print_results(&self, results: &[TestResult]) {
        if results.is_empty() {
            return;
        }

        let passed = results.iter().filter(|r| r.passed).count();
        let failed = results.len() - passed;

        println!();
        println!("{}", style("Test Results").cyan().bold());
        println!("{}", style("─".repeat(60)).dim());

        for result in results {
            let status = if result.passed {
                style("PASS").green()
            } else {
                style("FAIL").red()
            };

            println!(
                "  {} {} {}",
                status,
                style(&result.name).bold(),
                style(format!("({}ms)", result.latency_ms)).dim()
            );

            if !result.passed {
                if let Some(error) = &result.error {
                    println!("       {} {}", style("Error:").red(), error);
                }

                for (desc, assertion_result) in &result.assertion_results {
                    match assertion_result {
                        AssertionResult::Pass => {
                            if self.verbose {
                                println!("       {} {}", style("✓").green(), desc);
                            }
                        }
                        AssertionResult::Fail(msg) => {
                            println!("       {} {} - {}", style("✗").red(), desc, msg);
                        }
                    }
                }

                if self.verbose && !result.response_preview.is_empty() {
                    println!(
                        "       {} {}",
                        style("Response:").dim(),
                        style(&result.response_preview).dim()
                    );
                }
            } else if self.verbose {
                for (desc, _) in &result.assertion_results {
                    println!("       {} {}", style("✓").green().dim(), style(desc).dim());
                }
            }
        }

        println!("{}", style("─".repeat(60)).dim());
        println!(
            "  {} passed, {} failed",
            style(passed).green().bold(),
            if failed > 0 {
                style(failed).red().bold()
            } else {
                style(failed).dim()
            }
        );
        println!();
    }
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_contains_assertion() {
        let assertion = Assertion::Contains {
            value: "hello".to_string(),
        };
        assert!(matches!(
            assertion.check("hello world", 0),
            AssertionResult::Pass
        ));
        assert!(matches!(
            assertion.check("goodbye world", 0),
            AssertionResult::Fail(_)
        ));
    }

    #[test]
    fn test_regex_assertion() {
        let assertion = Assertion::Regex {
            pattern: r"Result<.*>".to_string(),
        };
        assert!(matches!(
            assertion.check("fn foo() -> Result<String, Error>", 0),
            AssertionResult::Pass
        ));
        assert!(matches!(
            assertion.check("fn foo() -> String", 0),
            AssertionResult::Fail(_)
        ));
    }

    #[test]
    fn test_valid_json_assertion() {
        let assertion = Assertion::ValidJson;
        assert!(matches!(
            assertion.check(r#"{"key": "value"}"#, 0),
            AssertionResult::Pass
        ));
        assert!(matches!(
            assertion.check("not json", 0),
            AssertionResult::Fail(_)
        ));
    }

    #[test]
    fn test_length_between_assertion() {
        let assertion = Assertion::LengthBetween { min: 5, max: 10 };
        assert!(matches!(
            assertion.check("hello", 0),
            AssertionResult::Pass
        ));
        assert!(matches!(
            assertion.check("hi", 0),
            AssertionResult::Fail(_)
        ));
        assert!(matches!(
            assertion.check("hello world!", 0),
            AssertionResult::Fail(_)
        ));
    }
}
