# apm — Audio Plugin Manager

A package manager for macOS audio plugins. Install, update, remove, and discover
AU and VST3 plugins from the command line.

apm pulls plugin definitions from Git-backed registries (560+ plugins and
growing), verifies SHA256 checksums before installing, and tracks everything in a
local state file. Think `apt` or `brew`, but for your DAW.

## Installation

```sh
brew tap andreanjos/apm https://github.com/andreanjos/apm
brew install apm
```

Or build from source (requires Rust 1.70+):

```sh
cargo install --path crates/apm-cli
```

Enable shell completions (optional):

```sh
# Bash
apm completions bash > ~/.local/share/bash-completion/completions/apm

# Zsh
apm completions zsh > ~/.zfunc/_apm

# Fish
apm completions fish > ~/.config/fish/completions/apm.fish
```

## Quick start

```sh
apm sync                        # Pull latest plugin definitions
apm search reverb               # Find plugins by keyword
apm info valhalla-supermassive  # View plugin details
apm install tal-noisemaker      # Install a plugin
apm list                        # See what's installed
apm outdated                    # Check for updates
apm upgrade                     # Upgrade everything
```

## Commands

### Sync and search

```sh
apm sync                              # Pull latest registry
apm search reverb                     # Search by keyword
apm search --category instruments     # Filter by category
apm search --vendor "Valhalla DSP"    # Filter by vendor
apm info surge-xt                     # Plugin details
```

### Install and remove

```sh
apm install tal-noisemaker                    # Install (AU + VST3)
apm install tal-noisemaker --format vst3      # VST3 only
apm install tal-noisemaker --version 4.3.2    # Specific version
sudo apm install tal-noisemaker --system      # System-wide (/Library/)
apm install --from-file plugins.toml          # Batch install from file
apm install --dry-run surge-xt                # Preview without installing

apm remove tal-noisemaker                     # Remove a plugin
```

Plugins install to `~/Library/Audio/Plug-Ins/` by default.

### Updates and versioning

```sh
apm outdated            # List plugins with newer versions
apm upgrade             # Upgrade all
apm upgrade surge-xt    # Upgrade one

apm pin vital           # Pin to current version (skip upgrades)
apm pin vital --unpin   # Unpin
apm pin --list          # List pinned plugins
```

### System and diagnostics

```sh
apm list                # apm-managed plugins
apm scan                # All AU/VST3 on the system
apm doctor              # Run diagnostic checks
apm cleanup             # Clear download cache
apm rollback <slug>     # Restore from pre-upgrade backup
apm export > list.toml  # Export installed plugins
apm import list.toml    # Import and install from a list
```

### Registry sources

apm supports multiple Git-backed registries.

```sh
apm sources list
apm sources add https://github.com/your-org/apm-registry --name my-registry
apm sources remove my-registry
```

## Registry format

Plugin definitions are TOML files in `registry/plugins/`. Each file describes
one plugin and its download locations per format.

```toml
slug        = "valhalla-supermassive"
name        = "Valhalla Supermassive"
vendor      = "Valhalla DSP"
version     = "5.0.0"
description = "Massive reverb and delay with lush modulation."
category    = "effects"
subcategory = "reverb"
license     = "freeware"
tags        = ["reverb", "delay", "free"]
homepage    = "https://valhalladsp.com/shop/reverb/valhalla-supermassive/"

[formats.vst3]
url          = "https://example.com/ValhallaSupermassiveOSX_5_0_0.dmg"
sha256       = "eaac6d0a24ffed0a02afd1dd06124d12f94716d32a8ac376606aa2d701a70c3e"
install_type = "dmg"
bundle_path  = "ValhallaSupermassive.vst3"

[formats.au]
url          = "https://example.com/ValhallaSupermassiveOSX_5_0_0.dmg"
sha256       = "eaac6d0a24ffed0a02afd1dd06124d12f94716d32a8ac376606aa2d701a70c3e"
install_type = "dmg"
bundle_path  = "ValhallaSupermassive.component"
```

| Field | Required | Description |
|-------|----------|-------------|
| `slug` | yes | Unique identifier used in CLI commands |
| `name` | yes | Display name |
| `vendor` | yes | Developer or company |
| `version` | yes | Semver or freeform version string |
| `description` | yes | One or two sentence description |
| `category` | yes | `"instruments"` or `"effects"` |
| `subcategory` | yes | e.g. `"reverb"`, `"synthesizer"`, `"eq"` |
| `license` | yes | SPDX identifier or `"freeware"` |
| `tags` | yes | Search keywords |
| `homepage` | yes | Official product page URL |
| `formats.vst3` / `formats.au` | at least one | Format-specific download info |
| `url` | yes | Direct download URL |
| `sha256` | yes | SHA256 hex digest of the download |
| `install_type` | yes | `"dmg"`, `"pkg"`, or `"zip"` |
| `bundle_path` | for dmg/zip | Path inside the archive to the plugin bundle |

## Contributing plugins

1. Fork this repo.
2. Create `registry/plugins/<slug>.toml` following the format above.
3. Compute the SHA256 of the macOS installer:
   ```sh
   shasum -a 256 /path/to/installer.dmg
   ```
4. Open a pull request.

Guidelines:
- Only include plugins that are genuinely free (no time-limited trials).
- Use the official developer download URL, not a mirror.
- If a download requires account signup, note it with a comment in the TOML.
- One file per plugin slug.

## License

[MIT](LICENSE)
