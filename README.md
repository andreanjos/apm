# apm - Audio Package Manager

`apm` is a macOS-first audio package manager for producers. The product goal is
a trusted desktop app and installer that can browse, install, update, remove,
repair, and eventually run audio packages locally.

Today, the repo contains the Rust CLI, package-manager core, and a Tauri desktop
shell. The core already pulls catalog definitions from Git-backed registries,
validates downloads before installing, and tracks what is installed on disk. The
registry catalog includes 8,000+ entries covering installable plugins, product
bundles, DAWs, utilities, upgrades, expansions, preset packs, sample libraries,
and vendor-managed products.

The next product milestone is the desktop front door: a signed/notarized macOS
app that reuses the same Rust engine instead of reimplementing package-manager
behavior. See [Desktop Product Plan](docs/desktop-product-plan.md) and
[Architecture](docs/architecture.md). The current requirement evidence lives in
[v3.0 Readiness Matrix](docs/v3-readiness.md).

## Current status

- **Available now:** Rust CLI for AU/VST3 plugin package management.
- **In progress:** desktop Catalog, Library, Diagnostics, and Runtime
  workspaces with first-run readiness, in-app package search and filters,
  service-backed library scan/reconcile from Diagnostics, local app/DMG preview
  bundles with a bundled CLI sidecar but without Developer ID signing or
  notarization, in-app distribution-channel readiness, manual signed desktop
  release workflow tooling with GitHub Environment and workflow-readiness
  preflights awaiting remote workflow publication, real
  `macos-desktop-release` secrets, and a verified run, foreground local service
  preview with desktop launch/reuse
  integration, and audio-AI model manifest, lockfile, `~/.apm` store, and
  verified weight-pull foundation.
- **Planned next:** configure real signed/notarized desktop release credentials,
  run the first verified desktop release workflow, privileged PKG helper and
  receipt-backed rollback implementation, and deeper audio-AI execution.

`apm` should be understood as an audio package manager, not only an audio plugin
installer. Plugins are the first useful package type; utilities, sample/preset
packs, and local audio-AI model packages belong in the same long-term product
model.

## CLI installation

```sh
brew tap andreanjos/apm https://github.com/andreanjos/apm
brew install apm
```

Or build from source with the current stable Rust toolchain:

```sh
cargo install --path crates/apm-cli
```

The public desktop app installer is not published yet. Local desktop preview
bundles without Developer ID signing or notarization can be built for internal
testing with
`cd apps/apm-desktop && npm run bundle:macos:verified`; the artifacts land
under the workspace root `target/release/bundle/`. Existing preview artifacts
can be opened with `npm run open:macos:preview` or mounted with
`npm run open:macos:preview:dmg` from `apps/apm-desktop`; both commands verify
the selected artifact before handing it to macOS. Bundle builds stage the
sidecar, run desktop unit tests, and build the frontend before bundling; the
verified local command then ad-hoc signs the preview app, rebuilds the DMG
around that signed app, and checks both artifacts. The current public release
artifact remains the CLI package.

The desktop package also has an explicit release gate:
`cd apps/apm-desktop && npm run release:macos:check` verifies the Tauri release
configuration, Cargo CLI/desktop crate, Tauri, and desktop package version
parity, required desktop release support files, build-tool tests, package
scripts, and the manual desktop workflow safety rails, while
`npm run release:macos:github-bootstrap -- --repo andreanjos/apm` creates or
updates the remote GitHub release Environment shell, and
`npm run release:macos:github-secrets` validates local signing/notarization
secret inputs before `--apply` stores them in GitHub. `npm run
release:macos:github-check` checks that the Environment has the required secret
names before the first signed workflow run. `npm run
release:macos:workflow-check` then verifies that GitHub Actions can see the
manual desktop workflow, that the release environment exposes the required
secret names, and that the release tag exists and points at the expected commit
before a dispatch. `npm run release:macos:status` prints the same
public-release readiness state and next steps without requiring a dispatch. `npm run
bundle:macos:release` refuses to build a public desktop artifact unless
Developer ID signing and notarization inputs are present.
`.github/workflows/desktop-release.yml` wires that gate into a manual CI
workflow that can upload dry-run artifacts or attach verified assets to a
GitHub Release after the `macos-desktop-release` environment secrets are
configured. `npm run release:macos:workflow-dispatch` triggers that workflow
with `publish=false` by default. Release asset packaging writes the app zip,
DMG, SHA-256 manifest, and a JSON evidence manifest for the verified artifact
set. `npm run release:macos:workflow-accept` validates that the GitHub dry-run
run succeeded, downloads the latest matching `publish=false` workflow artifact
set for the tag and expected commit, and runs local acceptance against it. See
[macOS Distribution Plan](docs/macos-distribution.md) and
[macOS Public Release Handoff](docs/macos-public-release-handoff.md).

