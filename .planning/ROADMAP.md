# Roadmap: apm — Audio Plugin Manager

## Milestones

- **v1.0 MVP** - Phases 1-5 (historical foundation embodied in the current codebase)
- **v2.0 CLI Storefront** - Phases 6-10 (shipped, see `.planning/milestones/v2.0-ROADMAP.md`)
- **v2.1 Registry Time Travel** - Phases 11-13 (shipped, see `.planning/milestones/v2.1-ROADMAP.md`)

## Phases

**Phase Numbering:**
- Integer phases (1, 2, 3): Planned milestone work
- Decimal phases (2.1, 2.2): Urgent insertions (marked with INSERTED)

Decimal phases appear between their surrounding integers in numeric order.

### v1.0 MVP

- [ ] **Phase 1: Foundation** - Compilable binary with correct CLI surface, error types, config paths, and install manifest schema
- [ ] **Phase 2: Scanner** - Offline discovery of installed AU and VST3 plugins by walking macOS plugin directories
- [ ] **Phase 3: Registry and Search** - TOML registry sync from Git, full-text search, category filter, and plugin info
- [ ] **Phase 4: Download and Install Engine** - Download, SHA256 verify, extract (DMG/PKG/ZIP), and install plugins to correct macOS paths
- [ ] **Phase 5: State Management and Lifecycle** - Lockfile, remove, outdated, upgrade, and pin — complete plugin lifecycle

### v2.0 CLI Storefront

- [x] **Phase 6: Workspace Restructure and Server Foundation** - Cargo workspace split, PostgreSQL-backed API server skeleton, and free-install independence invariant
- [x] **Phase 7: Authentication** - User accounts, OAuth Device Flow login, Keychain credential storage, API keys for agents, and JWT token management
- [x] **Phase 8: Purchasing and Webhooks** - Stripe Checkout purchasing, webhook-based fulfillment, idempotent payment processing, refunds, tax compliance, and price display
- [x] **Phase 9: License Management** - Ed25519-signed license keys, local SQLite cache, offline verification, license restore, and unified plugin listing
- [x] **Phase 10: Discovery and Agent Commerce** - Curated storefront browsing, plugin comparison, scoped API keys with spending limits, and autonomous agent purchase flow

### v2.1 Registry Time Travel

- [x] **Phase 11: Versioned Registry Model and Historical Install Surface** - Registry release history, `apm install --version`, and version-aware info output
- [x] **Phase 12: Lifecycle Consistency for Historical Versions** - Latest-vs-installed comparisons in outdated/upgrade/import/export and explicit semantics for older installs
- [x] **Phase 13: Install Reliability Audit and Registry Repair Loop** - Archive-type validation, wrapped-PKG install support, and downgrade/install-path hardening

Archive:
- `.planning/milestones/v2.0-ROADMAP.md`
- `.planning/milestones/v2.0-REQUIREMENTS.md`
- `.planning/v2.0-MILESTONE-AUDIT.md`
- `.planning/milestones/v2.1-ROADMAP.md`
- `.planning/milestones/v2.1-REQUIREMENTS.md`
- `.planning/v2.1-MILESTONE-AUDIT.md`

## Phase Details

### Phase 1: Foundation
**Goal**: The apm binary compiles, parses all subcommands correctly, and has the full infrastructure for error handling, configuration, and macOS paths — so every subsequent phase has a stable base to build on.
**Depends on**: Nothing (first phase)
**Requirements**: CORE-03, CORE-04, CORE-05, CORE-06
**Success Criteria** (what must be TRUE):
  1. Running `apm --help` lists all planned subcommands (install, remove, search, info, scan, list, sync, outdated, upgrade, pin, sources) with correct descriptions
  2. Running any unimplemented subcommand prints a clear "not yet implemented" error with a remediation hint, not a panic
  3. The config directory at `~/.config/apm/` is created automatically on first run, following XDG conventions
  4. Plugin registry TOML files parse correctly and errors include the file path and line number in the message
**Plans**: TBD

Plans:
- [ ] 01-01: TBD

