# apm Desktop Product Plan

## Premise

`apm` is becoming a macOS-first audio package manager with a real desktop app
and installer experience. The GUI is the front door for producers; the existing
Rust CLI/core remains the scriptable engine underneath.

This is not just a prettier CLI. The product should let a user install one
trusted Mac app, browse audio tools, install/update/remove packages, reconcile
vendor-managed installs, and eventually run local audio-AI model packages
without caring about plugin folders, checksums, Python environments, or model
weight caches.

## Product Layers

1. Desktop app and installer
   - Browse/search package catalog
   - Inspect package details
   - View installed library and health
   - Run scans/diagnostics
   - Install, update, remove, pin, and repair packages

2. Shared Rust engine
   - Current `apm-core` behavior becomes GUI-callable operations
   - CLI commands keep using the same behavior
   - Long-running work emits progress events
   - Errors become structured enough for GUI display

3. Local service boundary
   - `apm serve` becomes the future API for GUI, automation, and DAW/plugin
     clients
   - Localhost-only by default
   - Progress streaming for long tasks

4. Audio-AI package/runtime layer
   - Model manifests define runtime mode, weights, typed IO, params, license,
     and hardware requirements
   - `apm.lock` pins model versions, modes, weight hashes, and sources
   - `~/.apm` stores manifests, weights, runtimes, cache, logs, and config
   - Native MLX/Core ML first, managed Python fallback for the long tail

## v3.0 Roadmap

### Phase 15: Product Realignment and Architecture Reconciliation

Reframe docs and README around "Audio Package Manager"; reconcile old
server/storefront planning with the actual current workspace; document the
desktop/CLI/core/runtime boundaries.

### Phase 16: Shared Engine and Model Package Substrate

Extract GUI-safe operations from CLI behavior, add structured results/progress,
keep CLI parity, and make the model manifest/lockfile/store foundation a tested
part of `apm-core`.

Initial read-only engine surface exists in `apm-core`: package search, package
details, and installed library summaries. Lifecycle mutations still need a
progress/event model before they should be exposed to the desktop app.

### Phase 17: macOS Desktop App Foundation

Add the desktop app workspace/package, launch it locally, show first-run
onboarding, and prove it can call at least one real read-only engine operation.

The desktop shell now renders a compact first-run readiness panel above every
workspace when service, registry, diagnostics, or store state still needs
attention. The panel is derived from the same snapshot as the rest of the GUI
and reuses existing Start Service and Sync actions instead of adding a separate
onboarding flow. Doctor warnings and failures jump to the Diagnostics workspace
for the same repair guidance used elsewhere in the app. Audio-AI store
readiness is backed by the shared doctor report instead of unverified path
strings, and the setup panel can call the authenticated model-store initializer
so first-run users can create missing `~/.apm` directories without opening
Terminal. Service-backed setup repairs stay disabled until the local service is
authenticated, while diagnostics navigation remains available for failure
context.

### Phase 18: Catalog, Library, and Diagnostics GUI

Implement browse/search/filter, package detail pages, installed library state,
and scan/doctor diagnostics in the desktop app.

The desktop catalog now has in-app text search over package identity, vendor,
category, license, description, and format fields, plus status, product-type,
and access filters. Filtering updates the visible package table, catalog
metric, selected package inspector, and empty state without leaving the desktop
shell. The sidebar now switches between real Catalog, Library, Diagnostics, and
Runtime workspaces instead of acting as decorative navigation. The
selected-package inspector now owns the package detail view and
shows description, homepage/purchase links from the service-backed package
details endpoint, aliases, known versions, learned bundle IDs, type/access,
install state, and per-format
download/install/checksum/path details without bloating the top-level desktop
view renderer. The installed library now renders explicit per-package health
badges for current, update-ready, pinned, and external states next to version,
format, origin, pin, update, and remove controls. The desktop
diagnostics surface now includes a state-derived
readiness summary for the local service, registry, catalog, installed library,
available updates, and doctor checks. `apm serve` now exposes a protected
`GET /v1/diagnostics` endpoint backed by the shared core diagnostics report,
and the desktop snapshot renders warning/failure doctor checks with remediation
hints below the readiness cards. Diagnostics can now rerun the service-backed
doctor snapshot from the GUI and show refresh success or service errors without
opening Terminal. Diagnostics can now also launch the shared
library scan/reconcile operation, stream scan progress, and refresh the
installed library after matched external installs are adopted.