Local preview apps can be checked with `npm run verify:macos:preview`; local
preview app/DMG pairs can be checked with `npm run verify:macos:preview:dmg`.
Both preview checks require a valid local code signature, but only the release
gate accepts public Developer ID distribution evidence.
For a full local v3 checkpoint before a merge-ready handoff, run
`cd apps/apm-desktop && npm run verify:v3:local`; it runs the workspace tests,
desktop release preflight, verified preview bundle, release asset evidence,
checksum manifest verification, tracked diff whitespace checks, and untracked
file whitespace checks.
Public desktop artifacts must pass `npm run verify:macos:release`, which
checks Developer ID signing, Gatekeeper assessment, stapling, DMG integrity,
and the bundled `apm-cli` sidecar.

The first desktop install, update, uninstall, and troubleshooting policy is in
[macOS Release Runbook](docs/macos-release-runbook.md). App uninstall removes
the desktop app and embedded sidecar only; package removal remains an explicit
`apm` package operation.

### Claude Code

```sh
cp -r .claude/skills/apm ~/.claude/skills/
```

Then use `/apm search reverb` or `/apm install surge-xt` directly in Claude Code.

## CLI quick start

```sh
apm sync                        # Pull latest catalog definitions
apm search reverb               # Find plugins by keyword
apm info valhalla-supermassive  # View plugin details
apm install tal-noisemaker      # Install a plugin
apm list                        # See what's installed
apm uninstalled                 # Browse installable products not yet installed
apm outdated                    # Check for updates
apm upgrade                     # Upgrade everything
```

## Commands

### Sync and search

```sh
apm sync                              # Pull latest registry
apm search reverb                     # Search by keyword
apm search --category instruments     # Filter by category
apm search --vendor "Valhalla DSP"    # Filter by vendor
apm info surge-xt                     # Plugin details
```

`apm info` shows the product type and access mode so it is clear whether a
record is a standalone plugin, bundle, upgrade, or vendor-managed product.

### Install and remove

```sh
apm install tal-noisemaker                    # Install (AU + VST3)
apm install tal-noisemaker --format vst3      # VST3 only (also supports au/app)
apm install tal-noisemaker --version 4.3.2    # Specific version
sudo apm install tal-noisemaker --system      # System-wide (/Library/)
printf "vital\nsurge-xt\n" | apm install --stdin
apm install --dry-run surge-xt                # Preview without installing
apm --json install surge-xt --dry-run         # Machine-readable install plan
apm install massive-x                         # Opens Native Access when required

apm remove tal-noisemaker                     # Remove a plugin
```

Audio plugins install to `~/Library/Audio/Plug-Ins/` by default. App-format
entries install to `~/Applications/` unless `--system` is used.

apm supports three install modes:

- Direct: apm downloads the archive and installs it itself.
- Managed: apm opens the required vendor installer app, such as Native Access,
  Arturia Software Center, Waves Central, iLok License Manager, or UA Connect.
  After installing there, run `apm scan` or use the desktop Diagnostics scan.
- Manual: apm opens the product page or download page. Install outside apm,
  then run `apm scan` or the desktop Diagnostics scan so apm can detect the
  plugin on disk later.

