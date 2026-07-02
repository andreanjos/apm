# Local Service Contract

`apm serve` is the planned localhost boundary for the desktop app, automation,
and future DAW/plugin clients. The current repo includes the versioned contract,
a foreground preview daemon, and an internal desktop supervisor so the CLI,
desktop app, and core engine can agree on the API before signed helper/sidecar
distribution work begins.

```bash
apm serve contract
apm --json serve contract
apm serve run
```

The JSON output is generated from `apm-core::service::local_service_contract`.
Treat that core contract as the source of truth for generated docs, client
fixtures, and desktop integration tests.

## Current Contract

- API version: `v1alpha1`
- Planned bind: `127.0.0.1:4767`
- Port override: `APM_SERVE_PORT`
- Status: foreground preview with persisted operation status/history, request
  metadata for newly accepted operations, explicit retry for failed/canceled
  saved requests, typed operation recovery policy, and typed operation control
  policy for cancel/retry/progress support
- Security default: localhost only, with token auth for non-public routes and a
  typed privileged install policy
- Desktop integration: Tauri can reuse an existing service only after
  `/v1/health`, `/v1/service/contract`, the API version, the contract schema,
  localhost-only policy, full typed contract payload, and the loopback token path
  all validate; otherwise it
  launches `apm serve run` from `APM_DESKTOP_CLI`, the bundled `apm-cli`
  sidecar, a neighboring `apm`, or `apm` on `PATH`

The available endpoint set covers the desktop foundation:

- health
- service contract
- diagnostics report
- catalog search and package details
- installed library and available updates
- library scan and external install reconciliation operation submission, status,
  and event streaming
- registry sync operation submission, status, and event streaming
- install planning
- manual/vendor/PKG/App Store install handoff resolution
- direct URL install operation submission, status, and event streaming
- explicit local archive install operation submission, status, and event
  streaming
- package pin/unpin state updates
- package update operation submission, status, and event streaming
- package remove operation submission, status, and event streaming
- recent operation history listing
- operation cancellation requests
- explicit operation retry from persisted request metadata
- restart-interrupted operation recovery summary
- audio-AI cached model package listing/search
- audio-AI curated model catalog listing/search from configured registries
- audio-AI curated model manifest import into the local model store
- audio-AI model store layout and first-run initialization
- audio-AI model manifest validation from request content
- audio-AI model manifest caching from request content
- audio-AI cached-model weight pull operation submission, status, and event
  streaming
- audio-AI cached-model install operation submission, status, and event
  streaming
- audio-AI cached-model run planning from prepared runtime metadata with
  manifest-validated parameter bindings and typed execution-readiness blockers
- audio-AI cached-model run operation submission, saved request metadata, and
  blocked execution events
- audio-AI cached-model chain planning over prepared steps and typed IO edges
- audio-AI cached-model removal with unreferenced weight cleanup

