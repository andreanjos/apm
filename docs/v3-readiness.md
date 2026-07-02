# apm v3.0 Readiness Matrix

This matrix records current evidence for the v3.0 Desktop Audio Package Manager
requirements. It is intentionally evidence-first: a requirement is only marked
done when current files or verification commands prove the behavior exists.

## Verification Snapshot

Last verified on 2026-07-02:

- GitHub Actions `CI` run `28566100125` passed on merged `main` commit
  `037b3a1eeb091b5a3c1ccff27ba0168f89158859`, including Build, Clippy, and
  Test.
- `cd apps/apm-desktop && npm run verify:v3:local`
- `cd apps/apm-desktop && npm run verify:v3:local -- --help`
- `cargo test --workspace`
- `cd apps/apm-desktop && npm run test:unit`
- `cd apps/apm-desktop && npm run build`
- `cd apps/apm-desktop && npm run release:macos:check`
- `cd apps/apm-desktop && npm run release:macos:check -- --help`
- `cd apps/apm-desktop && node build-tools/macos-preview-bundle.test.mjs`
- `cd apps/apm-desktop && npm run bundle:macos:verified`
- `cd apps/apm-desktop && npm run sign:macos:preview`
- `cd apps/apm-desktop && node build-tools/macos-preview-sign.test.mjs`
- `cd apps/apm-desktop && npm run verify:macos:preview -- --help`
- `cd apps/apm-desktop && npm run verify:macos:release -- --help`
- `cd apps/apm-desktop && npm run verify:macos:preview:dmg`
- `cd apps/apm-desktop && node build-tools/macos-preview-open.test.mjs`
- `cd apps/apm-desktop && npm run open:macos:preview -- --help`
- `cd apps/apm-desktop && npm run open:macos:preview -- --dry-run`
- `cd apps/apm-desktop && npm run open:macos:preview:dmg -- --dry-run`
- Dry-ran the preview opener against the current local `.app` and DMG with
  both the real CLI `--dry-run` path and unit coverage that stubs only the
  final macOS `open` call, proving the selected artifacts pass launch
  verification before opening.
- `node apps/apm-desktop/build-tools/macos-release-assets.mjs --help`
- `node apps/apm-desktop/build-tools/macos-release-assets.mjs --version 0.1.1`
- `cd desktop-release && shasum -a 256 -c apm-0.1.1-desktop.sha256`
- `cd apps/apm-desktop && npm run release:macos:github-bootstrap -- --repo andreanjos/apm`
- `cd apps/apm-desktop && node build-tools/macos-release-github-secrets.test.mjs`
- `cd apps/apm-desktop && npm run release:macos:github-secrets-template -- --output <temp>`,
  then rerunning the same output path, writes the ignored template with private
  permissions and refuses to overwrite an existing file.
- `git check-ignore -v .env.release.local` verifies that the local release
  secret template path is ignored before any real signing/notarization values
  are written there.
- `cd apps/apm-desktop && npm run release:macos:github-secrets -- --repo andreanjos/apm`
  currently fails because no local signing/notarization secret values are set
  in this shell.
- `gh api repos/andreanjos/apm/environments/macos-desktop-release/secrets`
  returned `total_count: 0`.
- `cd apps/apm-desktop && npm run release:macos:github-check` currently fails
  because the `macos-desktop-release` GitHub Environment exists for
  `andreanjos/apm`, but none of the eight required signing/notarization secret
  names are configured there yet.
- `cd apps/apm-desktop && npm run release:macos:workflow-check` currently fails
  because the same eight environment secrets are missing and the existing
  `v0.1.1` tag does not point at the current merged release foundation commit.
- `cd apps/apm-desktop && npm run release:macos:status -- --repo andreanjos/apm --tag v0.1.1`
  currently reports local preflight, local evidence, local worktree, and remote
  workflow visibility passing, with all eight environment secrets and the stale
  `v0.1.1` tag still blocking public release readiness; it also prints
  concrete next steps for those blockers, including the existing ignored
  `../../.env.release.local` template when present, an explicit fix step when
  an existing template is not ignored/private, and a required GitHub secrets
  dry run before upload. It also verifies and summarizes the local release
  evidence manifest, including generated time and artifact hashes, and exposes
  the exact local worktree change inventory in the report.