### Phase 2: Scanner
**Goal**: Users can discover all AU and VST3 plugins installed on their Mac — both apm-managed and third-party — without any network access.
**Depends on**: Phase 1
**Requirements**: DISC-01, DISC-02
**Success Criteria** (what must be TRUE):
  1. `apm scan` walks both system (`/Library/Audio/Plug-Ins/`) and user (`~/Library/Audio/Plug-Ins/`) directories and lists all `.component` and `.vst3` bundles found
  2. Each discovered plugin shows name, version, vendor, and bundle ID extracted from its `Info.plist`
  3. `apm list` shows only plugins installed by apm, with name, version, format, and install path
  4. Plugins with unreadable or missing `Info.plist` are reported with a warning rather than causing a crash
**Plans**: TBD

Plans:
- [ ] 02-01: TBD

### Phase 3: Registry and Search
**Goal**: Users can sync a community plugin registry from Git and find plugins by name, vendor, category, or description — so there is a catalog to install from.
**Depends on**: Phase 1
**Requirements**: REG-01, REG-02, REG-03, REG-04, DISC-03
**Success Criteria** (what must be TRUE):
  1. `apm sync` clones or fetches the default registry Git repository and confirms the local cache was updated
  2. `apm search reverb` returns matching plugins with name, vendor, and short description
  3. `apm search --category synth` filters results to only synth plugins
  4. `apm info <plugin>` shows full plugin metadata: vendor, version, description, category, available formats, and homepage URL
  5. `apm sources add <url>` and `apm sources remove <name>` add and remove third-party registries that are consulted during search and install
**Plans**: TBD

Plans:
- [ ] 03-01: TBD

### Phase 4: Download and Install Engine
**Goal**: Users can install a free plugin by name — apm handles downloading, verifying, extracting, and placing it — so the core value of the product is delivered.
**Depends on**: Phase 3
**Requirements**: INST-01, INST-02, INST-03, CORE-01
**Success Criteria** (what must be TRUE):
  1. `apm install <plugin>` downloads the plugin archive, verifies the SHA256 checksum against the registry value, and aborts with a clear error if the checksum does not match
  2. After install, the plugin bundle appears in `~/Library/Audio/Plug-Ins/` in the correct subdirectory and is immediately visible in `apm scan`
  3. Installed plugins do not have the `com.apple.quarantine` extended attribute — DAWs can load them without Gatekeeper blocking
  4. `apm install <plugin> --format vst3` installs only the VST3 bundle; `--format au` installs only the AU bundle; omitting the flag installs all available formats
  5. An interrupted install (Ctrl+C) does not leave a mounted DMG volume or a partial bundle in the plugin directory
**Plans**: TBD

Plans:
- [ ] 04-01: TBD

### Phase 5: State Management and Lifecycle
**Goal**: Users can remove, update, and pin plugins — completing the full package management lifecycle that makes apm a reliable tool rather than a one-way installer.
**Depends on**: Phase 4
**Requirements**: INST-04, UPD-01, UPD-02, UPD-03, CORE-02
**Success Criteria** (what must be TRUE):
  1. `apm remove <plugin>` deletes the plugin bundle and all associated files, and the plugin no longer appears in `apm list` or `apm scan`
  2. `apm outdated` lists every apm-managed plugin that has a newer version available in the registry, showing current and available versions
  3. `apm upgrade <plugin>` replaces the installed version with the latest registry version and updates the lockfile
  4. `apm upgrade` (no argument) upgrades all outdated plugins except those that are pinned
  5. `apm pin <plugin>` prevents a plugin from being upgraded; `apm outdated` marks pinned plugins as pinned rather than listing them as upgradeable
**Plans**: TBD

Plans:
- [ ] 05-01: TBD