Accepted operation responses include the generated operation ID, the operation
kind, and the status URL, so desktop clients can route progress by the submitted
operation rather than guessing from event-name prefixes.
Authenticated `POST /v1/library/scan` runs the shared AU/VST3 scanner through the
service operation model. The operation learns matched bundle identifiers when it
can, adopts matched external installs into the install state, emits scan start
and finish events, and returns a typed `ScanPackagesResult`. The desktop
Diagnostics workspace uses this as the in-app follow-up after manual,
vendor-managed, PKG, or App Store handoffs, so users do not have to switch to a
terminal just to reconcile newly installed plugins.
Operation status and event history are persisted to
`<data_dir>/service/operations.json`. The foreground daemon reloads terminal
operation history on restart and marks any interrupted queued/running operation
as failed with a restart message. Newly accepted operations also persist the
typed request payload that produced the operation; older retained records may
not have this field. History updates write a temp file in the same directory and
rename it into place, so a failed write cannot truncate the previous recovery
snapshot. Authenticated clients can retry failed or canceled
operations with saved request metadata through
`POST /v1/operations/{operation_id}/retry`, which creates a new operation from
the original request. Succeeded, still-running, and legacy no-request records
are rejected instead of being silently reinterpreted. This is explicit user
retry, not automatic restart resume.
When a restart-interrupted recovery retry is accepted, the original interrupted
record remains in history with a `Retry submitted as ...` audit message, but
its saved request metadata is consumed so the same restart interruption cannot
be retried again through the row-level retry endpoint.
Persisted history keeps the 250 most recent operation records, and each record
keeps a bounded recent event tail, so progress-heavy streams cannot grow the
service state file without bound. Recent history is available through
authenticated `GET /v1/operations` and is rendered in the desktop preview
diagnostics strip, where failed/canceled request-backed records expose a retry
action and terminal audit/error messages remain visible. The desktop retry
action subscribes to the retried operation's event stream and refreshes history
after terminal status is read, so the diagnostics strip reflects the retry
result instead of just the retry submission.
Authenticated clients can also read `GET /v1/operations/recovery` for a compact
summary of operations that were interrupted by a foreground service restart.
The summary separates total interrupted operations from retryable interrupted
operations, so the desktop can warn when saved request metadata is available
without replaying or reinterpreting history rows in the GUI. The service
contract now publishes `operation_recovery_policy`: automatic resume is disabled,
explicit retries require saved request metadata, retry-all-ready recovery is
available, and request metadata is consumed after a recovery retry is submitted.
The contract also publishes `operation_control_policy`, which pins the endpoint
IDs for cancel, row-level retry, recovery retry, and the operation event stream.
It declares that queued operations can be canceled, running operations move
through `cancel_requested`, and the current operation kinds participate in the
same cancellation/progress surface: registry sync, library scan, direct URL
install, local archive install, package update, package remove, model weight
pull, model install, and model run. Runtime adapter execution remains separate
future work; the current model-run entry covers planning and the structured
blocked result.
Recent operation history keeps matching recovery candidates visible even outside
the newest-three window, marks them as interrupted, and shows whether retry
metadata is ready or manual review is needed. The desktop uses the existing
per-operation retry endpoint for row-level retry and the service-owned recovery
retry endpoint for retry-all-ready recovery actions.
The desktop supervisor now treats contract compatibility as part of readiness:
a reused or launched foreground service must return the expected `v1alpha1`
contract schema and full typed contract payload before the GUI will build an
authenticated client. The runtime panel and diagnostics card carry that
API/schema identity so a stale sidecar or old foreground service is visible
instead of becoming a vague token or request failure.
The desktop snapshot additionally includes distribution-channel readiness from
the Tauri shell. That value is presentation metadata for the GUI diagnostics
surface; the release builder and verifier remain the source of truth for
Developer ID signing, Gatekeeper, notarization, and DMG integrity.
Doctor-style readiness checks are available through authenticated
`GET /v1/diagnostics`. The report is produced in `apm-core` and includes typed
check statuses, details, hints, and summary counts so the CLI, service, and
desktop can share the same diagnostic facts.
Queued operations can be canceled before they start. Running operations can
record a `cancel_requested` state. Initial cooperative checkpoints exist for
registry sync, direct archive install/update paths, direct-download loops, and
package removal before each tracked format deletion. If cancellation arrives
after package removal has already deleted one format, apm repairs install state
to keep only the formats that still remain on disk before returning the
canceled operation. The desktop UI can now request cancellation for visible
registry, install, update, and remove operations once their service operation ID
is known. Direct archive downloads emit byte-count progress events when data is
read from the response and remove canceled `.part` cache files.
Archive execution emits typed checkpoints for archive handling, quarantine
removal, and state recording. If cancellation is observed after a bundle is
placed but before quarantine cleanup or state recording completes, the
just-placed bundle is rolled back before the operation returns.
The desktop can also request cancellation for visible model weight-pull,
model-install, and model-run operations through the same operation-control
endpoint once their service operation ID is known. Model weight pulls now check
cancellation before and during direct downloads, emit byte-count progress
events, remove canceled `.part` files, model installs check cancellation again
before preparing runtime metadata, and model runs check cancellation before
planning and before returning the current structured blocked result.
Install planning now routes direct PKG formats to a privileged external handoff
and Mac App Store formats to an App Store handoff. The service contract exposes
that PKG policy through `security.privileged_install_policy`: current execution
is `external_handoff_only`, handoffs use `privileged_installer`, user
confirmation is required, and apm does not run PKG installers itself. The policy
also includes a typed helper/rollback design: the future execution boundary is
the signed `com.apm.pkg-helper` privileged helper installed at
`/Library/PrivilegedHelperTools/com.apm.pkg-helper` with the matching
`/Library/LaunchDaemons/com.apm.pkg-helper.plist`, and rollback is deferred to
receipt-backed uninstall metadata under
`<data_dir>/service/privileged-install-receipts.json`. The core service layer
owns that file as a typed v1 JSON receipt store, recording the package, source,
installer path/checksum, preflight snapshot paths, pkgutil receipt identifiers,
installed paths, operation ID, and timestamps needed by the future rollback
path. No current service endpoint writes that store or executes PKG installers.
The helper/escalation and rollback gates are now `designed`, while explicit user
consent, package verification, and audit trail capture remain `required`
invariants. Actually executing PKG installers remains pending until the helper
and receipt-backed rollback path are implemented. The desktop service session
forwards the validated policy into the GUI so diagnostics can display the current
external-handoff state and the designed privileged-execution artifacts from the
contract. The shared diagnostics report also checks the planned helper binary
and launchd plist paths directly; while `runs_pkg_installers` is false, any
installed `com.apm.pkg-helper` artifact is reported as a warning instead of
being treated as a usable helper.
The audio-AI model surface now includes authenticated `GET /v1/models`, which
lists or searches cached manifests from `~/.apm/manifests` with typed IO,
parameter declarations, runtime entry, and weight-cache status for generated GUI
review. The optional `query` parameter filters cached model metadata.
Authenticated `GET /v1/models/catalog` lists or searches curated model
manifests from configured registry sources and reports the source name, registry
manifest path, local manifest-cache state, runtime entry, declared params, and
weight-cache state. This is the desktop/automation discovery path before a
model manifest is imported into the local store. Authenticated
`GET /v1/models/store` reports the service-owned `~/.apm` root, manifests,
weights, runtimes, cache, logs, and config paths, and the desktop snapshot
renders those paths in the Audio-AI panel. Authenticated
`POST /v1/models/store/init` creates missing store directories through the
shared `ModelStore::ensure()` implementation used by `apm model store --init`.
The shared diagnostics report includes a `Model store` check that performs the
read-only side of first-run verification, warning when the store is absent or
partially initialized and failing when expected store paths are structurally
invalid. Authenticated
`POST /v1/models/catalog/{name}/{version}/cache` imports one curated registry
manifest into `~/.apm/manifests/<name>/<version>.toml` without accepting
arbitrary local paths from the client. The desktop snapshot renders both
registry catalog packages and cached model packages in its Audio-AI model
package panel.
`POST /v1/models/manifest/validate` accepts manifest TOML in the request body
and returns `ModelManifestValidationResult` summary metadata. The validation
endpoint does not read arbitrary local paths.
`POST /v1/models/manifest/cache` accepts the same content shape, validates it, and
writes the manifest under `~/.apm/manifests/<name>/<version>.toml`. Desktop
clients pass content from a user-selected file through the Tauri shell instead
of asking the service to read paths directly.
Model weight-pull and model-install operations now emit model operation events
into operation history and `/v1/operations/{operation_id}/events`, giving the
desktop Audio-AI panel a live started/finished/failed progress trail.
`POST /v1/models/{name}/{version}/weights/pull` accepts no local path input; it
resolves the already cached manifest through safe model name/version segments
and pulls the declared direct `http(s)` or explicit Hugging Face file weights
into `~/.apm/weights/<sha256>` as an authenticated operation. The response is
the same `OperationAccepted` shape as other long-running work, with retry
metadata preserved in operation history.
`POST /v1/models/{name}/{version}/install` installs from the cached manifest as
an authenticated operation. In the current runtime foundation that means the
manifest is present and its content-addressed weights are verified in the local
store, pulling them only when the verified cached file is missing. The operation
then provisions adapter metadata under
`~/.apm/runtimes/<mode>/<name>/<version>` and returns that prepared runtime
record in `ModelInstallResult`. Executing the adapter remains future runtime
work.
`POST /v1/models/{name}/{version}/run/plan` accepts requested input and output
paths and returns `ModelRunPlan` only when the cached manifest, verified
content-addressed weights, and prepared `adapter.toml` metadata are already
present. The adapter metadata is parsed and checked against the cached package,
runtime mode, entry, adapter ID, weight hash, and local weight path before the
plan is returned. It does not read the input path or execute model code; it
binds the future invocation to the installed runtime facts for desktop review
and reports typed execution readiness. Current adapters return
`adapter_runner_unavailable`, making the missing runner a structured blocker
instead of a display-string convention. The
desktop Audio-AI panel uses native file/folder pickers to choose those
input/output paths before requesting the plan, then renders the returned
adapter/runtime/weights/input/output/execution plan for review.
`POST /v1/models/{name}/{version}/run` accepts the same request as an
authenticated operation. The current executor validates the run plan, preserves
the request in operation history for explicit retry, emits `model_run_started`
and `model_run_blocked` when the adapter runner is unavailable, and marks the
operation failed with a structured blocked `model_run` result rather than
returning a fake execution result. The same contract now includes
`model_run_completed` and the `completed` result status for executable adapters,
but completion is gated on a `ready` execution plan; runner output that tries to
complete a still-blocked plan is rejected before it reaches operation history.
The default runner remains honestly blocked. The operation stream contract
also exposes concrete `event_names`, so the desktop release gate can reject a
stale sidecar that lacks the exact model-run event variants it renders. The
desktop diagnostics history renders the structured blocked result, so the
package ID and runner blocker remain visible without scraping the terminal error
string. The core run path now delegates the prepared plan to a `ModelRunner`
boundary; the default runner preserves this blocked result, and future adapter
execution belongs behind that boundary instead of in the service operation glue.
The desktop Audio-AI panel can submit the same operation as an execution
readiness check. It streams model-run progress, refreshes operation history
after the terminal blocked result, and keeps the returned plan visible for
review rather than presenting the blocked operation as successful inference.
`POST /v1/models/chains/plan` accepts an input path, output path, and ordered
cached-model steps. It validates that every step is cached, has verified weights
and matching prepared runtime metadata, binds each step's declared parameters,
and returns typed IO edges without executing model code. Direct IO matches are
accepted, and `stems -> audio` is accepted as an explicit stem-selection handoff
so a separator can feed a transcriber.
`DELETE /v1/models/{name}/{version}` removes the cached manifest addressed by
safe name/version path segments. It also removes package-specific runtime
metadata and removes the content-addressed weight file only when no other
cached manifest still references the same SHA256, so shared weights are
preserved.

Loopback auth is issued at startup and persisted to
`<data_dir>/service/token.json`. `/v1/health` and `/v1/service/contract` remain
public on localhost for discovery; every other endpoint requires the token via
the `x-apm-token` header or `Authorization: Bearer <token>`.

## Runtime Work Still Required

- Extend the current model-run cancellation/progress checkpoints into executable
  native MLX/Core ML and managed Python runtime sessions.
- Replace the default unavailable model runner with executable native MLX/Core
  ML adapters and managed Python runtime sessions.
- Configure the `macos-desktop-release` signing/notarization environment, run
  the manual desktop release workflow successfully, and complete
  release-channel artifact acceptance against the sidecar-bearing desktop
  bundle runbook.
