# apm Desktop

Tauri desktop shell for the macOS-first `apm` product.

## Development

```bash
npm run dev
```

On startup inside Tauri, the app establishes a local `apm serve` session for
the v3 service boundary: it reuses a healthy localhost service or starts
`apm serve run`. The catalog/library snapshot now loads through that localhost
service, and registry sync submits the local-service operation before returning
the typed sync result. Install review, manual/vendor handoff resolution, and
pin/unpin also use the service. Direct URL install, local archive install,
update, and remove execution submit service operations and return their typed
results plus the recorded event timeline. Inside Tauri, registry sync and
lifecycle execution also subscribe to the local service event stream and render
progress as events arrive. The service can now accept operation cancellation
requests: queued operations can be canceled before execution, while running
operations record `cancel_requested`; registry sync and direct ZIP lifecycle
work have the first cooperative checkpoints. The desktop UI surfaces cancel
requests for visible registry, install, update, and remove operations once the
service operation ID is known. The Audio-AI panel now uses the same control path
for visible model weight-pull and model-install operations; model weight pulls
check cancellation before and during downloads, and model installs check again
before runtime metadata is prepared. Model weight pulls and direct ZIP downloads
render byte-count progress events in their operation timelines.

For local testing, set `APM_DESKTOP_CLI` when the `apm` CLI is not on `PATH`:

```bash
APM_DESKTOP_CLI=/absolute/path/to/apm npm run tauri dev
```

In a plain browser preview it uses fixture data from `src/fallback.ts` and does
not launch the service.

## Local macOS Bundles

```bash
npm run bundle:macos:verified
```

This runs a bounded Tauri preview build, builds the frontend, compiles the app,
and writes local preview artifacts under the workspace root `target/`
directory:

- `target/release/bundle/macos/apm.app`
- `target/release/bundle/dmg/apm_<version>_<arch>.dmg`

Open an existing local preview app or DMG without rebuilding:

```bash
npm run open:macos:preview
npm run open:macos:preview:dmg
npm run open:macos:preview -- --dry-run
npm run open:macos:preview:dmg -- --dry-run
```

Both commands first check that the expected preview artifact exists and point
back to `npm run bundle:macos:verified` when it does not. The preview opener
also supports `--dry-run` to verify the selected app or DMG without opening it,
supports `--help` without launching, and rejects unknown flags before opening
an artifact.

The preview build command fails clearly instead of waiting indefinitely if the
initial Tauri app/DMG bundling step exceeds its timeout; pass
`--timeout-ms <ms>` to `npm run bundle:macos --` when a local machine needs a
longer preview build window. The Tauri build runs `npm run sidecar:stage`,
`npm test`, and
`npm run build` before bundling. The sidecar stage builds the release `apm`
CLI, stages it under `src-tauri/sidecars/` with the Rust host target triple,
and Tauri embeds it as `Contents/MacOS/apm-cli` inside `apm.app`. Tests run
after staging so the bundled sidecar contract check validates the CLI that will
actually ship inside the app.
The verified local bundle command then applies an ad-hoc preview signature,
rebuilds any local `apm_*.dmg` around that signed app with a bounded
`bundle_dmg.sh` timeout plus temp/scratch-DMG cleanup, and checks both the
preview app bundle and DMG before treating the preview as usable.

These artifacts are useful for internal testing only. The local build applies
an ad-hoc signature so the preview bundle is self-consistent, but the artifacts
are not Developer ID signed, notarized, or suitable as the public installer
path yet.

## Public Release Gate

For a local v3 checkpoint before a merge-ready handoff:

```bash
npm run verify:v3:local
```

This command accepts only `--help`; unknown arguments fail before any support
checks, tests, builds, or release packaging begin. It runs
`cargo test --workspace`, `release:macos:check`,
`bundle:macos:verified`, dry-run launch smoke checks for the preview app and
DMG, release asset evidence generation, checksum manifest verification,
tracked diff whitespace checks, and untracked file whitespace checks. It proves
the local desktop foundation and preview artifacts; it does not replace the
Developer ID/notarization release workflow. Its self-preflight also requires
the checked preview-launch helper and `open:macos:preview` scripts to remain
present, and dry-run smoke checks keep automated checkpoint verification from
opening the app.