### Updates and versioning

```sh
apm outdated            # List plugins with newer versions
apm upgrade             # Upgrade all
apm upgrade surge-xt    # Upgrade one

apm pin vital           # Pin to current version (skip upgrades)
apm pin vital --unpin   # Unpin
apm pin --list          # List pinned plugins
```

### Portable setup

Export your entire setup as a shareable string - paste it in Slack, a README,
or a terminal on another machine:

```sh
apm export                          # Outputs apm1://... string to stdout
apm export -o setup.apmsetup        # Save to file instead

apm import apm1://dGFsLW5v...        # Import from string (preview + confirm)
apm import setup.apmsetup            # Import from file
apm import --dry-run apm1://...      # Preview what would change
apm import --yes apm1://...          # Skip confirmation (for scripts)
```

The string encodes installed plugins, versions, pins, registry sources, and
preferences. Use `apm export --format toml` or `--format json` when you want an
editable file instead of the portable `apm1://...` string.

### System and diagnostics

```sh
apm list                # apm-managed plugins
apm scan                # All AU/VST3 on the system; tracks matched external installs
apm doctor              # Run diagnostic checks
apm cleanup             # Clear download cache
apm rollback <slug>     # Restore from pre-upgrade backup
```

`apm scan` is the bridge for manual and vendor-managed installs: it records what
is already on disk without asking you to re-enter file paths. The desktop
Diagnostics workspace uses the same service-backed scan/reconcile operation.

### Registry sources

apm supports multiple Git-backed registries.

```sh
apm sources list
apm sources add https://github.com/your-org/apm-registry --name my-registry
apm sources remove my-registry
```

### Local service contract

The first localhost service runtime is available as a foreground read/plan
preview for desktop and automation planning:

```sh
apm serve contract
apm --json serve contract
apm serve run
```

