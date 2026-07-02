# macOS Distribution Plan

## Current Release State

The current public release workflow builds a universal CLI binary and publishes
a tarball used by the Homebrew formula. The desktop app now has a verified local
Tauri preview bundle path with a bundled CLI sidecar and an explicit release
preflight gate. A manual desktop release workflow file exists locally, but it
still needs to reach the remote default branch, the release environment still
needs real signing/notarization secrets, and the first signed/notarized public
installer artifact still needs to be verified. The first install, update,
uninstall, and troubleshooting policy is defined in
[macOS Release Runbook](macos-release-runbook.md). The exact external release
handoff sequence is tracked in
[macOS Public Release Handoff](macos-public-release-handoff.md).

Local preview artifacts:

```bash
cd apps/apm-desktop
npm run bundle:macos:verified
```

The command writes under the workspace root `target/` directory:

- `target/release/bundle/macos/apm.app`
- `target/release/bundle/dmg/apm_<version>_<arch>.dmg`

The current app bundle embeds the Tauri desktop executable (`apm-desktop`) and
a Tauri `externalBin` sidecar named `apm-cli`, copied into
`apm.app/Contents/MacOS/apm-cli`. The sidecar is the same `apm` CLI binary
renamed for the app bundle and is used to launch `apm serve run`; package
manager behavior still lives in the shared Rust engine. The standalone `apm`
CLI remains distributed separately through the existing Homebrew/tarball path.

## Target Release Shape

v3 should produce:

- a macOS desktop app bundle for `apm`
- the `apm-cli` sidecar bundled inside the app for the local service process
- a signed and notarized distribution artifact
- user-facing install, update, uninstall, and troubleshooting instructions

## Signing And Notarization

Public distribution outside the Mac App Store needs Developer ID signing and
notarization. Apple describes Developer ID as the Gatekeeper path for apps
distributed outside the Mac App Store, and Apple's notarization docs state that
Developer ID software distributed for recent macOS versions should be
notarized.

References:

- Developer ID: https://developer.apple.com/developer-id/
- Notarization: https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution

## Tauri Distribution Notes

Tauri can produce macOS app bundles and DMG bundles. Its macOS bundle docs show
the `.app/Contents` layout, and its distribution docs expose app/dmg bundling
commands. The app now uses Tauri `bundle.externalBin` as the first service
process policy.

Current local scripts:

- `npm run bundle:macos`: build preview `.app` and `.dmg` artifacts through a
  bounded Tauri build; pass `-- --timeout-ms <ms>` for unusually slow local
  preview builders
- `npm run bundle:macos:verified`: build preview `.app` and `.dmg` with the
  bounded preview builder, ad-hoc sign the preview app, rebuild the DMG around
  that signed app, then verify both local preview artifacts
- `npm run bundle:macos:app`: build only `.app`
- `npm run bundle:macos:dmg`: build only `.dmg`
- `npm run sign:macos:preview`: ad-hoc sign the local preview app and rebuild
  existing preview DMGs around that signed app, retrying transient generated
  `bundle_dmg.sh` rebuild failures once with a bounded builder timeout while
  cleaning both output temp DMGs and internal `rw.*` scratch DMGs before each
  attempt
- `npm run sidecar:stage`: build the release CLI and stage the host-triple
  sidecar source under `src-tauri/sidecars/`
- `npm run release:macos:check`: validate the public release config shape and
  checked-in workflow safety rails, support files, build-tool tests, and
  package scripts without requiring secrets. Add `--help` to print release-gate
  usage without running preflight or release build work.
- `npm run release:macos:github-bootstrap`: create or update the remote
  `macos-desktop-release` GitHub Environment shell with no branch restriction
- `npm run release:macos:github-secrets`: validate local signing/notarization
  secret inputs and, with `--apply`, bootstrap the environment, upload them to
  the GitHub Environment, and verify the remote secret-name inventory
- `npm run release:macos:github-secrets-template`: print or safely write a
  local, ignored shell template for the required signing/notarization
  environment values without printing secret values. Add `--help` to either
  secrets command to print usage without validating, writing, or uploading
  secret material.
- `npm run release:macos:github-check`: use `gh api` to validate that the
  remote `macos-desktop-release` GitHub Environment exists and exposes every
  required secret name. Add `--help` to either environment command to print
  usage without checking or bootstrapping the GitHub Environment.
