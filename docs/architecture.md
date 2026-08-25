# apm Architecture

`apm` is a macOS-first command-line audio package manager. The CLI is the only
user-facing application; reusable package behavior lives in the Rust core.

## Workspace

- `crates/apm-cli`: the `apm` binary, command parsing, terminal output,
  confirmation prompts, downloads, and macOS command integration.
- `crates/apm-core`: registry, state, scanning, diagnostics, install planning,
  shared lifecycle operations, and audio-AI model packages.
- `registry/`: the bundled plugin, bundle, installer, and model definitions.
- `Formula/`: the Homebrew formula for the CLI release.
- `.github/workflows/ci.yml`: Rust build, Clippy, and test checks.
- `.github/workflows/release.yml`: universal macOS CLI packaging and release.

There is no desktop application or local HTTP service in the supported product.

## Boundaries

### CLI

The CLI owns concerns that require a terminal or process environment:

- Clap command and option parsing
- human-readable and JSON output
- interactive confirmations
- progress bars
- shell completion generation
- opening vendor installers and product pages
- invoking privileged PKG installation after explicit user review

Commands should translate arguments into core operations rather than duplicate
registry, state, matching, or lifecycle policy.

### Core

`apm-core` owns behavior that must remain deterministic and testable without a
terminal:

- configuration and registry loading
- package metadata and source matching
- installed-state persistence
- AU/VST3 scanning and reconciliation
- install planning, direct archive placement, updates, removal, and pinning
- diagnostics
- model manifests, lockfiles, content-addressed weights, runtime metadata, and
  the `ModelRunner` boundary

Structured engine results and events remain useful for CLI rendering and tests;
they do not imply a second frontend.

## Package Lifecycle

1. The CLI loads configuration and registry sources.
2. The core resolves package/version/format policy.
3. Direct packages are downloaded and checksum-verified before extraction.
4. AU and VST3 bundles are placed under the selected user or system plugin
   directory; app packages use the corresponding Applications directory.
5. Managed or manual packages open the vendor flow and are reconciled later by
   `apm scan`.
6. Installed state records version, format, path, origin, and pin status.
7. Update and removal commands consult that same state instead of inferring
   ownership from filenames.

PKG installers are exceptional: the CLI must prompt before invoking `sudo
installer`. Core archive operations do not silently escalate privileges.

## Local Data

The CLI uses XDG-compatible configuration, data, and cache roots with macOS
defaults. Package state and registry cache writes use the shared file helpers so
failed writes do not replace valid prior state.

Audio-AI packages use `~/.apm` for manifests, content-addressed weights, prepared
runtime metadata, cache, and logs. Executable model adapters remain incomplete;
`apm model run` reports the structured `adapter_runner_unavailable` blocker
rather than claiming success.

## Distribution

Tagged releases build `apm` for Apple Silicon and Intel, combine the binaries
with `lipo`, publish a universal tarball plus SHA-256 checksum, and update the
Homebrew formula. No app bundle, DMG, sidecar, signing, or notarization workflow
is required for the CLI product.