```bash
npm run release:macos:check
npm run verify:macos:preview:dmg
npm run bundle:macos:release
npm run verify:macos:release
npm run accept:macos:release -- --version <version> --artifacts <artifact-dir>
npm run accept:macos:release -- --help
```

`release:macos:check` validates the local Tauri release configuration,
including the unit-test/build/sidecar pre-bundle command, and the checked-in
desktop release support files, build-tool tests, package scripts, and workflow
safety rails without requiring secrets. `release:macos:check -- --help` and
`bundle:macos:release -- --help` print release-gate usage without running
preflight or release build work.
`release:macos:status -- --repo andreanjos/apm --tag v<version>` prints local
preflight, remote workflow visibility, environment-secret inventory, tag
presence/target, current blockers, and next steps without dispatching anything.
Add `--help` to print usage without running local or remote release checks.
`accept:macos:release -- --help` prints release artifact acceptance usage
without opening or validating any artifact directory.
`verify:macos:preview -- --help` and `verify:macos:release -- --help` print
artifact-verifier usage without checking local app, DMG, signature, Gatekeeper,
or notarization state.
`release:macos:workflow-accept -- --help` prints workflow artifact acceptance
usage without querying GitHub or downloading artifacts.
`release:macos:workflow-check -- --help` and
`release:macos:workflow-dispatch -- --help` print workflow readiness/dispatch
usage without checking readiness or dispatching GitHub Actions.
`release:macos:github-bootstrap -- --help` and
`release:macos:github-check -- --help` print GitHub Environment usage without
checking or bootstrapping the remote environment.
`node build-tools/macos-release-assets.mjs --help` prints release asset
packaging usage without writing app zips, DMGs, checksums, or evidence JSON.
Pass `--json` for automation or `--markdown` for paste-ready handoff notes;
those output formats are mutually exclusive. The local and GitHub release
helpers reject unknown flags and malformed boolean values before preflight,
artifact packaging, signing, verification, status, environment, secret,
workflow, or artifact-acceptance side effects.
`bundle:macos:release` writes a generated Tauri config overlay and refuses to
build unless all public-release inputs are present:

- `APM_MACOS_SIGNING_IDENTITY`: Developer ID Application signing identity
- `APM_MACOS_PROVIDER_SHORT_NAME`: Apple notarization provider short name
- `APPLE_API_KEY`, `APPLE_API_ISSUER`, `APPLE_API_KEY_PATH`: notarytool API
  authentication for local release builds; `APPLE_API_KEY_PATH` must point to
  the `AuthKey_*.p8` private-key file

For the GitHub `macos-desktop-release` Environment, store the same private-key
contents as `APPLE_API_KEY_BASE64`; the workflow writes that secret to
`APPLE_API_KEY_PATH` before running the release build.

The generated overlay is ignored by git. This gate prevents preview artifacts
from being mistaken for public desktop releases; it does not replace CI secret
management.

`verify:macos:preview` checks the local preview `.app` structure, Info.plist,
desktop executable, bundled `apm-cli` sidecar, valid local code signature,
sidecar CLI behavior, and the JSON service contract schema/endpoints that the
desktop supervisor expects.
`npm test` also covers that contract compatibility check directly, so stale
schema or endpoint expectations fail before a full app bundle is built.
`verify:macos:preview:dmg` adds local DMG presence, `hdiutil verify`, read-only
mount inspection, and the mounted app's local signature check.
`verify:macos:release` adds release-only checks: DMG presence and integrity,
strict Developer ID code-signing verification, Gatekeeper assessment, and
stapled notarization tickets for the app and DMG. `bundle:macos:release` runs
release verification after the Tauri build.
The release acceptance verifier checks a downloaded artifact directory after
the workflow packages assets: expected app zip, DMG, checksum manifest, release
evidence JSON, app zip payload, checksum drift, evidence drift, DMG integrity,
Developer ID signatures, Gatekeeper acceptance, and notarization stapling. For
workflow artifacts, use
`npm run release:macos:workflow-accept -- --repo andreanjos/apm --run-id <id> --tag v<version>`
so the run identity, workflow name, completion state, artifact download, and
local acceptance checks are verified together.