### Phase 6: Workspace Restructure and Server Foundation
**Goal**: The codebase is restructured into a Cargo workspace with shared types, the API server starts and accepts requests, and free plugin operations are verified to work with zero server dependency — so every v2 feature has a stable multi-binary foundation.
**Depends on**: Phase 5
**Requirements**: SRV-03, SRV-01, SRV-02, SRV-04, AGENT-01
**Success Criteria** (what must be TRUE):
  1. The Cargo workspace compiles with three crates (`apm-cli`, `apm-server`, `apm-core`) and all existing v1 tests pass without modification
  2. `apm-server` starts and responds to a health-check endpoint, backed by PostgreSQL with migrations applied via SQLx
  3. `apm install <free-plugin>` works identically to v1 with `apm-server` completely offline — no network request to the server is attempted for free plugin operations
  4. All commerce-related CLI commands (`buy`, `login`, `licenses`, `featured`, `explore`) accept `--output json` and produce valid, parseable JSON output (even if the commands themselves are stubs returning structured errors)
**Plans**: 3 plans

Plans:
- [x] 06-01-PLAN.md — Cargo workspace restructure (apm-core, apm-cli, apm-server crates, import migration, test relocation)
- [x] 06-02-PLAN.md — API server foundation (axum health check, PostgreSQL via SQLx, database migrations)
- [x] 06-03-PLAN.md — Commerce command stubs with --json and free-install server-independence invariant

### Phase 7: Authentication
**Goal**: Users can securely create accounts, log in from the CLI via browser-based OAuth, and manage credentials — with API key support so agents can authenticate without interactive login.
**Depends on**: Phase 6
**Requirements**: AUTH-01, AUTH-02, AUTH-03, AUTH-04, AUTH-05, AUTH-06
**Success Criteria** (what must be TRUE):
  1. `apm login` initiates OAuth Device Flow — a browser opens for authorization, and the CLI receives and stores a valid token upon completion
  2. `apm logout` clears all stored credentials from macOS Keychain and the CLI confirms the user is logged out
  3. Auth tokens are stored in macOS Keychain (verified by `security find-generic-password`) and never appear in any config file on disk
  4. `apm auth set-api-key <name> <key>` stores an API key, and a CLI session using `APM_API_KEY=<key>` authenticates successfully without browser interaction
  5. An expired JWT access token is automatically refreshed using the stored refresh token, without prompting the user to log in again
**Plans**: 3 plans

Plans:
- [x] 07-01-PLAN.md — Server auth foundation (PostgreSQL auth schema, device-flow/signup/refresh/API-key endpoints, JWT issuance)
- [x] 07-02-PLAN.md — CLI auth runtime (Keychain credential storage, device-flow client, credential precedence, automatic refresh)
- [x] 07-03-PLAN.md — CLI auth commands and verification (`signup`, `login`, `logout`, `auth` subcommands, API-key UX, regression coverage)

### Phase 8: Purchasing and Webhooks
**Goal**: Users can buy paid plugins with a single command — payment is processed via Stripe Checkout, fulfillment happens exclusively through webhooks, and all financial operations are idempotent and tax-compliant from day one.
**Depends on**: Phase 7
**Requirements**: SRV-05, PUR-01, PUR-02, PUR-03, PUR-04, PUR-05, PUR-06
**Success Criteria** (what must be TRUE):
  1. `apm buy <plugin>` creates a Stripe Checkout session, opens the browser for payment, and after successful payment the plugin is automatically installed with a valid license
  2. Closing the browser mid-checkout or revisiting the redirect URL does not fulfill the order — only the `checkout.session.completed` webhook triggers license issuance and install
  3. Running `apm buy <plugin>` twice in quick succession (simulating a retry) does not create duplicate charges — the same idempotency key is reused within a retry window
  4. `apm search --paid` and `apm info <paid-plugin>` display prices, and `apm search --free` filters to only free plugins
  5. `apm refund <plugin>` within the refund window processes the refund via Stripe and revokes the associated license
**Plans**: 3 plans

Plans:
- [x] 08-01-PLAN.md — Server checkout, webhook, and idempotent purchasing foundation
- [x] 08-02-PLAN.md — CLI `buy` flow with browser checkout and order polling
- [x] 08-03-PLAN.md — Paid catalog pricing, refund flow, and Phase 08 regressions

