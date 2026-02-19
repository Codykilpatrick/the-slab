# Releasing The Slab

Pushing a `v*` tag to `main` triggers the release workflow automatically. It builds binaries for all four targets and publishes a GitHub Release with auto-generated notes.

## Release targets

| Artifact | Platform |
|---|---|
| `slab-linux-x86_64.tar.gz` | Linux x86_64 (glibc) |
| `slab-linux-x86_64-musl.tar.gz` | Linux x86_64 (static, air-gapped) |
| `slab-macos-x86_64.tar.gz` | macOS Intel |
| `slab-macos-arm64.tar.gz` | macOS Apple Silicon |

## Steps

### 1. Make sure CI is green

All commits to `main` run the CI workflow (build + test + clippy + fmt). Verify it's passing before cutting a release.

### 2. Bump the version

Update `Cargo.toml`:

```toml
version = "X.Y.Z"
```

Then build so `Cargo.lock` is updated:

```bash
cargo build
```

Commit both files:

```bash
git add Cargo.toml Cargo.lock
git commit -m "Bump version to X.Y.Z"
```

### 3. Update the README

Make sure `README.md` reflects any new features or changed behavior since the last release. Commit any changes before tagging.

### 4. Tag and push

```bash
git tag vX.Y.Z
git push origin main --tags
```

That's it. The [release workflow](.github/workflows/release.yml) picks up the tag and handles the rest.

### 5. Verify the release

Check [GitHub Releases](https://github.com/Codykilpatrick/the-slab/releases) after a few minutes — the workflow takes ~5–10 minutes to build all targets. Confirm all four `.tar.gz` artifacts are attached.

## Versioning

Follow [semver](https://semver.org/):

- **Patch** (`0.5.x`) — bug fixes, docs, small tweaks
- **Minor** (`0.x.0`) — new features, backwards-compatible
- **Major** (`x.0.0`) — breaking changes to CLI interface or config format