### Phase 19: Safe Install, Update, Remove, and Vendor Handoff UX

Bring lifecycle operations into the GUI with visible progress, explicit
privilege/vendor confirmations, recoverable failure states, pinning, and update
flows.

Initial lifecycle foundation exists as a shared install-plan operation:
`ApmEngine::plan_install` lets the desktop app review direct installs,
already-installed packages, manual download handoffs, and vendor manager
handoffs without duplicating CLI install rules or mutating disk state.
`ApmEngine::install_handoff` adds the safe external action for manual and
vendor-managed packages. The first direct archive executor now exists as
`ApmEngine::install_package_from_archive`: it handles an explicitly selected
local ZIP or DMG archive for one format, emits typed progress events, records
state, and rolls back placed bundles when state recording fails. The desktop
shell now surfaces that executor through per-format archive picker actions in
the install review panel, followed by a confirmation sheet before disk
mutation, live event-stream progress while the operation runs, and a compact
final event timeline after completion or hard failure.
`ApmEngine::remove_package` now owns the matching removal path for apm-managed
installs and stale external state entries, while refusing to delete live
scan-discovered external/vendor installs; the desktop library panel exposes it
behind a confirmation sheet, live event-stream progress, and a final event
timeline.
`ApmEngine::available_updates` now classifies installed packages against the
registry as installable, pinned, or external updates, and the desktop library
surfaces those update states with a review action that reuses the shared install
plan path. `ApmEngine::set_package_pin` and `ApmEngine::pinned_packages` now
own pin state for both CLI and desktop, and the library panel exposes a pin
toggle that immediately changes update eligibility. `ApmEngine::install_package_from_url`
now adds the first direct-download path for ZIP and DMG packages: it stages the
archive in the download cache, verifies checksums, deletes bad downloads, and
then reuses the same placement/state-recording executor as local archives. The
desktop install review exposes that path behind an explicit confirmation sheet.
`ApmEngine::update_package` now owns one-package update execution for
apm-managed installs: it reuses `available_updates` eligibility, skips pinned
and external packages, infers a single tracked format when possible, updates
all tracked formats together for multi-format direct archive installs, and keeps
partial multi-format updates blocked so state cannot drift across versions.
The desktop library now exposes installable updates behind a confirmation
sheet, renders tracked formats, shows live event-stream progress, and records a
final event timeline for executable updates. The install plan now treats
direct PKG formats as privileged external handoffs and Mac App Store formats as
App Store handoffs, so the GUI no longer labels those paths as ready for
shared-engine mutation. The desktop now requires explicit confirmation before
opening manual downloads, vendor managers, PKG download targets, or App Store
listings; PKG handoffs remain external and apm still does not run privileged
installers itself. The local service contract now exposes that as typed
`security.privileged_install_policy` data, so desktop clients can show the policy
without re-deriving it from install-plan status text. The policy now includes
machine-readable prerequisite gates and a reviewed design direction: future PKG
execution uses a signed privileged helper, and rollback uses receipt-backed
uninstall metadata for helper-installed packages. The contract now names the
planned helper as `com.apm.pkg-helper`, with
`/Library/PrivilegedHelperTools/com.apm.pkg-helper`,
`/Library/LaunchDaemons/com.apm.pkg-helper.plist`, Developer ID signing, and a
receipt store at `<data_dir>/service/privileged-install-receipts.json` as the
future rollback boundary. The core service layer now owns that store as a typed
v1 JSON `PrivilegedInstallReceiptStore` with package, installer checksum,
preflight snapshot, pkgutil receipt, installed-path, and operation metadata;
future helper execution still has to record those receipts before any disk
mutation. Explicit user consent, package verification, and audit trail capture
remain required before any future privileged execution. Direct ZIP/DMG desktop
install confirmations now expose an explicit user/system destination selector
and send the selected `scope` through the Tauri local-service request boundary.
Remaining Phase 19 privileged work centers on implementing that helper and
wiring receipt capture into the privileged mutation path. The desktop
diagnostics panel now renders the same policy from the service session, so users
can see that PKG paths are external handoffs, which privileged-execution
artifacts have a design, and whether the future helper binary/plist are absent
as expected or unexpectedly present.
Manual,
vendor-managed, PKG, and App Store handoffs now have an in-app follow-up:
Diagnostics submits the service-backed library scan so newly installed bundles
can be matched and tracked without asking the user to run `apm scan` manually.