- `npm run release:macos:status`: print a non-dispatching release status report
  covering local preflight, local worktree cleanliness, remote workflow
  visibility, environment secrets, release tag presence/target, blockers, and
  next steps; pass `--check` to make blockers exit non-zero, `--json` for
  automation, or `--markdown` to generate paste-ready handoff notes from the
  same live report. JSON and markdown output are mutually exclusive. Add
  `--help` to print usage without running local or remote release checks.
- `npm run release:macos:workflow-check`: validate that GitHub can see the
  manual desktop workflow, the local worktree is clean, the release tag exists
  and points at the expected commit, and the protected environment exposes
  every required secret name before dispatch
- `npm run release:macos:workflow-dispatch`: run the manual desktop workflow
  with `publish=false` by default after the same readiness checks pass, and
  require `--accepted-run-id` before local `publish=true` dispatch. Add
  `--help` to either workflow command to print usage without checking
  readiness or dispatching GitHub Actions.
- `npm run verify:macos:preview`: verify the local preview `.app` bundle
  structure, local code signature, and bundled sidecar behavior
- `npm run verify:macos:preview:dmg`: run preview verification and require an
  `apm_*.dmg` that passes `hdiutil verify`, mounts read-only, and contains the
  expected `apm.app` with the bundled sidecar plus an `Applications ->
  /Applications` install target and valid local signature
- `npm run verify:macos:release`: verify release `.app`/`.dmg` artifacts are
  Developer ID signed, Gatekeeper-accepted, stapled, and structurally complete.
  Add `--help` to either verifier command to print usage without checking app,
  DMG, signature, Gatekeeper, or notarization state.
- `npm run verify:v3:local`: run the local merge-readiness checkpoint: workspace
  checkpoint support preflight, tests, release preflight, verified preview
  bundle, dry-run launch smoke checks for the preview app and DMG, release
  evidence packaging, checksum manifest verification, tracked diff whitespace
  checks, and untracked file whitespace checks
- `npm run accept:macos:release -- --version <version>`: verify a packaged
  `desktop-release/` artifact set after CI packaging or after downloading
  workflow artifacts. Add `--help` to print usage without validating an
  artifact directory.
- `npm run release:macos:workflow-accept`: verify that a dry-run GitHub Desktop
  Release run completed successfully, download the latest matching named
  desktop artifact set for the tag and expected commit, and run local release
  artifact acceptance against the download. Pass `--allow-published-run` only
  when deliberately inspecting a `publish=true` run after publication.
- `npm run bundle:macos:release`: require signing/notarization inputs, generate
  a Tauri config overlay, build `.app` plus `.dmg` through Tauri, and run
  release artifact verification

`tauri.conf.json` runs `npm run sidecar:stage && npm test && npm run build`
before bundling so local app/DMG previews include the CLI sidecar and pass
desktop unit/frontend checks. These scripts intentionally do not perform
Developer ID signing or notarization.

`bundle:macos:release` is the public desktop gate. It validates that the app
bundle still runs desktop unit tests before bundling, includes the `apm-cli`
sidecar, requires a Developer ID Application identity, requires notarization
API credentials, writes the ignored
`src-tauri/tauri.release.generated.conf.json` overlay, and passes that overlay
to `tauri build --bundles app,dmg --ci`. The release script expects:

- `APM_MACOS_SIGNING_IDENTITY`: a `Developer ID Application:` signing identity
- `APM_MACOS_PROVIDER_SHORT_NAME`: Apple provider short name for notarization
- `APPLE_API_KEY`, `APPLE_API_ISSUER`, `APPLE_API_KEY_PATH`: notarytool API
  authentication for local release builds; `APPLE_API_KEY_PATH` must point to
  the `AuthKey_*.p8` private-key file
- `APM_MACOS_TARGET`: optional Rust/Tauri target, such as
  `universal-apple-darwin`

