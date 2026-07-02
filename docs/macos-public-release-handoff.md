# macOS Public Release Handoff

This is the handoff checklist for turning the local v3 desktop foundation into
the first public signed/notarized `apm` macOS GUI installer. It records what is
already proven locally and what still requires external Apple/GitHub release
state.

Last local checkpoint: 2026-07-02. Latest evidence JSON generated at
`2026-07-02T04:46:13.184Z`.

Recent post-merge CI evidence includes GitHub Actions runs `28566100125` and
`28566548713`; both passed Build, Clippy, and Test on merged `main`
checkpoints.

## Proven Locally

Run the local checkpoint from the desktop package:

```bash
cd apps/apm-desktop
npm run verify:v3:local
```

That command proves:

- the checkpoint helper files, checkpoint regression test, and required
  desktop npm scripts are present
- the checked preview-open helper and npm scripts are present, and the opener
  verifies the selected app or DMG before invoking macOS `open`
- the full Rust workspace test suite passes
- desktop release preflight passes without requiring secrets
- the local preview `.app` and DMG build successfully through a bounded Tauri
  preview build
- the local preview app is ad-hoc signed
- the preview DMG is rebuilt around the signed preview app, with a bounded
  generated `bundle_dmg.sh` timeout, one retry for transient failures, and
  cleanup for both output temp DMGs and internal `rw.*` scratch DMGs before
  each attempt
- the loose app and mounted DMG app signatures verify locally
- the checked preview app and DMG launch paths pass `--dry-run` smoke checks
  without opening the app during automated verification
- release asset packaging writes app zip, DMG, checksum manifest, and evidence
  JSON
- the release asset packager can print `--help` without packaging artifacts
- the preview/release artifact verifier can print `--help` without checking
  local app, DMG, signature, Gatekeeper, or notarization state
- the checksum manifest verifies those local packaged artifacts
- tracked diff whitespace checks pass
- untracked file whitespace checks pass

The latest local evidence hashes are:

- `apm-0.1.1-macos-app.zip`:
  `35ee27c5d7d4dadb2aa8880dcb76f26a0d83638922810db119c9d42123078849`
- `apm_0.1.1_aarch64.dmg`:
  `dd974de8e38ae1db62a6011136b421dca1ed66c45793aa27e0a4c4705020fdce`
- `apm-0.1.1-desktop.sha256`:
  `002851bc4b3acd97c7c24bfa610d05e5a93a7b9bfe2f217e7161c190880aca20`

Current local preview artifacts:

- `target/release/bundle/macos/apm.app`
- `target/release/bundle/dmg/apm_0.1.1_aarch64.dmg`
- `desktop-release/apm-0.1.1-macos-app.zip`
- `desktop-release/apm_0.1.1_aarch64.dmg`
- `desktop-release/apm-0.1.1-desktop.sha256`
- `desktop-release/apm-0.1.1-desktop-release-evidence.json`

Run the current preview app or mount the current preview DMG from the desktop
package:

```bash
cd apps/apm-desktop
npm run open:macos:preview
npm run open:macos:preview:dmg
npm run open:macos:preview -- --dry-run
npm run open:macos:preview:dmg -- --dry-run
```

These artifacts are preview evidence only. They are ad-hoc signed and must not
be published as the public installer. The open commands verify the selected
preview artifact before handing it to macOS, and `--dry-run` performs the same
selection and verification without opening anything.

## Current External Blockers

As of the latest post-merge check:

- GitHub Actions for `andreanjos/apm` sees the merged
  `.github/workflows/desktop-release.yml` on the remote default branch.
- The local release worktree check passes with the current ignored preview
  artifacts left out of Git.
- The `macos-desktop-release` GitHub Environment exists, but all eight required
  signing/notarization secret names are still missing.
