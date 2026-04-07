---
name: apm
description: Audio Plugin Manager — install, search, and manage macOS AU/VST3 audio plugins. Use when the user mentions audio plugins, VSTs, DAW plugins, synthesizers, or wants to install/manage music production software.
argument-hint: [search|install|info|list] [plugin-name or query]
disable-model-invocation: false
user-invocable: true
allowed-tools: Bash(apm *) Bash(which apm) Bash(brew *apm*) Read
---

# APM — Audio Plugin Manager

Help users install and manage macOS audio plugins (AU/VST3) using the `apm` CLI.

## First: Check if APM is installed

```bash
which apm 2>/dev/null || echo "NOT_INSTALLED"
```

**If NOT_INSTALLED**, guide the user through installation:

```bash
# Option 1: Homebrew (recommended)
brew tap andreanjos/apm https://github.com/andreanjos/apm
brew install apm

# Option 2: From source (requires Rust)
cargo install --git https://github.com/andreanjos/apm --bin apm
```

After installation, set up the local registry:

```bash
# Point apm at the bundled registry (or sync from remote)
apm sync
```

## Parse the user's request

Extract the intent from `$ARGUMENTS` or the conversation:

| Intent | Command |
|--------|---------|
| Search for plugins | `apm search [query]` |
| Install a plugin | `apm install [slug]` |
| Plugin details | `apm info [slug]` |
| List installed | `apm list` |
| Check for updates | `apm outdated` |
| Upgrade plugins | `apm upgrade` |
| Remove a plugin | `apm remove [slug]` |
| Scan system plugins | `apm scan` |
| Plugin health check | `apm doctor` |
| Browse by category | `apm categories` |
| Discover random plugin | `apm random` |
| Export setup | `apm export` |
| Import setup | `apm import [string-or-file]` |
| Environment info | `apm env` |

## Workflow

1. Run the appropriate `apm` command based on user intent
2. Parse the output and present it clearly
3. Suggest logical follow-ups:
   - After search → "Want to install any of these?"
   - After install → "Plugin installed! Open your DAW to load it."
   - After outdated → "Run `apm upgrade` to update all?"
   - After info → "Want to install this? `apm install [slug]`"

## Common workflows

### "I want to find a free reverb plugin"
```bash
apm search --free --category effects reverb
```

### "Set up my production environment on a new Mac"
```bash
# Import from a portable setup string
apm import apm1://dGFsLW5v...

# Or install individually
apm install surge-xt tal-noisemaker valhalla-supermassive
```

### "What plugins do I have?"
```bash
apm list              # apm-managed plugins
apm scan              # ALL plugins on system (including manual installs)
apm scan --managed    # only apm-managed
```

### "What's taking up space?"
```bash
apm size
```

### "Share my setup with someone"
```bash
apm export  # outputs apm1:// string — paste it anywhere
```

## Tips

- Plugin slugs are lowercase with hyphens: `tal-noisemaker`, `surge-xt`, `valhalla-supermassive`
- Use `--json` on any command for structured output
- Use `--dry-run` on install/remove/upgrade to preview changes
- Plugins install to `~/Library/Audio/Plug-Ins/` by default
- Run `apm doctor` if anything seems broken
- `apm random` is fun for discovering new plugins
- Short aliases work: `apm i` (install), `apm s` (search), `apm ls` (list), `apm rm` (remove)