The gate prevents local preview artifacts from being treated as public release
artifacts. The checked-in desktop release workflow runs this gate before any
artifact upload or GitHub Release publish step, and the local preflight also
checks Cargo CLI/desktop crate, Tauri, and desktop package version parity, plus
that the required release build tools, build-tool tests, package-lock, and
package scripts are present. It also checks that the workflow stays manual,
tag-scoped, dry-run capable, checksum producing, protected by the `macos-desktop-release`
environment, and gated by `publish: false` before any GitHub Release
attachment. Before the first signed
run, `npm run release:macos:github-bootstrap -- --repo andreanjos/apm` creates
or updates the remote environment shell through the GitHub API,
`npm run release:macos:github-secrets` validates local secret inputs before
upload, and `npm run release:macos:github-secrets -- --apply` bootstraps the
environment, uploads them without printing secret values, and validates the
remote environment plus required secret names after upload.
After secrets are present and `.github/workflows/desktop-release.yml` is visible
on GitHub, `npm run release:macos:status -- --repo andreanjos/apm --tag v<version>`
is the readable readiness report with next steps, `npm run release:macos:workflow-check
-- --repo andreanjos/apm --tag v<version>` is the hard remote readiness gate, and
`npm run release:macos:workflow-dispatch -- --repo andreanjos/apm --tag v<version>`
starts the signed dry-run workflow with `publish=false`. The status, check, and
dispatch helpers also reject a dirty local worktree by default because the
workflow builds the release tag, not uncommitted local files. Use
`--allow-dirty` only when deliberately inspecting or dispatching an older
already-committed release state, and pair it with `--expected-commit <sha>` so
the checked release target is explicit. For merge/staging handoff review, add
`--untracked-files all` to the status command to expand untracked directories
into file-level `git status --short` entries.

The release asset packager writes four public-desktop asset classes under
`desktop-release/`: the zipped `apm.app`, the signed/notarized DMG copied from
Tauri output, `apm-<version>-desktop.sha256`, and
`apm-<version>-desktop-release-evidence.json`. The evidence JSON records schema
version, product, release version, generated timestamp, artifact role,
filename, byte size, SHA-256, and the packaging checks that were required before
upload. The packager verifies the evidence JSON against the actual files and
requires the app zip payload, DMG payload, and checksum-manifest checks to stay
marked `verified` before the workflow can upload or publish anything.

`macos-release-acceptance.mjs` is the post-packaging acceptance gate for that
same `desktop-release/` directory. It verifies the expected app zip, DMG,
checksum manifest, and evidence JSON inventory while rejecting unexpected files;
requires the checksum and evidence manifests to exactly cover every app zip and
DMG artifact that would be uploaded; rejects duplicate manifest entries;
extracts the app zip to verify the app payload and version; mounts DMGs to
inspect their root app and Applications symlink; and runs Developer ID,
Gatekeeper, and stapled-ticket checks against the extracted app and DMG
artifacts. Local preview artifacts can exercise the inventory, checksum,
evidence, zip, and DMG structure paths, but the full acceptance gate is
intentionally release-only: ad-hoc or unstapled preview artifacts must fail
until the signed workflow produces Developer ID/notarized assets.

Artifact verification has three levels:

- Preview verification checks the app bundle structure, expected Info.plist
  fields, executable bits, and the embedded `apm-cli` sidecar by running
  `apm-cli --version`, `apm-cli serve contract --help`, and
  `apm-cli --json serve contract`. The JSON contract check pins the schema,
  loopback-token auth policy, privileged-installer policy, recovery policy,
  operation-control policy, model run/chain endpoints, and concrete operation
  event names expected by the desktop app, including the `model_run_completed`
  lane reserved for real model runners. It also requires the contract to keep
  declaring pending release, privileged-helper, and runtime-adapter work so the
  desktop Diagnostics v3 integration card cannot drift from the bundled
  sidecar. The same check pins the future privileged helper readiness shape:
  `com.apm.pkg-helper`, `/Library/PrivilegedHelperTools/com.apm.pkg-helper`,
  `/Library/LaunchDaemons/com.apm.pkg-helper.plist`, Developer ID signing,
  authorization, and the receipt store
  `service/privileged-install-receipts.json`. The core service layer now has a
  typed v1 JSON receipt-store scaffold for that path, while still requiring
  `runs_pkg_installers = false` and shipping no helper-run PKG execution.
- Preview DMG verification adds a local `apm_*.dmg` requirement, `hdiutil
  verify`, and a read-only mount inspection that confirms the DMG contains
  `apm.app` with `Contents/MacOS/apm-desktop`,
  `Contents/MacOS/apm-cli`, `Contents/Info.plist`, and an `Applications`
  symlink pointing at `/Applications`; it also checks the mounted app's local
  code signature. `bundle:macos:verified` uses this gate after ad-hoc signing
  the preview app and rebuilding the preview DMG around that signed app.