### Phase 9: License Management
**Goal**: Users own their purchases with cryptographically-signed license keys that work offline, can be restored on new machines, and are visible alongside free plugins in a unified library view — with no DRM, no daemons, and no phone-home behavior.
**Depends on**: Phase 8
**Requirements**: LIC-01, LIC-02, LIC-03, LIC-04, LIC-05, LIC-06
**Success Criteria** (what must be TRUE):
  1. After purchasing a plugin, `apm licenses` shows the license with status, plugin name, and activation info — and the license key can be verified offline using the CLI's embedded Ed25519 public key
  2. `apm restore` on a fresh machine re-downloads and re-installs all previously purchased plugins after authenticating, without requiring re-purchase
  3. Licenses are cached locally in SQLite and `apm licenses` works without network access after an initial sync from the server
  4. License verification happens only during `apm install` — a purchased plugin that is already installed continues to work even if the server is down, the network is offline, or the license is later revoked on the server
  5. `apm list` shows a unified view of both free and purchased plugins, with license status annotations (e.g., "licensed", "expired", "no license") next to purchased plugins
**Plans**: 3 plans

Plans:
- [x] 09-01-PLAN.md — Server signed-license issuance, sync, and restore contracts
- [x] 09-02-PLAN.md — CLI SQLite cache and install-time offline verification
- [x] 09-03-PLAN.md — Restore command and unified `licenses` / `list` UX

### Phase 10: Discovery and Agent Commerce
**Goal**: Users can browse a curated storefront with featured plugins and editorial categories, compare plugins side-by-side, and AI agents can autonomously browse, purchase, and manage plugins within configurable guardrails.
**Depends on**: Phase 9
**Requirements**: DISC-V2-01, DISC-V2-02, DISC-V2-03, DISC-V2-04, AGENT-02, AGENT-03, AGENT-04, AGENT-05
**Success Criteria** (what must be TRUE):
  1. `apm featured` displays curated staff picks, new releases, and trending plugins — and this content updates from the server without requiring a CLI release
  2. `apm explore` presents editorial categories and recommendations that the user can drill into to discover plugins
  3. `apm compare <plugin1> <plugin2>` displays a structured side-by-side comparison of two plugins (price, features, formats, vendor)
  4. An agent with a `purchase`-scoped API key and spending limit can run `apm buy <plugin> --confirm --json` and receive a structured response with transaction ID, license key, install status, and cost — without any browser interaction
  5. An agent with a `read`-scoped API key cannot purchase plugins, and an agent exceeding its spending limit receives a clear denial with the limit details in the JSON response
**Plans**: 4 plans

Plans:
- [x] 10-01-PLAN.md — Server storefront content foundation for featured, explore, and compare
- [x] 10-02-PLAN.md — CLI `featured`, `explore`, and `compare` discovery surfaces
- [x] 10-03-PLAN.md — Server agent purchase guardrails, spending limits, and non-browser purchase contract
- [x] 10-04-PLAN.md — CLI `buy --confirm --json` flow, denial handling, and Phase 10 regressions

### Phase 11: Versioned Registry Model and Historical Install Surface
**Goal**: Users can inspect registry-backed version history and explicitly install a historical release while the default install path still resolves the latest version.
**Depends on**: Phase 10
**Requirements**: VERS-01, VERS-02, VERS-03, HIST-01, HIST-02, HIST-03, LIFE-02, REL-01
**Success Criteria** (what must be TRUE):
  1. Registry schema can represent a plugin's latest release plus historical releases without breaking existing single-version entries
  2. `apm install <plugin> --version <x.y.z>` resolves a historical release from the registry and uses that release's artifact metadata
  3. `apm install <plugin>` without `--version` still resolves the latest release by default
  4. Requesting an unavailable version fails with a clear message that includes available versions
  5. `apm info <plugin>` exposes available versions in both human and JSON output
**Plans**: 1 plan

Plans:
- [x] 11-01-PLAN.md — Schema, CLI flag, info output, and fixture-backed historical install tests

### Phase 12: Lifecycle Consistency for Historical Versions
**Goal**: The rest of the package-manager lifecycle understands that "installed version" and "latest available version" may differ intentionally.
**Depends on**: Phase 11
**Requirements**: LIFE-01, LIFE-03
**Success Criteria** (what must be TRUE):
  1. `apm outdated` reports when the installed release is older than the latest registry release
  2. `apm upgrade` upgrades from an installed historical version to the latest registry release
  3. Import/export and rollback semantics are explicit about local-backup restore versus registry-backed historical install
