# apm Architecture

## Product Shape

`apm` is a desktop-first audio package manager with a Rust engine underneath.
The GUI should be the normal producer-facing experience; the CLI should stay as
the scriptable/operator surface; the core crate should own package-manager
behavior.

The main architectural rule is simple: do not build a second package manager in
the GUI. The desktop app should call shared engine operations or a local service
boundary that exercises the same behavior as the CLI.

## Current Workspace

Current tracked workspace crates:

- `crates/apm-core`: configuration, registry loading, scanner/state logic,
  install metadata types, shared engine read models, and model package
  primitives.
- `crates/apm-cli`: command-line UX, terminal formatting, download/install
  orchestration, import/export, diagnostics, and command handlers.

Current non-code package assets:

- `registry/`: TOML package definitions and installer metadata.
- `Formula/`: Homebrew formula for the CLI release.
- `.github/workflows/`: CI, CLI release, and manual desktop release workflows.
- `docs/`: product, runtime, and architecture notes.

Historical planning files mention `apm-server`, authentication, purchases, and
storefront flows. Those are not present in the current tracked Cargo workspace.
Treat those as historical direction until the codebase has an actual server
crate again.

## Target Layers

### Desktop App

Producer-facing Mac app. It should own presentation, navigation, onboarding,
progress display, confirmations, and recovery guidance.
The current shell now keeps Catalog, Library, Diagnostics, and Runtime as typed
workspace sections behind the sidebar navigation.
First-run onboarding is a snapshot-derived readiness panel, not a separate
package-management path; setup actions reuse the same service start and registry
sync handlers as the main UI.
Distribution-channel readiness is also desktop-owned presentation: the Tauri
snapshot reports whether the app is a browser preview, development build,
preview bundle, or public-release build, and the Diagnostics workspace renders
that state without duplicating the signing/notarization verifier.

It should not own install rules, registry parsing, scanner behavior, or package
state mutations.

### Shared Engine

Rust operations that can be called by both CLI and GUI:

- registry sync/search/info
- scan/list/doctor
- install/remove/update/pin
- import/export
- model manifest/lockfile/store operations

The engine boundary should return structured results and structured errors.
Long-running operations should emit progress events rather than printing from
deep inside business logic.

Current shared engine surface:

- `ApmEngine::search_packages`
- `ApmEngine::package_details`
- `ApmEngine::installed_packages`
- `ApmEngine::scan_packages`
- `ApmEngine::sync_registries`
- `ApmEngine::plan_install`
- `ApmEngine::install_handoff`
- `ApmEngine::install_package_from_archive`
- `ApmEngine::install_package_from_url`
- `ApmEngine::remove_package`
- `ApmEngine::available_updates`
- `ApmEngine::update_package`
- `ApmEngine::pinned_packages`
- `ApmEngine::set_package_pin`

The package/catalog/library methods establish structured read models for the
desktop catalog, package detail, and library screens. `scan_packages` is the
shared AU/VST3 reconciliation bridge: CLI scan and the desktop Diagnostics
action use the same matcher, bundle-ID learning, external-install adoption, and
typed scan events. `sync_registries` is the first progress-enabled operation:
it returns per-source outcomes and emits typed `EngineEvent` values so a GUI can
render progress without scraping CLI text. `plan_install` is the first lifecycle
bridge: it resolves installable products into a GUI-safe review result covering
direct installs, already-installed packages, manual download handoff, and vendor
manager handoff. `install_handoff` derives the external target for manual
downloads or vendor-manager installs so the desktop can perform those handoffs
without duplicating package rules. `install_package_from_archive` is the first
disk-mutating lifecycle bridge: it installs one explicitly selected direct
archive through shared core code, emits typed install events, records state,
and rolls back placed bundles if the operation fails before state is saved.
`install_package_from_url` layers URL staging onto that same archive executor
for direct ZIP and DMG packages: it downloads into the cache,
validates checksums before placement, deletes bad downloads, and then follows
the same record/rollback path as local archives.
Download/cache/progress mechanics live in the dedicated engine download module
so the archive executor stays focused on placement, rollback, and state
recording.
`remove_package` is the matching shared removal bridge: it removes apm-owned
tracked bundles, cleans stale external state entries only after files are gone,
and refuses to delete live scan-discovered external installs.
`available_updates` is the first shared update-read bridge: it compares
installed packages against registry releases and classifies updates as
installable, pinned, or external so CLI and desktop views do not duplicate
version comparison rules. `pinned_packages` and `set_package_pin` make pinning
another shared state operation: the CLI and desktop toggle now use the same
state mutation, and update eligibility is derived from the shared update model.
`update_package` is the first shared update-execution bridge: it applies those
eligibility rules, skips pinned and external installs, infers a single tracked
installed format when possible, rejects partial updates for multi-format
installs, and updates all tracked direct archive formats together through the shared
download and placement executor. Privileged PKG execution and Mac App Store
handoff policy stay out of the mutating archive executor; the service contract
now exposes PKG execution as `external_handoff_only` with explicit confirmation
required and no apm-run PKG installers. The desktop shell has the first native
file-picker, direct download/install confirmation actions, live event-stream
progress, final event timelines for explicit ZIP/DMG archive installs, direct
archive updates, and apm-owned removals, plus pin/unpin controls.