The current contract is `v1alpha1` and localhost-only. Read endpoints,
diagnostics reporting, install planning, install handoff resolution, package
pin/unpin, library scan/reconcile operation submission/status, registry sync
operation submission/status, direct URL install operation submission/status,
explicit local archive install operation submission/status, package update
submission/status, package remove operation submission/status, operation
cancellation requests, operation event streaming, recent operation history
listing, restart-interrupted operation recovery summary, persisted operation
history, audio-AI model package listing/search, curated model catalog search,
model store layout, audio-AI model manifest validation, content-based model
manifest caching, cached-model
weight pull operations, cached-model install operations, non-mutating
cached-model run planning, blocked cached-model run operation submission,
cached-model chain planning, and cached-model removal are available.
Protected service routes require the loopback token stored under the service
data dir. The desktop app now has an internal Tauri supervisor that can reuse an
existing service or launch `apm serve run` from `APM_DESKTOP_CLI`, the bundled
`apm-cli` sidecar, or `apm` on `PATH` after the health response, service
contract API/schema, localhost-only policy, full typed contract payload, and
token path validate; its
catalog/library snapshot and diagnostics report now use that service boundary,
along with registry sync, library scan/reconcile, install review, handoff
resolution, pin/unpin, and install/update/remove execution. Registry sync, scan,
and lifecycle actions subscribe to the operation event stream while they run, so
the GUI can show live progress before the final result returns. Queued
operations can be canceled before execution; running operations can record a
`cancel_requested` state, and the
first cooperative checkpoints exist for registry sync plus direct archive
lifecycle work. Package removal also checks cancellation before each tracked
format deletion and repairs partial state when cancellation arrives after a
format was already removed. The desktop UI can request cancellation for visible
registry, install, update, remove, model weight-pull, and model install
operations; model weight pulls check cancellation before and during downloads,
emit byte-count progress events, and clean up canceled `.part` cache files.
Model installs check again before runtime metadata is prepared. Direct archive
downloads also emit byte-count progress events while cleaning up canceled
`.part` cache files.
Direct archive installs also roll back a just-placed bundle if cancellation
arrives before quarantine cleanup or state recording finishes.
Recent operation history is visible in the desktop diagnostics strip and
refreshes after service-backed lifecycle failures as well as successful snapshot
reloads. Persisted operation history keeps the 250 most recent records plus a
bounded recent event tail per record, and `GET /v1/operations` exposes that
recent history to authenticated desktop/service clients. The history file is
updated by writing a same-directory temp file and renaming it into place, so a
failed persistence write cannot truncate the previous recovery snapshot.
`GET /v1/operations/recovery` summarizes restart-interrupted operations and how
many can be retried from saved request metadata; matching desktop history rows
stay visible even outside the newest-three window and are marked as interrupted
and retry-ready when applicable. The desktop history panel can retry all
currently ready recovery candidates with one action while still keeping
per-operation retry controls available. The service contract publishes
`operation_recovery_policy`, which keeps automatic resume disabled and requires
explicit request-backed retries. It also publishes `operation_control_policy`,
pinning the cancel, retry, recovery retry, and progress-stream controls plus the
current operation kinds that support those controls.
`GET /v1/models` lists or searches cached audio-AI model manifests from
`~/.apm/manifests` with their typed IO, params, runtime entry, and weight-cache
status; pass `query` to filter cached package metadata.
`GET /v1/models/catalog` lists or searches curated audio-AI model manifests
from configured registry sources with source, manifest-cache, runtime, params,
and weight-cache metadata before a model is imported into the local store.
`POST /v1/models/catalog/{name}/{version}/cache` imports one curated registry
manifest into the local model store without giving the service arbitrary file
paths.
`POST /v1/models/manifest/validate` validates audio-AI model package TOML from
request content and returns GUI-safe summary metadata.
`POST /v1/models/manifest/cache` validates request TOML and writes it into the local
model store, giving the desktop a selected-file import path without granting the
service arbitrary file reads.
`POST /v1/models/{name}/{version}/weights/pull` pulls the exact cached manifest's
declared weights into the content-addressed model store as an authenticated
operation. `POST /v1/models/{name}/{version}/install` makes a cached model
manifest ready in the local store by verifying existing content-addressed
weights offline or pulling them when missing, then preparing runtime adapter
metadata under `~/.apm/runtimes/<mode>/<name>/<version>`. `DELETE
/v1/models/{name}/{version}` removes a cached manifest and package-specific
runtime metadata, and prunes cached weights only when no remaining manifest
references the same content hash.
`POST /v1/models/{name}/{version}/run/plan` validates that the cached manifest,
content-addressed weights, and prepared adapter metadata are present, then
returns a typed run plan for requested input/output paths and manifest-checked
parameter bindings without executing the model. The plan includes a structured
execution-readiness blocker; current adapters report
`adapter_runner_unavailable` until a real model runner is implemented.
`POST /v1/models/{name}/{version}/run` accepts the same request as an
authenticated operation, records the requested run in operation history, emits a
`model_run_blocked` event for that same execution blocker, and finishes failed
with a structured blocked `model_run` result rather than pretending execution
succeeded. Core execution now goes through a `ModelRunner` boundary; the current
default runner is intentionally unavailable, giving native MLX/Core ML and
managed Python execution one canonical implementation point later. Completed
model-run results are accepted only for plans whose typed execution readiness is
`ready`, so future runners cannot report success for the current blocked plan.
`POST /v1/models/chains/plan` validates prepared model steps, manifest-declared
IO edges, and per-step parameter bindings without executing the chain; it
accepts direct IO edges plus the explicit `stems -> audio` stem-selection edge
needed for separator-to-transcriber workflows.
`GET /v1/models/store` returns the local `~/.apm` model-store layout, and the
desktop snapshot renders those manifest, weight, runtime, cache, log, and config
paths in the Audio-AI panel.
Model pull, install, and run operations record model-specific operation events
in history, model weight pulls include byte-count progress events, and all of
those events stream through
`GET /v1/operations/{operation_id}/events` for desktop progress timelines.
`GET /v1/diagnostics` exposes the shared core doctor report to authenticated
desktop/service clients, including check statuses, details, hints, and summary
counts. Public distribution now has a signing/notarization preflight gate plus
workflow-shape validation, but still needs real release credentials and a
successful desktop release workflow run. See
[Local Service Contract](docs/local-service-contract.md).