The app bundle embeds the Rust desktop binary (`apm-desktop`) and a CLI sidecar
(`apm-cli`) used to launch `apm serve run`. Package-manager behavior still
comes from the shared Rust engine through the local service. Local service
launch can use `APM_DESKTOP_CLI`, the bundled `apm-cli`, a neighboring `apm`,
or `apm` on `PATH` for development. The standalone CLI release remains the
Homebrew/tarball artifact until the public desktop release path is signed and
notarized.

The Diagnostics workspace shows the desktop distribution channel. Browser
previews and development builds are informational, preview bundles are marked as
needing the public release gate, and builds created through
`bundle:macos:release` report the public-release channel. The release readiness
card now points operators to `release:macos:status -- --markdown` for the
non-dispatching blocker, next-step, and handoff-note report before any workflow
dispatch. The release verifier still owns the hard Developer ID, Gatekeeper,
notarization, and DMG checks.

## Desktop Release Workflow

`.github/workflows/desktop-release.yml` is the manual CI path for signed desktop
artifacts. Dispatch it with a release tag and leave `publish` false for the
first run; that uploads the signed `.app` zip, DMG, and checksums as workflow
artifacts without attaching them to the GitHub Release. Re-run with `publish`
true after the downloaded artifacts pass
`npm run release:macos:workflow-accept -- --run-id <id> --tag v<version>`.
The workflow/status readiness helpers reject stale release tags by default;
pass `--expected-commit <sha>` only when intentionally dispatching an older
release commit. `--allow-dirty` is accepted only with that explicit
`--expected-commit` so dirty checks stay tied to a committed release target.
Pass `--untracked-files all` to `release:macos:status` when preparing a merge
handoff that needs every untracked file listed instead of collapsed directory
entries.
The local publish dispatch path also requires `--accepted-run-id <id>` and
re-runs same-commit dry-run artifact acceptance before dispatching the publish
workflow.
Direct GitHub UI dispatches must provide the workflow `accepted_run_id` input
when `publish` is true; the workflow validates that dry-run ran from the same
commit before attaching release assets.

The workflow expects the protected `macos-desktop-release` environment to hold
the Developer ID `.p12`, its password, a temporary keychain password, the
signing identity, provider short name, App Store Connect key ID, issuer ID, and
base64-encoded `AuthKey_*.p8` contents. It runs
`npm run bundle:macos:release`, so the same release verifier guards local and
CI release artifacts. `npm run release:macos:check` also verifies that this
workflow remains manually dispatched, checks out an explicit tag, keeps
`publish` defaulted to false, uploads dry-run artifacts, validates
`accepted_run_id` before publish runs with `actions: read`, and only attaches
assets to a GitHub Release behind the `publish` switch. It also requires the
release helper test files that prove those local gates, and verifies that the
local `.env.release.local` secret template path is ignored by Git before
public-release setup.
Use
`npm run release:macos:github-secrets-template -- --output ../../.env.release.local`
to create a local ignored env template for the required secret names, base64
generation commands, dry-run validation, upload, remote inventory check, and
markdown status report sequence without overwriting an existing file; then
`source` the filled file before running
`npm run release:macos:github-secrets`. Add `--help` to either secrets command
to print usage without validating local values, writing a template, or
uploading secrets. When that ignored template already
exists, `release:macos:status` points at filling and sourcing it instead of
asking for regeneration; if the existing file is not ignored by Git or not
mode `600`, it points at fixing that before sourcing credentials.

## Install And Uninstall Policy

The first public desktop policy is documented in
[macOS Release Runbook](../../docs/macos-release-runbook.md). The short version:
install from the signed/notarized DMG by dragging `apm.app` to `/Applications`,
update by replacing that app bundle, and uninstall by deleting
`/Applications/apm.app`. App uninstall removes the desktop shell and embedded
sidecar only; installed plugins, package state, registry cache, backups, and
model store data are preserved unless the user explicitly performs a full data
reset.