### Phase 20: Signed macOS Distribution Path

Produce a coherent app plus CLI/helper artifact, document signing/notarization,
and define install, uninstall, update, and troubleshooting paths.

The first local packaging path now exists: `npm run bundle:macos` from
`apps/apm-desktop` produces a local Tauri `apm.app` and DMG preview artifact
under `target/release/bundle/`, with a bounded build timeout so the initial
generated DMG step fails clearly instead of hanging indefinitely. The bundle
now stages the release `apm` CLI as a Tauri `externalBin` sidecar and embeds it
as `Contents/MacOS/apm-cli`, giving the desktop app a concrete local-service
process policy for preview bundles.
The first public-release gate also exists:
`npm run bundle:macos:release` validates Developer ID signing and notarization
inputs, writes an ignored Tauri config overlay, and refuses to build public
desktop artifacts without those inputs. Tauri bundle builds now stage the
sidecar, run desktop unit tests, and build the frontend before bundling.
`npm run verify:macos:preview` checks local preview bundle structure,
local code signature, sidecar behavior, and the bundled sidecar's JSON service
contract compatibility with the desktop app. `npm run bundle:macos:verified`
ad-hoc signs the preview app, rebuilds the preview DMG around that signed app
with a bounded generated `bundle_dmg.sh` timeout plus temp/scratch-DMG cleanup,
and then runs `npm run verify:macos:preview:dmg`. The DMG verifier also mounts
the local DMG read-only and confirms it contains `apm.app` with the desktop
executable, sidecar, Info.plist, an `Applications -> /Applications` install
target, and a valid local signature. `npm run verify:v3:local` now gathers the
local checkpoint into one command: workspace tests, release preflight, verified
preview bundle, release evidence packaging, checksum verification, and
tracked plus untracked file whitespace checks. The checkpoint command also
rejects unknown arguments before support checks, tests, builds, or release
packaging begin.
`npm run verify:macos:release` still requires Developer ID signing, Gatekeeper
acceptance, notarization stapling, DMG integrity, and the sidecar-bearing app
layout. `.github/workflows/desktop-release.yml` now
provides a manual CI path for signed/notarized desktop artifacts: it checks out
a tag, provisions a temporary signing keychain and notarization API key from
the protected `macos-desktop-release` environment, runs the release
build/verifier, verifies the generated app zip payload before checksum
publication, uploads dry-run artifacts, and attaches them to the GitHub
Release only when `publish` is true. `npm run release:macos:check` now also
validates Cargo CLI/desktop crate, Tauri, and desktop package version parity
and verifies the local release support files, package scripts, and desktop
workflow release-channel safety rails. `npm run
release:macos:github-bootstrap` creates or updates the remote release
environment shell, and `npm run release:macos:github-check`
verifies that the environment exposes the required secret names before the
first signed CI attempt. `npm run release:macos:github-secrets` now validates,
applies, and verifies those secrets from local environment values without
printing them, and `npm run release:macos:github-secrets-template` prints a
safe local env template with the required secret names, base64 generation
commands, and dry-run/upload/check/status sequence; with
`-- --output ../../.env.release.local` it writes the ignored template with
private permissions and refuses to overwrite an existing file. `npm run
release:macos:workflow-check` and
`npm run release:macos:workflow-dispatch` now gate the first signed dry-run on
remote workflow visibility, release-tag presence, and environment secret
inventory. `npm run release:macos:workflow-accept` then verifies a specific
completed manual `publish=false` `Desktop Release` workflow run by default,
downloads its named `apm-desktop-<tag>` artifact set, and runs the same local
release acceptance checks against that download. Publish dispatch now carries
the accepted dry-run ID into the workflow, and the workflow itself validates
that `accepted_run_id` before attaching assets to a GitHub Release.
The remote environment and desktop workflow exist now, but public distribution
still requires configured environment secrets, the release tag to point at the
final release commit, and a first verified workflow run. The first
install/update/uninstall/troubleshooting policy is documented in
`docs/macos-release-runbook.md`: DMG install, manual app replacement for
updates, deleting `/Applications/apm.app` for app uninstall, and preserving
installed plugins, package state, registry cache, backups, and model store data
unless the user explicitly chooses a full data reset.

### Phase 21: Local Service Contract and v3 Integration Verification

