# Phase 14: Portable Setup (Import/Export) - Context

**Gathered:** 2026-04-06
**Status:** Ready for planning

<domain>
## Phase Boundary

Enhance apm's export/import to produce a compact, shareable string (`apm1://...`) that encodes the user's full setup — plugins with versions, pin status, registry sources, and preferences. The string is pasteable into chat, a README, or a terminal. File output is also supported for large setups.

This replaces the existing bare-bones export/import (which only captures plugin names/versions) with a complete portable environment.

</domain>

<decisions>
## Implementation Decisions

### Format
- **D-01:** Export produces a URI-style string: `apm1://` prefix + base64-encoded compressed payload
- **D-02:** Payload encodes: plugins (slug + version), pin status, configured sources (name + URL), and user preferences (install scope, registry URL)
- **D-03:** The `apm1://` prefix is a version marker so the format can evolve without breaking old strings
- **D-04:** Compression (zlib/deflate) keeps strings short — a 15-plugin setup should be ~200-300 characters

### Output Modes
- **D-05:** `apm export` prints the string to stdout by default (pipe-friendly, copy-paste ready)
- **D-06:** `apm export -o setup.apmsetup` writes to a file instead (for large setups or version control)
- **D-07:** Both modes produce the same content — the file is just the string saved to disk

### Import Behavior
- **D-08:** `apm import apm1://...` accepts the string directly as an argument
- **D-09:** `apm import setup.apmsetup` accepts a file path (auto-detected)
- **D-10:** Import always shows a preview first and asks for confirmation ("Would install 12 plugins, add 1 source, pin 3. Proceed? [Y/n]")
- **D-11:** `apm import --yes apm1://...` skips confirmation for scripting/automation
- **D-12:** `apm import --dry-run apm1://...` shows the preview without the confirmation prompt (exit after preview)

### Diff/Reconcile
- **D-13:** Preview shows what would change: new installs, version differences, new pins, new sources
- **D-14:** Already-installed plugins at the correct version are skipped silently
- **D-15:** Version conflicts (file says v1.0, machine has v2.0) default to keeping the newer version — preview highlights these as "skip (newer installed)"

### Scope
- **D-16:** Full setup encoded: plugins + versions + pins + sources + install_scope + registry_url
- **D-17:** Machine-specific paths (data_dir, cache_dir overrides) are NOT included — those are local

### Backwards Compatibility
- **D-18:** Existing `apm export --format json` and `apm import file.toml` continue to work for the old plugin-only format
- **D-19:** The new portable string is the default output of `apm export` (replaces the old TOML default)

### Claude's Discretion
- Compression algorithm choice (zlib, brotli, etc.) — whatever produces shortest strings
- Internal serialization format before compression (compact TOML, msgpack, custom binary)
- How to handle the `apm1://` prefix parsing and validation

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Existing Implementation
- `crates/apm-cli/src/commands/export_cmd.rs` — Current export command (plugin-only, TOML/JSON)
- `crates/apm-cli/src/commands/import_cmd.rs` — Current import command (plugin-only, batch install)
- `crates/apm-core/src/config.rs` — Config struct with sources, install_scope, registry_url
- `crates/apm-core/src/state.rs` — InstallState with plugins and pin status

### CLI Surface
- `crates/apm-cli/src/main.rs` — Command definitions for Export and Import (lines 231-252)

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `ExportDocument` / `ExportedPlugin` structs in export_cmd.rs — extend or replace with richer payload
- `load_export_file()` in import_cmd.rs — auto-detects TOML/JSON, extend to detect `apm1://` strings
- `process_one()` in import_cmd.rs — per-plugin install logic with skip/fail handling
- `Config` struct already has all the fields we need to serialize (sources, install_scope, registry_url)

### Established Patterns
- TOML is the standard serialization format throughout (config, state, registry)
- colored/Colorize for terminal output formatting
- `--dry-run` flag pattern used in install, import, upgrade, cleanup

### Integration Points
- `main.rs` Export/Import command variants — add new flags, change default behavior
- `Config::sources()` — read configured sources for export
- `InstallState::load()` — read installed plugins and pin status

</code_context>

<specifics>
## Specific Ideas

- The string should be short enough to paste in a Slack message or iMessage
- `apm1://` prefix signals the format version — future `apm2://` can change the encoding
- Think of it like a game save code — compact, shareable, self-contained

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

*Phase: 14-portable-setup*
*Context gathered: 2026-04-06*
