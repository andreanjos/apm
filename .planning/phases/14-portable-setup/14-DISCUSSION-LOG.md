# Phase 14: Portable Setup (Import/Export) - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-04-06
**Phase:** 14-portable-setup
**Areas discussed:** Format, Scope, Import behavior

---

## Format

| Option | Description | Selected |
|--------|-------------|----------|
| Shareable string | apm1:// prefix + base64 compressed blob, pasteable anywhere | ✓ |
| Short code via Gist | Upload to server, get 6-char code like APM-7KX3 | |
| URL scheme | Human-readable URL with plugin names in path | |
| Compact TOML file | .apmsetup file that's small enough to paste | |

**User's choice:** Shareable string (Option A)
**Notes:** User liked the "string of characters" concept from the start. Chose A because it's self-contained with no server dependency.

---

## Scope (what's in the string)

| Option | Description | Selected |
|--------|-------------|----------|
| Plugins + versions only | Minimal, short strings | |
| Plugins + versions + pins | Most common use case | |
| Full setup | Plugins, versions, pins, sources, config preferences | ✓ |

**User's choice:** Full setup — "All of it"
**Notes:** User wants the string to be a complete environment snapshot. Machine-specific paths excluded by Claude's discretion.

---

## Output modes

| Option | Description | Selected |
|--------|-------------|----------|
| String only | Always stdout | |
| File only | Always write to disk | |
| Both (string default, file optional) | String to stdout, -o flag for file | ✓ |

**User's choice:** Both — "All of it"

---

## Import behavior

| Option | Description | Selected |
|--------|-------------|----------|
| Always preview + confirm | Show diff, ask Y/n | ✓ |
| Just do it (apt-style) | Install immediately, --dry-run for preview | |
| Flag to skip confirmation | --yes for automation | ✓ (also included) |

**User's choice:** Preview + confirm by default, --yes to skip, --dry-run for preview-only

---

## Claude's Discretion

- Compression algorithm (zlib vs brotli vs lz4)
- Internal serialization format before compression
- apm1:// prefix parsing and validation
- Version conflict resolution strategy (keep newer)

## Deferred Ideas

None