- Release verification requires the preview checks plus at least one
  mounted `apm_*.dmg`, strict `codesign --verify`, Developer ID authority,
  Team ID, successful `spctl` Gatekeeper assessment, and stapled notarization
  tickets for both the app and the DMG.

## Desktop CI Publishing Workflow

`.github/workflows/desktop-release.yml` is the first public desktop publishing
workflow. It is manual (`workflow_dispatch`) so the existing tag-triggered CLI
release remains unaffected while the desktop installer path is being proven.

The workflow:

- checks out an explicit release tag
- validates that the tag, Cargo package version, Tauri version, and desktop
  package version match
- installs desktop npm dependencies
- imports a Developer ID Application certificate from a protected GitHub
  Environment into a temporary keychain
- writes the App Store Connect API key from that environment to
  `APPLE_API_KEY_PATH`
- runs `npm run bundle:macos:release`, which also runs release artifact
  verification
- runs `macos-release-assets.mjs` to zip the signed `.app`, extract the zip
  back into a temp directory to verify its `apm.app` payload and app-bundle
  version, verify the signed/stapled DMG artifact version, write and verify
  SHA-256 checksums, write and verify the release evidence JSON, and upload the
  resulting set as workflow artifacts
- runs `macos-release-acceptance.mjs` against the packaged `desktop-release/`
  directory before any workflow artifact upload or GitHub Release attachment
- requires and verifies an accepted `publish=false` run ID before any
  `publish=true` attachment, even when dispatched from the GitHub UI; the
  workflow grants `actions: read` so this guard can inspect the accepted
  workflow run and download its artifacts
- attaches those files to the GitHub Release only when the manual `publish`
  input is true

The local release script reads `APPLE_API_KEY_PATH`; the GitHub workflow stores
the private key as `APPLE_API_KEY_BASE64` and writes that temporary file path
before invoking the same release script.

Required GitHub Environment secrets for `macos-desktop-release`:

- `APM_MACOS_CERTIFICATE_BASE64`: base64-encoded `.p12` Developer ID
  Application certificate
- `APM_MACOS_CERTIFICATE_PASSWORD`: password for the `.p12`
- `APM_MACOS_KEYCHAIN_PASSWORD`: temporary CI keychain password
- `APM_MACOS_SIGNING_IDENTITY`: exact `Developer ID Application:` identity
- `APM_MACOS_PROVIDER_SHORT_NAME`: Apple notarization provider short name
- `APPLE_API_KEY`: App Store Connect API key ID
- `APPLE_API_ISSUER`: App Store Connect issuer ID
- `APPLE_API_KEY_BASE64`: base64-encoded `AuthKey_*.p8` contents

Create or update the environment shell with:

```sh
cd apps/apm-desktop
npm run release:macos:github-bootstrap -- --repo andreanjos/apm
```

Add `--help` to `release:macos:github-bootstrap` or
`release:macos:github-check` to print usage without checking or bootstrapping
the GitHub Environment.

Then add the eight environment secrets above in GitHub or with `gh secret set
--env macos-desktop-release <NAME>`. The repo ignores `.env` and `.env.*`, so a
local ignored env file or password-manager export can feed the scripted path.
To generate a safe local template with the required names and base64 commands:

```sh
cd apps/apm-desktop
npm run release:macos:github-secrets-template -- --output ../../.env.release.local
```

The command writes the ignored `.env.release.local` file with private
permissions and refuses to overwrite an existing file. The template comments
include the dry-run, upload, remote inventory check, and markdown status
commands. Add `--help` to either secrets command to print usage without
validating local values, writing a template, or uploading secrets.

Fill that file locally, then source it and run the dry-run/upload sequence:

```sh
cd apps/apm-desktop
source ../../.env.release.local
npm run release:macos:github-secrets -- --repo andreanjos/apm
npm run release:macos:github-secrets -- --repo andreanjos/apm --apply
npm run release:macos:workflow-check -- --repo andreanjos/apm --tag v<version>
npm run release:macos:workflow-dispatch -- --repo andreanjos/apm --tag v<version>
```

The first command is a dry run that checks local environment presence and
credential shape. The `--apply` command passes values to `gh secret set` over
stdin so secret values do not appear in command arguments, then confirms GitHub
exposes all required secret names. The workflow check verifies the remote
workflow surface, required environment secret names, and release tag before a
manual dispatch. By default the tag must point at the current local `HEAD`; pass
`--expected-commit <sha>` only when intentionally releasing another commit. The
workflow dispatch keeps `publish=false` unless explicitly overridden with
`--publish`.

