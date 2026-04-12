# apm — Audio Plugin Manager

A package manager for macOS audio plugins. Install, update, remove, and discover
AU and VST3 plugins from the command line.

apm pulls catalog definitions from Git-backed registries, validates downloads
before installing, and tracks what is already installed on disk. The registry
catalog includes 8,000+ entries covering standalone plugins, bundles, upgrades,
expansions, preset packs, sample libraries, utilities, and vendor-managed
products.

## Installation

```sh
brew tap andreanjos/apm https://github.com/andreanjos/apm
brew install apm
```

Or build from source (requires Rust 1.70+):

```sh
cargo install --path crates/apm-cli
```

### Claude Code

```sh
cp -r .claude/skills/apm ~/.claude/skills/
```

Then use `/apm search reverb` or `/apm install surge-xt` directly in Claude Code.

## Quick start

```sh
apm sync                        # Pull latest catalog definitions
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

`apm info` shows the product type and access mode so it is clear whether a
record is a standalone plugin, bundle, upgrade, or vendor-managed product.

### Install and remove

```sh
apm install tal-noisemaker                    # Install (AU + VST3)
apm install tal-noisemaker --format vst3      # VST3 only (also supports au/app)
apm install tal-noisemaker --version 4.3.2    # Specific version
sudo apm install tal-noisemaker --system      # System-wide (/Library/)
printf "vital\nsurge-xt\n" | apm install --stdin
apm install --dry-run surge-xt                # Preview without installing
apm install massive-x                         # Opens Native Access when required

apm remove tal-noisemaker                     # Remove a plugin
```

Audio plugins install to `~/Library/Audio/Plug-Ins/` by default. App-format
entries install to `~/Applications/` unless `--system` is used.

apm supports three install modes:

- Direct: apm downloads the archive and installs it itself.
- Managed: apm opens the required vendor installer app, such as Native Access,
  Arturia Software Center, Waves Central, iLok License Manager, or UA Connect.
  After installing there, run `apm scan`.
- Manual: apm opens the product page or download page. Install outside apm,
  then run `apm scan` so apm can detect the plugin on disk later.

### Updates and versioning

```sh
apm outdated            # List plugins with newer versions
apm upgrade             # Upgrade all
apm upgrade surge-xt    # Upgrade one

apm pin vital           # Pin to current version (skip upgrades)
apm pin vital --unpin   # Unpin
apm pin --list          # List pinned plugins
```

### Portable setup

Export your entire setup as a shareable string - paste it in Slack, a README,
or a terminal on another machine:

```sh
apm export                          # Outputs apm1://... string to stdout
apm export -o setup.apmsetup        # Save to file instead

apm import apm1://dGFsLW5v...        # Import from string (preview + confirm)
apm import setup.apmsetup            # Import from file
apm import --dry-run apm1://...      # Preview what would change
apm import --yes apm1://...          # Skip confirmation (for scripts)
```

The string encodes installed plugins, versions, pins, registry sources, and
preferences. Use `apm export --format toml` or `--format json` when you want an
editable file instead of the portable `apm1://...` string.

### System and diagnostics

```sh
apm list                # apm-managed plugins
apm scan                # All AU/VST3 on the system; tracks matched external installs
apm doctor              # Run diagnostic checks
apm cleanup             # Clear download cache
apm rollback <slug>     # Restore from pre-upgrade backup
```

`apm scan` is the bridge for manual and vendor-managed installs: it records what
is already on disk without asking you to re-enter file paths.

### Registry sources

apm supports multiple Git-backed registries.

```sh
apm sources list
apm sources add https://github.com/your-org/apm-registry --name my-registry
apm sources remove my-registry
```

## Optional setup

### Shell completions

```sh
# Bash
apm completions bash > ~/.local/share/bash-completion/completions/apm

# Zsh
apm completions zsh > ~/.zsh/completions/_apm

# Fish
apm completions fish > ~/.config/fish/completions/apm.fish
```

## Registry format

The published registry is a Git repo with:

- `registry/index.toml`
- `registry/installers.toml`
- `registry/bundles/*.toml`
- `registry/plugins/<vendor>/<slug>.toml`

```toml
slug         = "valhalla-supermassive"
aliases      = ["supermassive"]
name         = "Valhalla Supermassive"
vendor       = "Valhalla DSP"
version      = "5.0.0"
product_type = "plugin"
description  = "Massive reverb and delay with lush modulation."
category     = "effects"
subcategory  = "reverb"
license      = "freeware"
tags         = ["reverb", "delay", "free"]
homepage     = "https://valhalladsp.com/shop/reverb/valhalla-supermassive/"

[formats.vst3]
url          = "https://example.com/ValhallaSupermassiveOSX_5_0_0.dmg"
sha256       = "eaac6d0a24ffed0a02afd1dd06124d12f94716d32a8ac376606aa2d701a70c3e"
install_type = "dmg"
download_type = "direct"
bundle_path  = "ValhallaSupermassive.vst3"

[formats.au]
url          = "https://example.com/ValhallaSupermassiveOSX_5_0_0.dmg"
sha256       = "eaac6d0a24ffed0a02afd1dd06124d12f94716d32a8ac376606aa2d701a70c3e"
install_type = "dmg"
download_type = "direct"
bundle_path  = "ValhallaSupermassive.component"
```

| Field | Required | Description |
|-------|----------|-------------|
| `slug` | yes | Unique identifier used in CLI commands |
| `aliases` | no | Alternate slugs that resolve to this record |
| `name` | yes | Display name |
| `vendor` | yes | Developer or company |
| `version` | yes | Semver or freeform version string |
| `product_type` | yes | `plugin`, `bundle`, `upgrade`, `subscription`, and similar catalog types |
| `description` | yes | One or two sentence description |
| `category` | yes | Registry category such as `"effects"`, `"instruments"`, or `"daws"` |
| `subcategory` | no | e.g. `"reverb"`, `"synthesizer"`, `"eq"` |
| `license` | yes | SPDX identifier or `"freeware"` |
| `tags` | yes | Search keywords |
| `installer` | no | Vendor manager key from `registry/installers.toml` |
| `homepage` | no | Official product page URL |
| `purchase_url` | no | Product purchase page |
| `releases` | no | Historical versions for explicit installs |
| `bundle_ids` | no | Known CFBundleIdentifier prefixes for scanner matching |
| `formats.*` | at least one | Format-specific download info such as `au`, `vst3`, or `app` |
| `url` | yes | Direct archive URL for `direct` downloads, or the official product/download page for `manual` and `managed` entries |
| `sha256` | for direct downloads | SHA256 hex digest of the direct archive; manual and vendor-managed entries may leave this blank |
| `install_type` | yes | `"dmg"`, `"pkg"`, or `"zip"` |
| `download_type` | no | `"direct"`, `"manual"`, or `"managed"` |
| `bundle_path` | for dmg/zip | Path inside the archive to the plugin bundle |

## Contributing plugins

1. Fork this repo.
2. Add or update the relevant registry records:
   - `registry/plugins/<vendor>/<slug>.toml`
   - `registry/installers.toml` when a vendor manager is needed
   - `registry/bundles/*.toml` when bundle membership changes
3. Compute the SHA256 of the macOS installer:
   ```sh
   shasum -a 256 /path/to/installer.dmg
   ```
4. Open a pull request.

Guidelines:
- Only include products that are genuinely installable and not temporary trials.
- Use the official developer download URL, not a mirror.
- If a download requires account signup, note it with a comment in the TOML.
- Mark non-standalone catalog entries with `product_type` so search results stay honest.

## License

apm is released under the MIT License. See [LICENSE](./LICENSE) for details.