**Plans**: 1 plan

Plans:
- [x] 12-01-PLAN.md — Version-aware outdated/upgrade and lifecycle semantics

### Phase 13: Install Reliability Audit and Registry Repair Loop
**Goal**: Historical install is trustworthy because the install engine correctly handles real vendor archive shapes and reports failures precisely.
**Depends on**: Phase 12
**Requirements**: REL-02, REL-03
**Success Criteria** (what must be TRUE):
  1. ZIP and DMG archives that wrap PKG installers no longer fail as "no bundle found" or "registry metadata mismatch"; they reach the PKG installer flow instead
  2. PKG installs select the intended installed bundle using requested format and expected bundle path instead of blindly taking the first discovered bundle
  3. Regression coverage exists for the common failure modes observed during live install probing
**Plans**: 1 plan

Plans:
- [x] 13-01-PLAN.md — Install reliability audit, wrapped PKG support, and regression coverage

### Phase 14: Portable Setup (Import/Export)
**Goal**: Users can export their entire apm setup to a compact, shareable `apm1://` string and import it on another machine to recreate the same plugin environment — including installed plugins with versions, pinned status, registry sources, and preferences.
**Depends on**: Phase 13
**Requirements**: D-01 through D-19 (see 14-CONTEXT.md)
**Success Criteria** (what must be TRUE):
  1. `apm export` produces a compact `apm1://` portable string encoding installed plugins (with versions and pin status), configured sources, and user preferences
  2. `apm import apm1://...` accepts the string directly or `apm import file.apmsetup` reads it from a file, then installs plugins, adds sources, and applies preferences
  3. `apm import --dry-run apm1://...` shows a preview of what would change without making modifications
  4. `apm import` shows a confirmation prompt before proceeding; `--yes` skips it for automation
  5. Version conflicts default to keeping the newer installed version; legacy `--format toml` and `--format json` exports continue to work
  6. Round-trip fidelity: export -> import on a clean machine -> export again produces an equivalent string
**Plans**: 2 plans

Plans:
- [x] 14-01-PLAN.md — Portable encoding module (PortableSetup, encode/decode pipeline) and export command update
- [x] 14-02-PLAN.md — Import command overhaul (apm1:// decoding, preview/confirm, version reconciliation) and integration tests

## Progress

**Execution Order:**
Phases execute in numeric order: 1 -> 2 -> 3 -> 4 -> 5 -> 6 -> 7 -> 8 -> 9 -> 10 -> 11 -> 12 -> 13 -> 14

| Phase | Milestone | Plans Complete | Status | Completed |
|-------|-----------|----------------|--------|-----------|
| 1. Foundation | v1.0 | 0/TBD | Not started | - |
| 2. Scanner | v1.0 | 0/TBD | Not started | - |
| 3. Registry and Search | v1.0 | 0/TBD | Not started | - |
| 4. Download and Install Engine | v1.0 | 0/TBD | Not started | - |
| 5. State Management and Lifecycle | v1.0 | 0/TBD | Not started | - |
| 6. Workspace Restructure and Server Foundation | v2.0 | 3/3 | Complete | 2026-04-01 |
| 7. Authentication | v2.0 | 3/3 | Complete | 2026-04-01 |
| 8. Purchasing and Webhooks | v2.0 | 3/3 | Complete | 2026-04-01 |
| 9. License Management | v2.0 | 3/3 | Complete | 2026-04-01 |
| 10. Discovery and Agent Commerce | v2.0 | 4/4 | Complete | 2026-04-01 |
| 11. Versioned Registry Model and Historical Install Surface | v2.1 | 1/1 | Complete | 2026-04-03 |
| 12. Lifecycle Consistency for Historical Versions | v2.1 | 1/1 | Complete | 2026-04-03 |
| 13. Install Reliability Audit and Registry Repair Loop | v2.1 | 1/1 | Complete | 2026-04-03 |
| 14. Portable Setup (Import/Export) | v0.2 | 2/2 | Complete    | 2026-04-07 |