- The existing `v0.1.1` tag points at `e11943192307`, not the release commit
  currently expected by `release:macos:status`; use the dry-run
  `release:macos:tag` helper to review the lease-protected tag move after the
  final release commit lands, then rerun it with `--apply` only after the plan
  matches the intended release commit. When an explicit expected commit is
  supplied, the readiness report names both the expected tag target and the
  current tag target.
- No completed `publish=false` `Desktop Release` dry run exists yet for
  `v0.1.1` at the expected release commit, so workflow artifact acceptance
  fails until the first signed dry run completes.
- Local release acceptance against preview artifacts fails as expected because
  preview artifacts are not Developer ID signed, not Gatekeeper accepted, and
  not stapled.

Do not dispatch `publish=true` until a signed `publish=false` dry run has been
accepted with the workflow artifact acceptance command.

Inspect the current public-release state without dispatching anything:

```bash
cd apps/apm-desktop
npm run release:macos:status -- --repo andreanjos/apm --tag v0.1.1
```

The report prints local release evidence, local worktree inventory, blockers,
and the next commands/actions to clear them.
Add `--check` when the command should exit non-zero while blockers remain, or
`--markdown` when the live report should be pasted into handoff notes without
manual reformatting. The markdown report includes a `Local Evidence` section
with the generated time and artifact hashes from
`desktop-release/apm-0.1.1-desktop-release-evidence.json`, and the JSON report
includes the same data under `localEvidence`. The markdown report also includes
`Local Worktree Changes`, and the JSON report includes the same
`git status --short` lines under `localWorktree.changes`. By default, untracked
directories use Git's normal collapsed form; add `--untracked-files all` for a
file-level merge/staging inventory, or `--untracked-files no` for a
tracked-change-only check. When `--allow-dirty` is used with
`--expected-commit` for an intentional older-release check, the local worktree
check can pass but the same dirty-tree inventory remains visible in the report;
`--allow-dirty` without `--expected-commit` is rejected before release checks.
When the ignored local secret template already exists, the secret next step
points at filling that file and passing it with `--env-file` instead of
regenerating it.
If an existing template is not ignored by Git or is not mode `600`, the status
report points at fixing that before any credentials are read. Use `--json` for
automation instead; JSON and markdown output are mutually exclusive. Add
`--help` to print usage without running local or remote release checks.

## Required Secrets

Configure these as GitHub Environment secrets on `macos-desktop-release`:

- `APM_MACOS_CERTIFICATE_BASE64`
- `APM_MACOS_CERTIFICATE_PASSWORD`
- `APM_MACOS_KEYCHAIN_PASSWORD`
- `APM_MACOS_SIGNING_IDENTITY`
- `APM_MACOS_PROVIDER_SHORT_NAME`
- `APPLE_API_KEY`
- `APPLE_API_ISSUER`
- `APPLE_API_KEY_BASE64`

Generate a local template if it is missing:

```bash
cd apps/apm-desktop
npm run release:macos:github-secrets-template -- --output ../../.env.release.local
```

The generated template is written to the ignored `../../.env.release.local`
path with private file permissions, and the command refuses to overwrite an
existing file. Its comments include the dry-run, upload, remote inventory check,
markdown status, and dry-run tag commands so secret setup can be driven from
the filled local file without retyping the sequence. `release:macos:check`
verifies that this local secret template path remains ignored by Git before
public-release setup, and `release:macos:status` warns when an existing template
is not ignored or not private. Add `--help` to `release:macos:check` or
`bundle:macos:release` to print release-gate usage without running preflight or
release build work. Add `--help` to either secrets command to print usage
without validating local values, writing a template, or uploading secrets.

After filling the local template, validate without upload:

```bash
npm run release:macos:github-secrets -- \
  --repo andreanjos/apm \
  --env-file ../../.env.release.local
```

Upload and verify the remote inventory:

```bash
npm run release:macos:github-secrets -- \
  --repo andreanjos/apm \
  --env-file ../../.env.release.local \
  --apply
```

## Release Sequence