### CLI

Scriptable/operator interface over the same engine. It can own terminal
formatting, clap argument parsing, prompts, and JSON output, but not separate
business rules.

### Local Service

Future `apm serve` layer for GUI, automation, and DAW/plugin clients. The
service is localhost-only and versioned. It should expose health, package/model
lists, operation submission, and progress streaming.

The initial foreground runtime is now available as a localhost preview:

```bash
apm serve contract
apm --json serve contract
apm serve run
```

`apm-core::service::local_service_contract` is the canonical typed contract.
The CLI renders it and hosts the first Axum adapter over existing `apm-core`
engine reads. The desktop supervisor validates the same schema, auth, localhost
binding, and full typed contract payload before reusing or accepting a
foreground service. Registry sync is the first in-memory background operation exposed
through `/v1/registry/sync`, `/v1/operations/{operation_id}`, and
`/v1/operations/{operation_id}/events`; package pin/unpin is the first
state-changing package route exposed through the service. Library scanning is
also a service operation through `/v1/library/scan`, giving the desktop a
first-class reconcile action after external handoffs without duplicating scanner
or matcher logic. The service also resolves manual/vendor/PKG/App Store install
handoff targets without opening them; the desktop app remains responsible for
explicit user confirmation and OS-level launching.
Direct URL install, explicit local archive install, package update, and package
remove are disk-mutating package operations submitted through the service
operation/event model, and terminal operation history is persisted across
foreground service restarts. Install planning also marks direct PKG packages as
privileged external handoffs and Mac App Store packages as App Store handoffs,
so service and desktop callers can route those flows without pretending they
are executable archives. Non-public routes require the issued loopback token.
Accepted operation responses now carry the operation kind alongside the
operation ID and status URL, so desktop progress routing follows the service
operation model instead of inferring scope from engine-event name prefixes.
The desktop app now has an internal Tauri supervisor that derives
service state from health checks plus the child process it launched: it reuses
a healthy localhost service, or starts `apm serve run` from `APM_DESKTOP_CLI`,
the bundled `apm-cli` sidecar, a neighboring `apm` binary that is not the
desktop executable, or `apm` on `PATH`. The desktop snapshot path now uses that
service boundary for catalog, installed library, and available-update data.
Desktop registry sync also uses the service operation model by submitting
`/v1/registry/sync`, subscribing to the operation event stream for live
progress, and reading terminal operation status for the typed result. Install
review, install handoff resolution, library scan/reconcile, pin/unpin, direct
install, archive install, update, and remove now use service endpoints; mutating
lifecycle commands also stream operation events into the GUI while they run. The
operation model now
has a cancellation request endpoint: queued operations can become `canceled`,
and running operations can become `cancel_requested`. Initial cooperative
checkpoints cover registry sync, direct archive install/update paths,
direct-download loops, and package removal before each tracked format deletion.
If cancellation arrives after removal has already deleted one format, apm repairs
install state to keep only the formats still present on disk. The desktop UI can
request cancellation for visible registry, install, update, and remove
operations once their service operation ID is known. The Audio-AI panel uses the
same operation-control path for visible model weight-pull, model-install, and
model-run operations; model weight pulls check cancellation before and during
downloads, emit byte-count progress events, model installs check again before
runtime metadata is prepared, and model runs check cancellation before planning
and before the current blocked terminal result is returned. Direct archive
downloads emit byte-count progress events
into the lifecycle timeline while removing canceled `.part` cache files. Direct
archive installs roll back a just-placed bundle if cancellation is observed
before quarantine cleanup or state recording completes. Persisted operation
history keeps the 250 most recent records plus a bounded recent event tail per
record, so service state stays bounded as progress events become more detailed.
History persistence uses a same-directory temp file and rename before replacing
`operations.json`, keeping the previous recovery snapshot intact if a write
fails. Authenticated clients can list the recent operation history with
`GET /v1/operations`, and the desktop preview now shows that service-backed
history in a compact diagnostics strip. Newly accepted operations persist the
typed request payload that created them, and the desktop history strip marks
records with that metadata. Failed or canceled records with saved request
metadata can now be retried through
`POST /v1/operations/{operation_id}/retry`, creating a new operation from the
original request while rejecting succeeded, active, and legacy no-request
records. The desktop history strip exposes that retry action for eligible
records, subscribes to the retried operation's event stream, and refreshes
history after terminal status is read. This is explicit user retry, not
automatic restart resume. `GET /v1/operations/recovery` provides a compact
service-owned recovery summary for foreground-service restart interruptions,
including how many interrupted records still have retry metadata. Desktop
operation history renders those recovery candidates as interrupted rows with
retry-ready or manual-review state, keeps them visible even when they are older
than the newest-three history window, and can retry all currently ready
recovery candidates through the same per-operation retry path. The contract now
publishes `operation_recovery_policy`, so automatic resume is explicitly disabled
and request-backed retry semantics are machine-readable. It also publishes
`operation_control_policy`, naming the cancel/retry/recovery-retry endpoints,
the operation event stream, the `cancel_requested` running state, and the
current operation kinds that expose cancellation/progress controls. Failed lifecycle
operations refresh the same snapshot path so the diagnostics strip can reflect
the terminal service state instead of staying on stale preview data.
The core diagnostics report now lives in `apm-core::diagnostics`; `apm doctor`
prints that shared report, `GET /v1/diagnostics` exposes it to authenticated
service clients, and the desktop snapshot renders warning/failure checks with
their remediation hints.
The service also exposes the first audio-AI package review boundaries:
authenticated `GET /v1/models` lists or searches cached model manifests from
`~/.apm` with typed IO, params, runtime entry, and weight-cache status, while
authenticated `GET /v1/models/catalog` lists or searches curated model
manifests from configured registry sources before they are imported into the
local model store. Registry model discovery lives in `apm-core::model::catalog`
and reuses the same configured source resolution as plugin registry loading.
Authenticated `GET /v1/models/store` reports the service-owned model-store root
and subpaths, and the desktop snapshot renders that layout in the Audio-AI panel
beside catalog and cached-package state. Authenticated
`POST /v1/models/store/init` is the first desktop-safe store mutation: it calls
the shared `ModelStore::ensure()` implementation to create missing directories,
while the shared diagnostics report keeps first-launch verification read-only
until the user asks to initialize the store.
Authenticated `POST /v1/models/catalog/{name}/{version}/cache` imports a
registry manifest into the local model store without exposing arbitrary
service-side path reads to desktop clients.
`POST /v1/models/manifest/validate` validates manifest TOML from request
content and returns summary metadata for future GUI review.
`POST /v1/models/manifest/cache` validates request TOML and writes it into the local
model store without service-side arbitrary path reads. The desktop snapshot
consumes the model list through the same authenticated local-service client and
renders it as a model package panel with selected-file manifest import,
cached search, pull/verify weight actions, cached-model install readiness, and
cached-model removal that preserves shared content-addressed weights.
Authenticated `POST /v1/models/{name}/{version}/run/plan` builds a
non-mutating runtime plan from a cached manifest, verified weight blob, and
prepared `adapter.toml` metadata without executing model code. The adapter
metadata is parsed and checked against the cached package, runtime mode, entry,
adapter ID, weight hash, and local weight path before planning succeeds.
Requested parameter values are validated and coerced against the manifest in
core before they reach CLI, service, or desktop callers, giving the desktop
boundary an honest handoff shape before native runtime sessions exist. The plan
also includes typed execution readiness; current adapters report
`adapter_runner_unavailable` so the GUI can show that execution is still blocked
without inferring that state from prose.
Authenticated `POST /v1/models/{name}/{version}/run` accepts the same request
as a request-backed operation, records it in operation history, emits
`model_run_started` plus `model_run_blocked` for the current blocker, and marks
the operation failed until a real adapter runner can produce an execution
result. The service and desktop contract already exposes `model_run_completed`
and the `completed` result status for that future success path. Core model-run
execution is routed through a `ModelRunner` boundary, and completed results are
accepted only when the plan's typed execution readiness is `ready`. Future
MLX/Core ML/Python runners therefore have one implementation point without
leaking adapter execution into service operation plumbing or reporting success
for a still-blocked plan.
Authenticated `POST /v1/models/chains/plan` validates a non-executing sequence
of prepared model steps. It checks that each cached manifest has verified
weights and matching prepared runtime metadata, binds each step's
manifest-declared parameters, validates direct IO edges, and models
`stems -> audio` as an explicit stem-selection handoff for
separator-to-transcriber chains.
The desktop Audio-AI panel now invokes the run-plan endpoint from cached model
cards after choosing an input audio file and output folder through native
dialogs, then renders the returned adapter/runtime/weights/input/output/execution plan
for review.
It can also submit the request-backed model-run operation as an execution
readiness check, stream the started/blocked events, and render the same
structured blocked result in operation history without suggesting that adapter
execution exists yet. That operation now shares the same cancellation contract:
cancellation is checked before planning and before the blocked result is
recorded by the default unavailable runner, so future runtime adapters have a
canonical control point to extend.
The contract also publishes `security.privileged_install_policy`, making the
current PKG policy machine-readable: direct PKG formats are external privileged
installer handoffs, require user confirmation, and are not executed by apm until
a signed privileged helper and receipt-backed rollback path are implemented.
The same policy now carries typed prerequisite gates and the selected
helper/rollback strategies, so the GUI and bundled sidecar verifier can
distinguish designed helper/rollback gates from required consent, package
verification, and audit-trail behavior before any privileged execution path is
enabled. The selected design names `com.apm.pkg-helper` as the future signed
helper, `/Library/PrivilegedHelperTools/com.apm.pkg-helper` and
`/Library/LaunchDaemons/com.apm.pkg-helper.plist` as the helper artifacts, and
`service/privileged-install-receipts.json` as the receipt-backed rollback store
relative to the service data dir. Core owns that store as a typed v1 JSON model
for package/source, installer checksum, preflight snapshot, pkgutil receipt,
installed-path, operation, and timestamp metadata, but no current endpoint writes
it or executes PKG installers. The Tauri service supervisor carries the
validated policy into the desktop session, and the diagnostics view renders that
session policy as the visible installer-safety state. The same diagnostics flow
now checks the future helper binary and launchd plist paths and warns if stale
helper artifacts are present while the contract still disables PKG execution;
the desktop summary promotes that doctor check into a dedicated helper-artifacts
card so the normal "absent" state is visible too. Browser preview data mirrors
the same shape only as sample data.
The desktop supervisor validates both `/v1/health` and the public
`/v1/service/contract` before it reuses or accepts a launched foreground
service. The GUI session now carries the local service API and schema version
into the runtime panel and diagnostics card, making stale sidecars and contract
mismatches visible at the desktop boundary.
Developer ID
signing and notarization release gates now exist for the sidecar-bearing desktop
bundle, and artifact verification covers preview structure plus signed/stapled
release artifacts. A manual CI workflow can build/upload or publish verified
desktop artifacts once the `macos-desktop-release` environment secrets are
configured. Local preflight now validates the checked-in workflow safety rails,
but a first signed/notarized workflow run, broader cancellation coverage, and
progress polish for future package types remain pending.
The first install/update/uninstall/troubleshooting policy is captured in
`docs/macos-release-runbook.md` and keeps app uninstall separate from package
removal.

