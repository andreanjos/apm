---
phase: 14-portable-setup
plan: 01
subsystem: cli
tags: [base64, deflate, flate2, portable, encoding, export]

# Dependency graph
requires: []
provides:
  - "portable.rs module with PortableSetup data model and encode/decode pipeline"
  - "apm1:// URL scheme for shareable setup strings"
  - "ImportPreview and build_preview for import categorization"
  - "Updated export command defaulting to portable format"
affects: [14-02-PLAN]

# Tech tracking
tech-stack:
  added: [flate2, base64]
  patterns: [JSON-DEFLATE-base64url encoding pipeline, serde skip_serializing_if for compact payloads]

key-files:
  created:
    - crates/apm-cli/src/portable.rs
  modified:
    - crates/apm-cli/Cargo.toml
    - crates/apm-cli/src/main.rs
    - crates/apm-cli/src/commands/export_cmd.rs

key-decisions:
  - "URL_SAFE_NO_PAD base64 encoding for URL-safe apm1:// strings without padding characters"
  - "Compression::best() for smallest payloads; 15 plugins encode under 500 chars"
  - "Portable format writes file confirmations to stderr, keeping stdout clean for piping"

patterns-established:
  - "Compact serde field names (v, p, n, s, c) to minimize payload size"
  - "skip_serializing_if for default values (pinned=false, default scope, default registry)"

requirements-completed: [D-01, D-02, D-03, D-04, D-05, D-06, D-07, D-16, D-17, D-18, D-19]

# Metrics
duration: 4min
completed: 2026-04-07
---

# Phase 14 Plan 01: Portable Setup Encoding Summary

**apm1:// encoding pipeline (JSON -> DEFLATE -> base64url) with PortableSetup data model, import preview, and export command defaulting to portable format**

## Performance

- **Duration:** 4 min
- **Started:** 2026-04-07T06:41:15Z
- **Completed:** 2026-04-07T06:45:23Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments
- Created portable.rs module with PortableSetup, PortablePlugin, PortableConfig, and ImportPreview data model
- Implemented encode/decode pipeline: JSON -> DEFLATE -> base64url -> apm1:// prefix (and reverse)
- Built from_state_and_config to convert install state and config into portable representation
- Built build_preview to categorize what importing a setup would do (install, skip, pin, sources, config)
- Updated export command to default to portable format while preserving legacy TOML/JSON paths
- 10 unit tests covering round-trip fidelity, size limits, config omission, and preview categorization
- 15-plugin setup with 2 custom sources encodes to under 500 characters

## Task Commits

Each task was committed atomically:

1. **Task 1: Create portable.rs module with encode/decode pipeline and unit tests** - `462a9c2` (feat)
2. **Task 2: Update export command to produce portable string by default with legacy fallback** - `46e20c7` (feat)

## Files Created/Modified
- `crates/apm-cli/src/portable.rs` - Portable setup encode/decode pipeline, data model, import preview, 10 unit tests
- `crates/apm-cli/Cargo.toml` - Added flate2 and base64 dependencies
- `crates/apm-cli/src/main.rs` - Added mod portable, changed export default format to "portable"
- `crates/apm-cli/src/commands/export_cmd.rs` - Added portable format path, refactored into run_portable and run_legacy helpers

## Decisions Made
- Used URL_SAFE_NO_PAD base64 encoding for URL-safe strings without trailing = padding
- Used Compression::best() DEFLATE for maximum compression (15 plugins < 500 chars)
- Portable file export writes confirmation to stderr (not stdout) to keep stdout clean for piping
- Kept ExportDocument/ExportedPlugin structs for legacy format paths and import_cmd.rs compatibility

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness
- portable.rs encode/decode pipeline ready for Plan 02 to wire up the import/decode path
- ImportPreview and build_preview ready for Plan 02's import command integration
- All existing tests pass with zero regressions

---
*Phase: 14-portable-setup*
*Completed: 2026-04-07*