1. Run and keep evidence for the local checkpoint:

   ```bash
   cd apps/apm-desktop
   npm run verify:v3:local
   ```

2. Confirm `.github/workflows/desktop-release.yml` is visible on the remote
   default branch. This is currently proven by the post-merge release status
   check.

3. Configure and verify the `macos-desktop-release` secrets:

   ```bash
   # Run this first command only when ../../.env.release.local is missing.
   npm run release:macos:github-secrets-template -- --output ../../.env.release.local
   npm run release:macos:github-secrets -- \
     --repo andreanjos/apm \
     --env-file ../../.env.release.local
   npm run release:macos:github-secrets -- \
     --repo andreanjos/apm \
     --env-file ../../.env.release.local \
     --apply
   npm run release:macos:github-check -- --repo andreanjos/apm
   npm run release:macos:tag -- --tag v0.1.1 --expected-commit "$(git rev-parse HEAD)"
   # Rerun the same tag command with --apply only after the dry-run plan is right.
   ```

   Add `--help` to `release:macos:github-bootstrap` or
   `release:macos:github-check` to print usage without checking or
   bootstrapping the GitHub Environment.

4. Verify remote workflow readiness:

   ```bash
   npm run release:macos:status -- --repo andreanjos/apm --tag v0.1.1
   npm run release:macos:workflow-check -- --repo andreanjos/apm --tag v0.1.1
   ```

   These commands also verify that the local worktree is clean and that the tag
   points at the expected commit. By default that commit is the current local
   `HEAD`. Review the lease-protected tag update before applying it:

   ```bash
   npm run release:macos:tag -- --tag v0.1.1 --expected-commit "$(git rev-parse HEAD)"
   ```

   Rerun the same tag command with `--apply` after confirming the plan. Add
   `--expected-commit <sha>` only when deliberately releasing a different
   commit, and pair `--allow-dirty` with `--expected-commit` only when
   deliberately checking an older committed release state.

5. Dispatch a signed dry run:

   ```bash
   npm run release:macos:workflow-dispatch -- --repo andreanjos/apm --tag v0.1.1
   ```

   Add `--help` to `release:macos:workflow-check` or
   `release:macos:workflow-dispatch` to print usage without checking readiness
   or dispatching GitHub Actions.

6. Accept the dry-run artifacts after the workflow succeeds:

   ```bash
   npm run release:macos:workflow-accept -- --repo andreanjos/apm --run-id <id> --tag v0.1.1
   ```

   The accepted run must be a completed `publish=false` Desktop Release run for
   the same tag and expected commit. By default the expected commit is local
   `HEAD`; add `--expected-commit <sha>` only when deliberately accepting a
   different release commit.

7. Publish only with that accepted dry-run ID:

   ```bash
   npm run release:macos:workflow-dispatch -- \
     --repo andreanjos/apm \
     --tag v0.1.1 \
     --publish true \
     --accepted-run-id <id>
   ```

If publishing from the GitHub UI instead, set `publish=true` and set
`accepted_run_id` to the same accepted dry-run run ID. The workflow re-runs the
artifact acceptance guard before attaching release assets.

## Acceptance Criteria

The desktop release is public-ready only when all of these are true:

- `Desktop Release` is visible and active in GitHub Actions.
- `macos-desktop-release` exposes all eight required secret names.
- The local worktree is clean, or the release is intentionally checked against
  an older committed state with `--expected-commit` and `--allow-dirty`.
- `release:macos:workflow-check` passes for the release tag and expected commit.
- The `publish=false` workflow run completes successfully.
- `release:macos:workflow-accept` passes for that dry-run run ID and expected
  commit.
- The `publish=true` dispatch uses the accepted dry-run ID.
- The GitHub Release contains the DMG, app zip, checksum manifest, and release
  evidence JSON.
- A clean macOS machine can open the DMG, drag `apm.app` to `/Applications`,
  and launch it without Gatekeeper bypass or a separately installed CLI.