### Audio-AI Runtime

Package type and runtime layer for useful local audio models. The early
foundation is intentionally small:

- manifest schema
- reproducible lockfile
- `~/.apm` store layout rendered in the desktop Audio-AI panel
- curated registry model catalog discovery
- curated registry manifest import into the local model store
- verified, content-addressed model weight pull for cached manifests
- cached model install readiness over local manifests and weights
- prepared runtime adapter metadata under `~/.apm/runtimes`
- non-mutating model run planning from prepared runtime metadata
- blocked model run operation submission with saved request metadata
- non-mutating model chain planning over prepared steps and typed IO edges
- cached model package removal with runtime-metadata cleanup and shared-weight
  preservation
- model pull/install events in operation history, SSE streams, and the desktop
  Audio-AI timeline, with cancellation checkpoints before/during weight pulls
  and byte-count progress during weight pulls, plus cancellation checkpoints
  before runtime metadata provisioning
- model run started/blocked/failed events in service history, SSE streams, and
  desktop progress, with cancellation checkpoints before planning and before
  the current blocked result is recorded

Executable Native MLX/Core ML adapters, managed Python environment creation,
runtime execution progress, and chain execution remain follow-on runtime work.

## Desktop Stack Default

The planning default is Tauri 2 because it can keep Rust close to the app shell
and can bundle external binaries as sidecars via `bundle.externalBin`.

This is not irreversible. Swift/SwiftUI remains the fallback if deep native
macOS behavior becomes more valuable than Rust reuse, but the first build should
prefer the stack that best protects the existing package-manager core from
duplication.

References:

- Tauri sidecars: https://v2.tauri.app/develop/sidecar/
- Tauri macOS app bundle layout: https://v2.tauri.app/distribute/macos-application-bundle/

## Boundary Rules

- Keep package behavior in `apm-core` or a dedicated engine module, not in UI.
- Keep terminal formatting in `apm-cli`, not in core.
- Keep GUI display models separate from registry/state persistence types.
- Keep operation progress in the shared event model as GUI flows expand.
- Prefer adding behavior to the shared engine first, then adapting CLI/GUI
  presentation around it.
- Do not scatter `if gui` or `if cli` conditionals through existing command
  handlers.
- Do not let desktop work push already-large command files further toward
  unreviewable size; extract engine operations first.