- `cd apps/apm-desktop && npm run release:macos:status -- --repo andreanjos/apm --tag v0.1.1 --markdown`
  uses the same live report, now including `Local Evidence` and
  `Local Worktree Changes` sections, to generate paste-ready handoff notes.
  The status command rejects conflicting `--json --markdown` output requests,
  supports `--untracked-files all` for file-level merge/staging inventory
  instead of collapsed untracked directories, and rejects invalid untracked
  modes before release checks. When `--allow-dirty` is paired with
  `--expected-commit` for an intentional older-release check, the worktree
  check can pass while the report still includes the exact dirty-tree
  inventory; `--allow-dirty` by itself is rejected before release checks. When a
  release tag points at the wrong commit, the status and workflow helpers now
  tailor the blocker text to the current command: implicit `HEAD` checks can
  still suggest `--expected-commit`, while explicit expected-commit checks name
  the expected tag target and the current tag target.
- `cd apps/apm-desktop && npm run release:macos:status -- --repo andreanjos/apm --tag v0.1.1 --markdown --untracked-files all`
  expands the current local worktree report to the full file-level inventory
  needed for merge review.
- `cd apps/apm-desktop && npm run release:macos:workflow-check -- --repo andreanjos/apm --tag v0.1.1 --allow-dirty`
  now fails before readiness checks unless `--expected-commit <sha>` is also
  provided, matching the status helper's dirty-tree override policy.
- `cd apps/apm-desktop && node build-tools/macos-release-status.test.mjs`
  covers missing, existing ignored/private, and existing unsafe local secret
  template next-step guidance, plus the required
  `--allow-dirty`/`--expected-commit` pairing.
- `cd apps/apm-desktop && node build-tools/v3-local-checkpoint.test.mjs`
  covers the checkpoint step list, support inventory, argument validation, and
  untracked file whitespace checks.
- The v3 checkpoint, GitHub release helpers, and local release helper tests and
  command probes reject unknown arguments and malformed boolean values before
  running checkpoint support checks, release preflight, artifact packaging,
  preview signing, verification, status checks, environment setup, secret
  upload/template work, workflow dispatch, or artifact acceptance. The main
  release gate also accepts `--help` before those preflight/build checks.
- `cd apps/apm-desktop && npm run release:macos:workflow-accept -- --repo andreanjos/apm`
  currently fails because no completed `publish=false` `Desktop Release` dry
  run exists for `v0.1.1` at the merged release foundation commit, so no
  matching same-commit artifact set can be accepted yet.
- `node apps/apm-desktop/build-tools/macos-release-acceptance.mjs --version 0.1.1`
  currently reaches the real local `desktop-release/` inventory/evidence set,
  but fails as expected because the local preview artifacts are ad-hoc signed,
  lack Developer ID Application authority/TeamIdentifier, and are not stapled.
- Latest local release evidence was regenerated at `2026-07-02T04:46:13.184Z`:
  `apm-0.1.1-macos-app.zip` SHA-256
  `35ee27c5d7d4dadb2aa8880dcb76f26a0d83638922810db119c9d42123078849`,
  `apm_0.1.1_aarch64.dmg` SHA-256
  `dd974de8e38ae1db62a6011136b421dca1ed66c45793aa27e0a4c4705020fdce`,
  and `apm-0.1.1-desktop.sha256` SHA-256
  `002851bc4b3acd97c7c24bfa610d05e5a93a7b9bfe2f217e7161c190880aca20`.

## Status Summary

| Status | Count | Meaning |
|--------|-------|---------|
| Done | 31 | Current code/docs/tests provide direct evidence. |
| Partial | 2 | Foundation exists, but at least one required proof remains. |
| Open | 0 | No v3.0 requirement is wholly unstarted. |

## Requirement Evidence

### Product Definition

- `PROD-01` - Done. `README.md`, `docs/desktop-product-plan.md`, and
  `docs/architecture.md` frame `apm` as an Audio Package Manager.
- `PROD-02` - Done. `README.md` and `docs/desktop-product-plan.md` identify
  the desktop app as the producer front door and the CLI/core as the engine.
- `PROD-03` - Done. `docs/architecture.md`, `docs/local-service-contract.md`,
  and `docs/audio-ai-runtime.md` separate plugin flows, model flows, service
  contracts, local state, and desktop responsibilities.

### Engine Boundary

- `ENG-01` - Done. `ApmEngine` owns search, details, library, scan, sync,
  install planning/execution, update, remove, and pin operations.
- `ENG-02` - Done. Service and desktop tests cover structured install, update,
  remove, diagnostics, operation, and model results.
- `ENG-03` - Done. Engine/service events cover registry sync, scan, direct
  downloads, archive install, update/remove, model pulls, installs, and blocked
  model runs.
