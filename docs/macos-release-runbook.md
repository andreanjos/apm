# macOS Release Runbook

This runbook defines the first public desktop install, update, uninstall, and
troubleshooting policy for the signed/notarized `apm` app. It applies to the
Tauri desktop bundle with the embedded `Contents/MacOS/apm-cli` sidecar.
Use [macOS Public Release Handoff](macos-public-release-handoff.md) for the
exact workflow, secret, dry-run, and publish checklist.

## Release Artifacts

The public desktop workflow should publish these artifacts after
`npm run bundle:macos:release` and `npm run verify:macos:release` pass. The
bundle command stages the sidecar, runs desktop unit tests, and builds the
frontend before bundling. The workflow then runs
`macos-release-assets.mjs` to create the app zip, extract the zip back into a
temp directory to verify the `apm.app` payload and app-bundle version, verify
the DMG artifact version, write the checksum manifest, write the release
evidence JSON, and verify both manifests against the copied artifacts:

- `apm_<version>_<arch>.dmg`: normal user-facing installer artifact
- `apm-<version>-macos-app.zip`: zipped signed `.app` for inspection and
  diagnostics
- `apm-<version>-desktop.sha256`: checksums for the app zip and DMG
- `apm-<version>-desktop-release-evidence.json`: machine-readable artifact
  evidence with schema version, product version, filenames, byte sizes,
  SHA-256 values, and packaging checks

The DMG is the first user-facing distribution format. PKG support remains
deferred until the app needs privileged installation, LaunchAgent installation,
or a managed uninstall receipt. The current service contract already reserves
that future path as `com.apm.pkg-helper`, installed at
`/Library/PrivilegedHelperTools/com.apm.pkg-helper` with
`/Library/LaunchDaemons/com.apm.pkg-helper.plist`, and receipt metadata under
`<data_dir>/service/privileged-install-receipts.json`; core now defines the v1
JSON receipt-store shape for that path, but public builds must keep PKG
execution disabled until the signed helper writes rollback receipts before
mutation and the full helper path is verified.

## Install

User path:

1. Download the DMG from the GitHub Release.
2. Verify the checksum if installing outside the browser download flow.
3. Open the DMG.
4. Drag `apm.app` to `/Applications`.
5. Launch `apm` from `/Applications`.

The app must be self-contained for launch. It embeds `apm-cli` as a sidecar and
uses that sidecar to start `apm serve run` when no compatible local service is
already running. The app must not require a separate Homebrew CLI install for
normal desktop use.

The standalone Homebrew/tarball CLI remains supported for terminal users. If a
user has both the desktop app and Homebrew CLI installed, the desktop app should
prefer its bundled sidecar unless `APM_DESKTOP_CLI` is set for development.

## Update

Initial update policy is manual replacement:

1. Quit `apm`.
2. Download the new signed/notarized DMG.
3. Drag the new `apm.app` over the existing `/Applications/apm.app`.
4. Launch the app and let it reuse or restart the local service.

The app must keep user package state, registry cache, model store data, and
installed plugins outside the `.app` bundle so app replacement does not remove
or rewrite user content.

Automatic desktop updates are deferred until the signed release channel has at
least one successful public artifact and a clear rollback policy. The future
updater must not update installed audio packages implicitly; app updates and
package updates are separate user actions.

## Uninstall

Basic app uninstall:

1. Quit `apm`.
2. Remove `/Applications/apm.app`.
3. Remove any downloaded desktop DMGs or app zip files.

This removes the desktop app and embedded sidecar only. It intentionally leaves
the standalone Homebrew CLI, installed audio plugins, user package state,
registry cache, backups, and model store data in place.

Optional CLI removal:

```bash
brew uninstall apm
```

Optional full data reset, only when the user explicitly wants to remove local
`apm` state:

```bash
rm -rf ~/.config/apm
rm -rf ~/.local/share/apm
rm -rf ~/.cache/apm
rm -rf ~/.apm
```

Those paths can differ when `XDG_CONFIG_HOME`, `XDG_DATA_HOME`,
`XDG_CACHE_HOME`, `APM_HOME`, or `data_dir` / `cache_dir` config overrides are
set. Prefer reading `/v1/health` from a running local service before
documenting a destructive cleanup for a specific user.

Full data reset must not delete audio plugin folders by default:

- `~/Library/Audio/Plug-Ins/Components`
- `~/Library/Audio/Plug-Ins/VST3`
- `/Library/Audio/Plug-Ins/Components`
- `/Library/Audio/Plug-Ins/VST3`

Package removal belongs to package operations inside `apm`, not app uninstall.
The desktop uninstaller path must never silently delete plugins, model weights,
or third-party vendor-managed installs.

## Troubleshooting

If the app does not launch:

1. Confirm the app came from the signed/notarized DMG.
2. Run `spctl -a -vv -t execute /Applications/apm.app`.
3. Run `codesign --verify --deep --strict --verbose=2 /Applications/apm.app`.
4. If Gatekeeper rejects the app, do not bypass it for a public release; verify
   the release workflow and notarization stapling first.

If the local service panel is unavailable:

1. Quit and reopen the app.
2. Check whether another service is already listening on the reported localhost
   port.
3. Run the bundled or standalone CLI with `apm serve contract` to confirm the
   service contract renders and matches the API/schema shown in the app panel.
4. Inspect the service token path reported by health output; protected routes
   require that loopback token.

If the model store setup card warns:

1. Use the setup panel's model-store initialize action first.
2. If the action fails, run `apm model store --init` with the bundled or
   standalone CLI and inspect the reported `~/.apm` path.
3. Do not delete model weights or manifests unless the user explicitly chooses a
   full data reset.

If package operations fail:

1. Check the final operation timeline in the desktop app.
2. Re-run the equivalent CLI command with `--json` when available.
3. For direct downloads, clear the download cache only after confirming the
   operation is not running.
4. For privileged PKG handoffs, confirm the desktop still reports external
   handoff mode; there should be no installed
   `/Library/PrivilegedHelperTools/com.apm.pkg-helper` until helper execution is
   explicitly implemented. `apm doctor` and the desktop diagnostics report now
   warn if either the helper binary or its launchd plist appears while
   `runs_pkg_installers` is still false.
5. For vendor-managed packages, rerun `apm scan` after completing work in the
   vendor app.

## Release Acceptance

A desktop artifact is not public-release ready until:

- the manual desktop release workflow completes against a tag
- `verify:macos:release` passes inside the workflow
- the dry-run workflow run passes
  `cd apps/apm-desktop && npm run release:macos:workflow-accept -- --repo andreanjos/apm --run-id <id> --tag v<version>`
  after downloading its named artifact set and accepting it locally
- the publish dispatch is run with
  `cd apps/apm-desktop && npm run release:macos:workflow-dispatch -- --repo andreanjos/apm --tag v<version> --publish true --accepted-run-id <id>`
  so the accepted dry-run is rechecked before release attachment
- the DMG opens on a clean macOS machine without Gatekeeper bypass
- `/Applications/apm.app` launches without requiring a separately installed CLI
- removing `/Applications/apm.app` leaves installed plugins and model data
  untouched
- the GitHub Release includes the DMG, app zip, checksum manifest, and release
  evidence JSON
