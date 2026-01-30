# Fish completion for slab
# Install: Place in ~/.config/fish/completions/slab.fish

# Disable file completions by default
complete -c slab -f

# Helper function to get models dynamically
function __slab_models
    slab models --names-only 2>/dev/null
end

# Helper function to get sessions dynamically
function __slab_sessions
    slab sessions --names-only 2>/dev/null
end

# Helper to check if a subcommand has been given
function __slab_needs_command
    set -l cmd (commandline -opc)
    set -l subcommands chat run config models sessions test init completions
    for i in (seq 2 (count $cmd))
        if contains -- $cmd[$i] $subcommands
            return 1
        end
    end
    return 0
end

# Helper to check which subcommand is active
function __slab_using_command
    set -l cmd (commandline -opc)
    set -l subcommands chat run config models sessions test init completions
    for i in (seq 2 (count $cmd))
        if contains -- $cmd[$i] $subcommands
            if test "$cmd[$i]" = "$argv[1]"
                return 0
            end
            return 1
        end
    end
    return 1
end

# Global options
complete -c slab -l model -s m -d 'Model to use' -xa '(__slab_models)'
complete -c slab -l config -s c -d 'Path to config file' -rF
complete -c slab -l verbose -s v -d 'Enable verbose output'
complete -c slab -l no-stream -d 'Disable streaming'
complete -c slab -l help -s h -d 'Print help'
complete -c slab -l version -s V -d 'Print version'

# Subcommands
complete -c slab -n __slab_needs_command -a chat -d 'Start interactive chat REPL'
complete -c slab -n __slab_needs_command -a run -d 'Run a single prompt and exit'
complete -c slab -n __slab_needs_command -a config -d 'Show or edit configuration'
complete -c slab -n __slab_needs_command -a models -d 'List available Ollama models'
complete -c slab -n __slab_needs_command -a sessions -d 'List saved sessions'
complete -c slab -n __slab_needs_command -a test -d 'Run prompt tests'
complete -c slab -n __slab_needs_command -a init -d 'Initialize project with .slab directory'
complete -c slab -n __slab_needs_command -a completions -d 'Generate shell completions'

# Chat options
complete -c slab -n '__slab_using_command chat' -l continue -s C -d 'Continue from last session'
complete -c slab -n '__slab_using_command chat' -l session -s s -d 'Use a named session' -xa '(__slab_sessions)'

# Config options
complete -c slab -n '__slab_using_command config' -l show -d 'Show current configuration'
complete -c slab -n '__slab_using_command config' -l init -d 'Initialize a new config file'
complete -c slab -n '__slab_using_command config' -l set -d 'Set a config value (key=value)'

# Models options
complete -c slab -n '__slab_using_command models' -l names-only -d 'Output only model names'

# Sessions options
complete -c slab -n '__slab_using_command sessions' -l names-only -d 'Output only session names'

# Test options
complete -c slab -n '__slab_using_command test' -l filter -d 'Filter tests by pattern'
complete -c slab -n '__slab_using_command test' -l model -d 'Run tests with specific model' -xa '(__slab_models)'

# Completions options
complete -c slab -n '__slab_using_command completions' -a 'bash zsh fish powershell' -d 'Shell type'
