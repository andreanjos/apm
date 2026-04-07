---
phase: 14-portable-setup
plan: 02
subsystem: cli
tags: [import, portable, apm1, preview, confirmation, round-trip]

# Dependency graph
requires:
  - phase: 14-portable-setup plan 01
    provides: portable.rs module with decode, build_preview, ImportPreview, PortableSetup
provides:
  - "Import command accepting apm1:// strings directly or from .apmsetup files"
  - "Preview/confirm UX with --yes and --dry-run flags"
  - "Version conflict resolution keeping newer installed version"
  - "Source reconciliation with URL mismatch warnings"
  - "Legacy TOML/JSON import backward compatibility"
  - "8 integration tests for export/import round-trip"
affects: []

# Tech tracking
tech-stack:
  added: []
  patterns: [input auto-detection (apm1:// vs file), preview-before-apply UX, dual-path legacy/portable import]

key-files:
  created: []
  modified:
    - crates/apm-cli/src/commands/import_cmd.rs
    - crates/apm-cli/src/main.rs
    - crates/apm-cli/tests/cli_tests.rs

key-decisions:
  - "Input auto-detection: apm1:// prefix check first, then file existence, then file content sniffing"
  - "Legacy TOML/JSON path preserved as separate run_legacy function for zero-risk backward compat"
  - "Portable path loads state twice (once for preview, once for execution) to support dry-run cleanly"

patterns-established:
  - "detect_input pattern: string-first detection with file-content sniffing fallback"
  - "Preview-then-confirm UX: show categorized diff, summary line, then Y/n prompt"
  - "setup_fixture_env_with_state helper for integration tests needing registry + state"

requirements-completed: [D-08, D-09, D-10, D-11, D-12, D-13, D-14, D-15]

# Metrics
duration: 4min
completed: 2026-04-07
---

# Phase 14 Plan 02: Import Command Overhaul Summary

**apm1:// string import with categorized preview, confirmation prompt, --yes/--dry-run flags, version conflict resolution, and 8 integration tests**

## Performance

- **Duration:** 4 min
- **Started:** 2026-04-07T06:47:44Z
- **Completed:** 2026-04-07T06:51:21Z
- **Tasks:** 2
- **Files modified:** 3

## Accomplishments
- Overhauled import command to accept apm1:// strings directly, .apmsetup files, or legacy TOML/JSON
- Built categorized preview showing installs, skips, version conflicts, pin changes, sources, and config diffs
- Added --yes flag for scriptable automation and --dry-run for safe preview
- Preserved legacy TOML/JSON import path with zero behavioral change via run_legacy helper
- Added 8 integration tests covering export formats, import paths, error cases, and legacy regression

## Task Commits

Each task was committed atomically:

1. **Task 1: Overhaul import command with apm1:// support, preview/confirm UX, and CLI flag changes** - `ee55750` (feat)
2. **Task 2: Integration tests for portable export/import round-trip and edge cases** - `7aa4319` (test)

## Files Created/Modified
- `crates/apm-cli/src/commands/import_cmd.rs` - Rewritten: detect_input auto-detection, run_portable with preview/confirm, run_legacy preserving old behavior
- `crates/apm-cli/src/main.rs` - Import command: file:PathBuf -> input:String, added --yes flag
- `crates/apm-cli/tests/cli_tests.rs` - 8 new integration tests: export formats, portable/file import, invalid input, legacy regression

## Decisions Made
- Input auto-detection checks apm1:// prefix first (fast path), then checks if path exists and sniffs file content for apm1:// prefix, then falls back to legacy file format
- Legacy path preserved as a completely separate code path (run_legacy) to avoid any risk of breaking existing import behavior
- Portable import loads state twice (once for build_preview, once for execution) which is a clean separation between preview and mutation phases

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness
- Portable setup feature is complete: export produces apm1:// strings, import consumes them with preview/confirm
- All 41 integration tests pass (33 existing + 8 new), 26 unit tests pass
- Phase 14 is fully complete

---
*Phase: 14-portable-setup*
*Completed: 2026-04-07*