Specify/seed `apm serve`, verify CLI/GUI parity against fixtures, run release
readiness checks, and record the follow-on runtime/DAW/marketplace work.

The first local service contract is seeded in `apm-core::service` and exposed
through `apm serve contract` / `apm --json serve contract`. `apm serve run` now
starts a foreground localhost preview for health, contract,
catalog/library reads, available updates, install planning, model-store
inspection, manual/vendor/PKG/App Store install handoff resolution, package
pin/unpin, direct URL install operation submission, explicit local archive
install operation submission, package update operation submission, package
remove operation submission, library scan/reconcile operation submission,
registry sync operation submission, operation status, operation event streaming,
operation cancellation requests, and persisted operation history across
foreground service restarts. The foreground daemon now issues a
loopback token and requires it for non-public routes. The Tauri
desktop shell now has an internal local-service supervisor: on launch it can
reuse an already-running localhost service or start `apm serve run` from
`APM_DESKTOP_CLI`, the bundled `apm-cli` sidecar, a neighboring `apm` binary
that is not the desktop executable, or `apm` on `PATH`, then report service
readiness, contract schema, API version, and token availability in the runtime panel.
The supervisor validates both `/v1/health` and `/v1/service/contract`, including
schema, auth, localhost binding, and the full typed contract payload, before
reusing or accepting a launched foreground service, so a stale sidecar or old
developer service becomes an explicit unavailable state instead of a vague
request failure. The first desktop read path now uses that
boundary: the Tauri `desktop_snapshot` command loads catalog, installed
library, available-update data, and distribution-channel readiness through one
typed snapshot instead of scattering readiness facts across the GUI. Package
data still comes from authenticated `apm serve` endpoints instead of calling the
engine directly. Desktop registry sync now uses the same boundary by submitting
`/v1/registry/sync`, subscribing to
`/v1/operations/{operation_id}/events` for live progress, and reading the
terminal status for a typed `RegistrySyncResult`. Install review,
manual/vendor/PKG/App Store handoff resolution, library scan/reconcile,
pin/unpin, direct install, archive install, update, and remove now call
authenticated service endpoints as well; mutating lifecycle commands stream
operation events into the GUI while they run and still render the final
recorded event timeline after completion. Operation
cancellation is now available at the service boundary: queued operations can be
canceled before execution and running operations can record a
`cancel_requested` state. Initial cooperative checkpoints now cover registry
sync, direct archive install/update paths, direct-download loops, and package
removal before each tracked format deletion. If cancellation arrives after one
package format has already been deleted, apm repairs install state to keep only
the formats still present on disk. The desktop UI can request cancellation for
visible registry, install, update, remove, model weight-pull, model install,
and model run operations once the service operation ID is known. Model weight
pulls check cancellation before and during downloads, emit byte-count progress
events, model installs check again before runtime metadata is prepared, and
model runs check cancellation before planning and before returning the current
structured blocked result. The core run path now has a dedicated `ModelRunner`
boundary, with the current default runner intentionally returning
`adapter_runner_unavailable` until native MLX/Core ML or managed Python runners
exist. Direct archive downloads now emit byte-count
progress events into the same lifecycle timeline and remove canceled `.part`
cache files. Archive execution emits typed
checkpoints for archive
handling, quarantine removal, and state recording. If cancellation arrives after
archive placement but before quarantine cleanup or state recording completes,
the just-placed bundle is rolled back before the operation returns. Persisted operation
history keeps the 250 most recent records plus a bounded recent event tail per
record so progress-rich timelines do not grow the foreground service state file
without bound. History writes now use a temp file plus same-directory rename so
the previous recovery snapshot is not truncated by a failed persistence write.
Authenticated clients can list that recent history through `GET /v1/operations`,
and the desktop preview now renders the same history in a compact diagnostics
strip. Accepted operation responses now include the operation kind as well as the
generated ID and status URL, so desktop progress routing can follow service
operation scope instead of inferring it from event-name prefixes.
Distribution-channel readiness now renders in the Diagnostics workspace too:
browser previews, development builds, preview bundles, and public-release builds
have distinct states, while Developer ID signing and notarization acceptance
remain enforced by the release builder and verifier instead of being reimplemented
in the GUI. Existing preview artifacts can now be launched through
`npm run open:macos:preview` or mounted through
`npm run open:macos:preview:dmg`, with both commands checking that the local
artifact exists before calling macOS `open`. Both commands now support
`--dry-run`, and the v3 local checkpoint uses that path to smoke the runnable
preview app and DMG without opening the app during automated verification.
Preview bundles now also show an explicit public-release checklist in the same
Diagnostics workspace, and public-release builds still call out
`verify:macos:release` as the proof point rather than treating the selected
channel as notarization evidence. The checklist now starts with
`release:macos:status -- --markdown`, keeping the GUI front door aligned with
the non-dispatching blocker, next-step, and handoff-note report before a
workflow dispatch. That same status report can render as markdown so release
handoff notes can be generated from live checks instead of manually retyping the
blocker inventory, and its secret-setup step now names the dry run before
uploading environment secrets. The
Diagnostics workspace also renders the service contract's `pending_runtime_work`
summary as an informational v3
integration card plus a full detail list, so release credentials, privileged
helper execution, and real runtime adapter gaps stay visible from the GUI front
door.
Newly accepted operations now persist the typed request payload that created
them, and the desktop history strip marks records where that request metadata is
available. Failed or canceled records with saved
request metadata can now be retried through
`POST /v1/operations/{operation_id}/retry`, creating a new operation from the
original request; succeeded, active, and legacy no-request records are rejected.
The desktop history strip exposes that retry action for eligible records,
subscribes to the retried operation's event stream, and refreshes history after
terminal status is read. This is explicit user retry, not automatic restart
resume. `GET /v1/operations/recovery` now summarizes foreground-service restart
interruptions and identifies how many interrupted records have saved request
metadata, so the desktop diagnostics can surface recoverable work without
re-deriving the policy from raw history rows. Recent operation history keeps
matching recovery candidates visible even outside the newest-three window, marks
them as interrupted, and distinguishes retry-ready records from manual-review
records. The service can now retry all currently ready recovery candidates
through `POST /v1/operations/recovery/retry`, and the desktop history panel uses
that service-owned policy instead of collecting candidate IDs itself. Once a
retry is accepted, the original interrupted record remains in operation history
but leaves the recovery summary, preventing repeated retry prompts for the same
restart interruption. The service contract now exposes
`operation_recovery_policy`, making the current stance explicit: automatic resume
is disabled, explicit retries require saved request metadata, retry-all-ready
recovery is available, and request metadata is consumed after a recovery retry is
submitted. It also exposes `operation_control_policy`, which pins the cancel,
retry, recovery retry, and progress-stream IDs plus the operation kinds
currently covered by cancellation/progress controls. That keeps desktop
compatibility checks from inferring operation-control support indirectly from
endpoint names. Operation history now also renders the stored audit/error message for
failed, canceled, and retry-submitted records so those terminal states are
readable in the GUI. The desktop refreshes that snapshot after failed
service-backed lifecycle operations too, so diagnostics reflects terminal
operation state even when the user-facing action fails. The first service-backed
doctor report is now part of that snapshot through authenticated
`GET /v1/diagnostics`, sharing core diagnostic checks between CLI, service, and
desktop. The local service now lists and searches cached audio-AI model package
manifests through authenticated `GET /v1/models`, returning typed IO, param
declarations, runtime entry, and weight-cache status for generated GUI review;
the desktop snapshot renders those cached model packages as a compact searchable
Audio-AI panel, including the service-owned `~/.apm` store layout paths for
manifests, weights, runtimes, cache, logs, and config. The same boundary now
exposes authenticated `POST /v1/models/store/init`, which creates missing
model-store directories through the shared `ModelStore::ensure()` path used by
`apm model store --init`; the desktop setup panel uses that endpoint when the
shared diagnostics report says the store is missing or partially initialized.
The local service also
lists and searches curated model manifests from configured registry sources through authenticated
`GET /v1/models/catalog`; the desktop renders those registry manifests in a
separate browse band and imports a selected registry manifest through
authenticated `POST /v1/models/catalog/{name}/{version}/cache` without mixing
registry browsing into the cached-model store view. It also validates manifest
TOML through authenticated
`POST /v1/models/manifest/validate`, returning a compact GUI-safe summary
without letting the service read arbitrary local manifest paths, and can now
cache a user-selected manifest through authenticated
`POST /v1/models/manifest/cache`. Cached model packages can now submit an
authenticated `POST /v1/models/{name}/{version}/weights/pull` operation that
downloads and verifies declared weights into `~/.apm/weights/<sha256>`, and the
desktop Audio-AI panel exposes a pull/verify action for model weights. Cached
model packages can now also submit an authenticated
`POST /v1/models/{name}/{version}/install` operation that makes a cached
manifest ready in the local model store by verifying existing content-addressed
weights offline or pulling them when missing, then preparing adapter metadata
under `~/.apm/runtimes/<mode>/<name>/<version>` for the selected runtime mode.
The service can then build a non-mutating run plan through
`POST /v1/models/{name}/{version}/run/plan`, binding the cached manifest,
verified weights, prepared adapter metadata, and manifest-validated parameter
bindings without executing model code. Planning parses `adapter.toml` and
rejects stale metadata whose package, runtime mode, entry, adapter ID, hash, or
weight path no longer matches the cached manifest and local store. The returned
plan includes typed execution readiness; current adapters report
`adapter_runner_unavailable` until the real runner exists.
The CLI `apm model run ... --input ... --output ...` uses the same core runner
boundary as the service execution lane and currently returns the structured
blocked `model_run` result for that same blocker.
The service also accepts `POST /v1/models/{name}/{version}/run` as the future
execution operation lane: today it saves the same run request, emits
`model_run_blocked`, and fails with a structured blocked `model_run` result for
`adapter_runner_unavailable` instead of claiming model execution happened. The
same lane now has a `model_run_completed` event and `completed` result status
reserved for the first real adapter runner, and the core now rejects any
completed result whose plan still says execution is blocked.
The desktop operation-history strip renders that structured blocked result, so
diagnostics shows the package and runner blocker from operation data rather
than relying only on a fallback error string.
The Runtime workspace also exposes the service-backed model-store initializer
beside the local layout, so audio-AI setup can be repaired from the same panel
that lists manifests, weights, runtimes, cache, logs, and config paths.
The service now also exposes `POST /v1/models/chains/plan` for non-executing
chain validation. It checks prepared cached-model steps, binds each step's
parameters, validates direct IO edges, and models `stems -> audio` as an
explicit stem-selection handoff for separator-to-transcriber workflows.
The desktop Audio-AI panel can build an ordered chain draft from cached model
cards, request the same service-owned chain plan contract, and render the
returned step/edge/execution readiness as review-only state so the app does not
imply that chain execution exists.
The desktop Audio-AI panel exposes that run-plan hook from cached model cards
and uses native pickers for the input audio file and output folder, then keeps
the returned adapter/runtime/weights/input/output/parameter/execution plan
visible for review.
It can now also submit the service-backed model run operation as an execution
readiness check from cached model cards. The action uses the same native
input/output pickers and model-operation progress lane, then renders the
structured blocked result from the failed operation instead of implying that
audio inference has executed.
Executable runtime adapter launch remains future runtime work. Model pull and
install now emit model operation events into service history and desktop live
progress timelines. The model run operation lane emits started/blocked/failed
events for service history, desktop progress, operation history, and future
runner wiring, and it now honors cancellation before planning and before the
current blocked terminal result. Weight pulls also
emit byte-count progress events into the
`/v1/operations/{operation_id}/events` stream with the same cancellation
control path. Cached
model packages can also be removed through authenticated
`DELETE /v1/models/{name}/{version}`, removing package-specific runtime
metadata and preserving shared content-addressed weights when another cached
manifest still references them; the desktop panel exposes that cached-model
remove action next to weight management.
The service contract also exposes the typed privileged PKG policy:
`external_handoff_only`, explicit confirmation required, and no apm-run PKG
installers. Its helper and rollback gates now carry designed artifact details
for `com.apm.pkg-helper` and
`service/privileged-install-receipts.json`, and core now has a non-mutating typed
receipt-store scaffold for that path while execution stays disabled.
Remaining Phase 21 work centers on configuring signed/notarized release
credentials, retagging the final release commit, running the desktop release
workflow successfully, and implementing future runtime execution.

## v3.0 Non-Goals

- Windows/Linux support
- AAX or VST2
- Full paid marketplace operations
- Complete native audio-AI runtime
- First-party model weight hosting
- Raw artist voice-clone package distribution
- Full DAW plugin clients

## First Build Step

Start with Phase 15. Before writing desktop code, make the repo tell the truth:
README, architecture docs, and planning should all say what `apm` is now, what
exists, what is stale, and where the GUI/runtime boundaries live.