Run it first with `publish: false` and inspect the uploaded app zip, DMG,
checksum manifest, and release evidence JSON. Once the signed app and DMG pass
local download/open tests and workflow artifact acceptance passes, rerun with
`publish: true` to attach the verified desktop assets to the existing GitHub
Release for the tag. The local dispatch helper requires an accepted dry-run run
ID before `publish=true`; it re-runs artifact acceptance with a same-commit
dry-run `publish=false` check before dispatching the publish workflow. Direct
GitHub UI dispatches must set `accepted_run_id`; the workflow rejects
`publish=true` before release attachment if that run fails acceptance, and the
workflow token has `actions: read` for that check. The artifact acceptance
command verifies the explicit completed successful manual `publish=false`
`Desktop Release` run whose run title includes the tag and whose `headSha`
matches the expected commit before downloading:

```sh
cd apps/apm-desktop
npm run release:macos:workflow-accept -- --repo andreanjos/apm --run-id <id> --tag v<version>
npm run release:macos:workflow-dispatch -- --repo andreanjos/apm --tag v<version> --publish true --accepted-run-id <id>
```

Add `--help` to `release:macos:workflow-accept` to print usage without querying
GitHub or downloading artifacts.
Omit `--run-id` only for exploratory local acceptance of the latest matching
dry-run; public release publication should carry the explicit accepted run ID
into `--accepted-run-id`.
Use `--allow-published-run` only for post-publication inspection; it is not part
of the pre-publish acceptance gate.

## Install, Update, Uninstall, And Troubleshooting

The first public policy is intentionally DMG-first and conservative:

- install by dragging `apm.app` from the signed/notarized DMG into
  `/Applications`
- update by replacing `/Applications/apm.app` with the new signed app
- uninstall by deleting `/Applications/apm.app`
- leave installed plugins, package state, registry cache, backups, and model
  store data in place unless the user explicitly asks for a full data reset
- keep package removal separate from app uninstall

See [macOS Release Runbook](macos-release-runbook.md) for the full operator and
user-facing procedure.

References:

- Tauri distribution: https://v2.tauri.app/distribute/
- Tauri macOS app bundle: https://v2.tauri.app/distribute/macos-application-bundle/
- Tauri sidecars: https://v2.tauri.app/develop/sidecar/
- Tauri macOS signing: https://v2.tauri.app/distribute/sign/macos/
- GitHub-hosted macOS runners: https://docs.github.com/actions/reference/runners/github-hosted-runners
- GitHub environments:
  https://docs.github.com/actions/deployment/targeting-different-environments/using-environments-for-deployment
- GitHub deployment environment API: https://docs.github.com/rest/deployments/environments

## Internal Service Launch Preview

The desktop app now has a small Tauri supervisor for the local service. It
checks `/v1/health`, verifies `/v1/service/contract` matches the desktop API
and schema version, verifies that the loopback token file exists, reuses an
already-running compatible service when possible, and otherwise starts:

1. the executable named by `APM_DESKTOP_CLI`
2. the bundled `Contents/MacOS/apm-cli` sidecar, or a neighboring `apm`, while
   skipping the desktop executable itself
3. `apm` from `PATH`

This keeps development override explicit while making the app bundle
self-contained for local previews. The release gate now requires Developer ID
signing/notarization inputs before a public desktop build can run, and release
artifact verification checks the signed/stapled sidecar-bearing app plus DMG.
The manual desktop release workflow now provides the CI publishing lane; the
local preflight validates its release-channel safety rails, and the release
environment still needs configured credentials plus a first successful
signed/notarized run.

## Open Decisions

- Whether the public sidecar should stay host-triple-only per build or become a
  universal sidecar artifact.
- Whether the manual desktop workflow should become tag-triggered after the
  first signed/notarized release succeeds.
- Whether the first public release should add PKG support or remain DMG-only
  until the signed `com.apm.pkg-helper` privileged helper and
  receipt-backed rollback path are implemented.

## Non-Negotiables

- Do not silently run privileged installers.
- Do not ship public artifacts without Developer ID signing and notarization as
  the normal user path.
- Do not build a desktop installer that cannot also uninstall or explain how to
  remove helper artifacts.
- Do not make GUI packaging depend on a separate implementation of package
  manager behavior.