- `ENG-04` - Done. `cargo test --workspace` covers CLI commands and the shared
  engine behavior used by the service/desktop.
- `ENG-05` - Done. `apm serve contract`, `apm serve run`, and
  `docs/local-service-contract.md` define the local service boundary.

### macOS Desktop App

- `APP-01` - Done. `apps/apm-desktop` builds a Tauri desktop shell and local
  preview app bundle. Current local evidence includes
  `target/release/bundle/macos/apm.app` and
  `target/release/bundle/dmg/apm_0.1.1_aarch64.dmg` from
  `npm run bundle:macos:verified`; the preview bundle command now wraps the
  initial Tauri app/DMG build with a timeout, and the verified preview path
  ad-hoc signs the app, rebuilds the DMG around that signed app with a bounded
  `bundle_dmg.sh` timeout plus temp/scratch-DMG cleanup, and verifies both the
  loose app and mounted DMG app signatures. `npm run open:macos:preview` and
  `npm run open:macos:preview:dmg` provide checked launch commands for the
  current local preview artifacts, verify the selected app or DMG before
  calling macOS `open`, support `--dry-run` for launch-path verification
  without opening the app, handle help/invalid arguments without launching, and
  the v3 checkpoint self-preflight requires those commands plus their
  helper/test files to remain present.
- `APP-02` - Done. The setup checklist renders service, registry, diagnostics,
  and model-store repair actions without Terminal.
- `APP-03` - Done. The Tauri supervisor validates service health, sidecar
  contract, token path, schema, and readiness.
- `APP-04` - Done. Desktop catalog search/filter tests cover package identity,
  vendor, category, status, product type, and access filters.
- `APP-05` - Done. Package inspector renders details, links, versions, aliases,
  formats, checksums, install type, and status.
- `APP-06` - Done. Library workspace renders installed version, update state,
  format, origin, pin, update, remove, and health state.
- `APP-07` - Done. Diagnostics uses service-backed doctor checks,
  scan/reconcile, operation history, recovery, release readiness, and
  helper-artifact checks. Its release readiness card points preview and public
  release builds to the non-dispatching `release:macos:status -- --markdown`
  blocker, next-step, and handoff-note report before workflow dispatch.

### Install And Lifecycle UX

- `LIFE-01` - Done. Direct URL install operations stage downloads, verify
  checksums, install through the shared executor, and stream progress to the GUI.
- `LIFE-02` - Done. The GUI exposes per-format direct install actions plus a
  user/system scope control, and sends the selected scope through Tauri to
  shared engine plan and install requests.
- `LIFE-03` - Done. Manual, vendor, PKG, and App Store handoffs require
  explicit in-app confirmation, and PKG execution stays external.
- `LIFE-04` - Done. Vendor/manual handoffs are opened from the GUI, and
  Diagnostics can run service-backed scan/reconcile afterward.
- `LIFE-05` - Done. The GUI removes apm-managed packages through the
  service-backed remove operation with confirmation and progress.
- `LIFE-06` - Done. The library shows available updates and supports
  one-package plus all-direct-ready update flows.
- `LIFE-07` - Done. Pin/unpin is service-backed and affects update eligibility.
- `LIFE-08` - Done. Operation history, bounded event tails, restart recovery,
  row retry, retry-all-ready recovery, and terminal audit/error messages are
  visible in Diagnostics.

### macOS Distribution

- `DIST-01` - Partial. Local preview DMG/app artifacts, a local manual signed
  workflow file, checksum packaging, a machine-readable release evidence
  manifest, workflow-dispatch readiness checks, and a GitHub workflow artifact
  acceptance helper exist. The workflow now names runs with the release tag and
  publish mode, so `workflow-accept` requires dry-run evidence by default and
  can discover the latest matching successful `publish=false` dry run for the
  same expected commit after dispatch; the workflow also validates
  `accepted_run_id` before any
  `publish=true` release attachment and grants `actions: read` for that guard.
  `npm run release:macos:check` also validates the local release support files,
  build-tool tests, package scripts, and ignored local secret-template path the
  workflow depends on before the workflow is merged.
  Release evidence verification now rejects stale or missing evidence check
  statuses for the app zip payload, DMG payload, and checksum manifest, and
  release artifact acceptance rejects unexpected files plus checksum/evidence
  manifests that do not exactly cover every uploadable app zip and DMG. Local
  release helpers also reject malformed arguments before package, sign, verify,
  or acceptance side effects. Current
  local evidence under
  `desktop-release/` includes
  `apm-0.1.1-macos-app.zip`, `apm_0.1.1_aarch64.dmg`,
  `apm-0.1.1-desktop.sha256`, and
  `apm-0.1.1-desktop-release-evidence.json`, with exact checksum/evidence
  coverage for the app zip and DMG. `npm run verify:v3:local` now runs this
  local checkpoint as one command before a merge-ready handoff, rejects unknown
  arguments before heavyweight work, runs dry-run launch smoke checks against
  the current preview app and DMG, checks tracked diffs plus untracked files for
  whitespace errors, and first fails fast if its own helper files or required
  desktop package scripts are missing, including the checkpoint
  regression test, the bounded preview bundle wrapper/test, and the preview
  signer regression test that covers retry and timeout/scratch-DMG cleanup; the
  release preflight inside that checkpoint also fails if required release helper
  tests disappear.
  Full release acceptance still needs a real Developer ID signed, notarized,
  stapled workflow run that passes
  `cd apps/apm-desktop && npm run release:macos:workflow-accept`.