### Audio-AI model package foundation

The audio-AI runtime is not complete yet, but the package format foundation is
present. Model packages use manifest files that declare metadata, runtime mode,
weights, typed IO, parameters, license, and hardware requirements.
The local service can also list or search cached model manifests through
`GET /v1/models`, list or search curated registry model manifests through
`GET /v1/models/catalog`, import curated registry manifests through
`POST /v1/models/catalog/{name}/{version}/cache`, validate manifest TOML through
`POST /v1/models/manifest/validate`, cache validated manifest content through
`POST /v1/models/manifest/cache`, pull verified cached-model weights through
`POST /v1/models/{name}/{version}/weights/pull`, install cached model packages
through `POST /v1/models/{name}/{version}/install`, build non-mutating run
plans through `POST /v1/models/{name}/{version}/run/plan`, submit blocked
request-backed model run operations through
`POST /v1/models/{name}/{version}/run`, validate non-executing model chains
through `POST /v1/models/chains/plan`, remove cached model packages through
`DELETE /v1/models/{name}/{version}`, and report the local `~/.apm` store
layout through `GET /v1/models/store` for GUI and automation clients. The
desktop preview renders registry and cached model packages with their IO shape,
runtime mode, manifest-cache state, weight-cache state, and declared
parameters, plus the service-owned `~/.apm` store paths. It can import a
registry manifest or user-selected manifest into the model store, search
models, pull/verify weights, install cached models with prepared runtime
adapter metadata and live operation progress, attempt model runs through the
current structured runner lane, plan model runs for desktop review through the
service endpoint, or remove cached models.

```sh
apm model validate examples/models/demucs.toml
apm model info examples/models/demucs.toml
apm model lock examples/models/demucs.toml -o apm.lock
apm model store --init
apm model list
apm model search stems
apm model search --available stems
apm model pull path/to/model-with-direct-weights.toml
apm model pull name@version
apm model install demucs@4.0.1
apm model run demucs@4.0.1 --input mix.wav --output stems/ --param stems=4
apm model rm demucs@4.0.1
```

This is the start of the "Ollama for audio" layer: native Apple Silicon where
possible, managed Python fallback where necessary, one content-addressed model
store, and reproducible `apm.lock` files.

## Desktop app roadmap

The desktop app will be the producer-facing product. The CLI remains the
scriptable engine.

Planned v3.0 sequence:

1. Reconcile docs and architecture around "Audio Package Manager."
2. Extract GUI-safe shared Rust operations with structured results/progress.
3. Add the macOS desktop app shell and first-run onboarding.
4. Build catalog, library, diagnostics, and runtime screens.
5. Bring install/update/remove/vendor-handoff flows into the GUI safely.
6. Produce signed/notarized macOS release artifacts.
7. Seed the local `apm serve` contract for future GUI, automation, and DAW
   clients, then implement the localhost daemon lifecycle.

## Optional setup

### Shell completions

```sh
# Bash
apm completions bash > ~/.local/share/bash-completion/completions/apm

# Zsh
apm completions zsh > ~/.zsh/completions/_apm

# Fish
apm completions fish > ~/.config/fish/completions/apm.fish
```

## Registry format

The published registry is a Git repo with:

- `registry/index.toml`
- `registry/installers.toml`
- `registry/bundles/*.toml`
- `registry/plugins/<vendor>/<slug>.toml`

`registry/installers.toml` defines vendor manager apps that plugin records can
reference with `installer = "<key>"`. Example:

```toml
[native-access]
name = "Native Access 2"
vendor = "Native Instruments"
app_paths = [
  "/Applications/Native Access 2.app",
  "/Applications/Native Access.app",
]
download_url = "https://www.native-instruments.com/en/specials/native-access/"
homepage = "https://www.native-instruments.com/"
```