- `DIST-02` - Done. Tauri bundles `apm.app` with the `apm-cli` sidecar, and
  preview/release verifiers check the layout. Preview verification now also
  rejects invalid local signatures for both the loose app and the app mounted
  from the DMG, and the preview signer rebuilds the DMG around the signed app
  with a bounded generated `bundle_dmg.sh` timeout, one retry for transient
  builder failures, cleanup for both output temp DMGs and internal `rw.*`
  scratch DMGs, and stale-DMG preservation if all rebuild attempts fail.
- `DIST-03` - Partial. Release gates require Developer ID and notarization
  inputs. The remote `macos-desktop-release` environment now exists and the
  GitHub checker reaches it, and a local secret-template helper now prints or
  safely writes the required env names, base64 generation commands, and
  dry-run/upload/check/status sequence without secret values. The file-writing
  path uses private permissions and refuses to overwrite an existing local env
  file. All required environment secrets are still missing, the manual desktop
  workflow is visible on GitHub, and the first signed/notarized workflow run is
  not yet proven. The release status/workflow checks also
  reject a dirty local worktree and stale release tag before dispatch, because
  the workflow checks out the tag it is asked to build and cannot include
  uncommitted local changes. `release:macos:status` now exposes those blockers
  and their next steps without dispatching, routes secret setup through a dry
  run before upload, warns before sourcing existing unsafe local templates, can
  render the same live report as markdown handoff notes, rejects malformed
  GitHub release-helper arguments before side effects, verifies and summarizes
  the local release evidence manifest for handoff notes, includes exact local
  worktree changes in markdown and JSON output, and keeps
  `docs/macos-public-release-handoff.md` aligned with the exact external
  handoff sequence and acceptance criteria.
- `DIST-04` - Done. First launch/setup checks service, registry, diagnostics,
  model-store layout, and helper artifacts without destructive changes.
- `DIST-05` - Done. `docs/macos-release-runbook.md` documents install, update,
  uninstall, troubleshooting, and data preservation.

### Audio-AI Package Foundation

- `AI-01` - Done. Model manifests cover package metadata, runtime mode,
  weights, typed IO, params, license, and hardware.
- `AI-02` - Done. `apm model lock` writes exact model versions, runtime modes,
  weight hashes, and sources.
- `AI-03` - Done. `apm model store --init`, service store init, diagnostics,
  and desktop runtime panels expose the `~/.apm` layout.
- `AI-04` - Done. `docs/audio-ai-runtime.md` explains native-first modes, Core
  ML, and managed Python fallback without claiming full runtime coverage.
- `AI-05` - Done. `docs/audio-ai-runtime.md` now names early utility-model
  categories, deferred generation, excluded artist-clone weights, and follow-on
  DAW clients.

## Remaining v3.0 Work

1. Fill and source the existing ignored `../../.env.release.local` template,
   pass `npm run release:macos:github-secrets`, apply them with `--apply` so
   the remote secret inventory is verified, keep the local release worktree
   clean before dispatch, retag the final merged release commit or pass
   `--expected-commit <sha>` plus `--allow-dirty` for an intentional old
   committed release, pass `npm run release:macos:workflow-check`, run the
   manual signed/notarized desktop workflow with `publish=false`, and pass
   `npm run release:macos:workflow-accept` against the completed same-commit
   matching run before dispatching `publish=true` with `--accepted-run-id` or
   the workflow `accepted_run_id` input.
2. Complete release-channel artifact acceptance against the sidecar-bearing
   DMG, app zip, checksum manifest, release evidence JSON, Gatekeeper,
   notarization stapling, and the runbook.