Curated install bundles live separately in `registry/bundles/*.toml`; product
bundle SKUs still live in `registry/plugins/<vendor>/<slug>.toml` with
`product_type = "bundle"`.

```toml
slug         = "valhalla-supermassive"
aliases      = ["supermassive"]
name         = "Valhalla Supermassive"
vendor       = "Valhalla DSP"
version      = "5.0.0"
product_type = "plugin"
description  = "Massive reverb and delay with lush modulation."
category     = "effects"
subcategory  = "reverb"
license      = "freeware"
tags         = ["reverb", "delay", "free"]
homepage     = "https://valhalladsp.com/shop/reverb/valhalla-supermassive/"

[formats.vst3]
url          = "https://example.com/ValhallaSupermassiveOSX_5_0_0.dmg"
sha256       = "eaac6d0a24ffed0a02afd1dd06124d12f94716d32a8ac376606aa2d701a70c3e"
install_type = "dmg"
download_type = "direct"
bundle_path  = "ValhallaSupermassive.vst3"

[formats.au]
url          = "https://example.com/ValhallaSupermassiveOSX_5_0_0.dmg"
sha256       = "eaac6d0a24ffed0a02afd1dd06124d12f94716d32a8ac376606aa2d701a70c3e"
install_type = "dmg"
download_type = "direct"
bundle_path  = "ValhallaSupermassive.component"
```

| Field | Required | Description |
|-------|----------|-------------|
| `slug` | yes | Unique identifier used in CLI commands |
| `aliases` | no | Alternate slugs that resolve to this record |
| `name` | yes | Display name |
| `vendor` | yes | Developer or company |
| `version` | yes | Semver or freeform version string |
| `product_type` | yes | `plugin`, `bundle`, `expansion`, `preset_pack`, `sample_library`, `daw`, `utility`, `upgrade`, `subscription`, `template`, or `ebook` |
| `description` | yes | One or two sentence description |
| `category` | yes | Registry category such as `"effects"`, `"instruments"`, or `"daws"` |
| `subcategory` | no | e.g. `"reverb"`, `"synthesizer"`, `"eq"` |
| `license` | yes | SPDX identifier or `"freeware"` |
| `tags` | yes | Search keywords |
| `installer` | no | Vendor manager key from `registry/installers.toml` |
| `homepage` | no | Official product page URL |
| `purchase_url` | no | Product purchase page |
| `releases` | no | Historical versions for explicit installs |
| `bundle_ids` | no | Known CFBundleIdentifier prefixes for scanner matching |
| `formats.*` | at least one | Format-specific download info such as `au`, `vst3`, or `app` |
| `url` | yes | Direct archive URL for `direct` downloads, or the official product/download page for `manual` and `managed` entries |
| `sha256` | for direct downloads | SHA256 hex digest of the direct archive; manual and vendor-managed entries may leave this blank |
| `install_type` | yes | `"dmg"`, `"pkg"`, `"zip"`, or `"mas"` |
| `download_type` | yes | `"direct"`, `"manual"`, or `"managed"` |
| `bundle_path` | for dmg/zip | Path inside the archive to the plugin bundle |

## Contributing plugins

1. Fork this repo.
2. Add or update the relevant registry records:
   - `registry/plugins/<vendor>/<slug>.toml`
   - `registry/installers.toml` when a vendor manager is needed
   - `registry/bundles/*.toml` when bundle membership changes
3. Compute the SHA256 of the macOS installer:
   ```sh
   shasum -a 256 /path/to/installer.dmg
   ```
4. Open a pull request.

Guidelines:
- Only include products that are genuinely installable and not temporary trials.
- Use the official developer download URL, not a mirror.
- If a download requires account signup, note it with a comment in the TOML.
- Mark non-standalone catalog entries with `product_type` so search results stay honest.

## License

apm is released under the MIT License. See [LICENSE](./LICENSE) for details.
