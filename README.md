# apm — Audio Plugin Manager

apt-style package manager for macOS AU and VST3 audio plugins.

apm lets you install, update, remove, and search free audio plugins from the
command line. It pulls plugin definitions from Git-backed registries, verifies
SHA256 checksums before installing, and tracks everything in a local state file.

## Installation

**From source (recommended during development):**

```sh
cargo build --release
# Binary will be at ./target/release/apm
```

**Install system-wide via Cargo:**

```sh
cargo install --path .
```

Requires Rust 1.70 or later. Install Rust via [rustup.rs](https://rustup.rs/).

## Usage

### Sync the registry

Pull the latest plugin definitions from the configured registry sources.
Run this first, and periodically to get new plugin versions.

```sh
apm sync
```

### Search for plugins

```sh
# Search by keyword (name, vendor, description, tags)
apm search reverb

# Filter by category
apm search --category instruments

# List all available plugins
apm search
```

### Get plugin details

```sh
apm info valhalla-supermassive
apm info surge-xt
```

### Install a plugin

```sh
# Install all available formats (AU + VST3)
apm install tal-noisemaker

# Install only VST3
apm install tal-noisemaker --format vst3

# Install to system directory (/Library/Audio/Plug-Ins/) — requires sudo
sudo apm install tal-noisemaker --system
```

Plugins are installed to `~/Library/Audio/Plug-Ins/` by default.

### List installed plugins

```sh
# List plugins installed by apm
apm list

# Scan all AU/VST3 plugins on the system (apm-managed and unmanaged)
apm scan
```

### Remove a plugin

```sh
apm remove tal-noisemaker
```

### Check for updates

```sh
# Show outdated plugins
apm outdated

# Upgrade all outdated plugins
apm upgrade

# Upgrade a specific plugin
apm upgrade surge-xt
```

### Pin a plugin

Pinned plugins are skipped by `apm upgrade`.

```sh
# Pin a plugin to its current version
apm pin vital

# Unpin a plugin
apm pin vital --unpin

# List all pinned plugins
apm pin --list
```

### Manage registry sources

apm supports multiple Git-backed registry sources, similar to apt's sources.list.

```sh
# List configured sources
apm sources list

# Add a custom registry
apm sources add https://github.com/your-org/apm-registry --name my-registry

# Remove a source
apm sources remove my-registry
```

## Registry format

Plugin definitions are TOML files stored in `registry/plugins/`. Each file
describes one plugin and its download locations for each format.

Example (`registry/plugins/valhalla-supermassive.toml`):

```toml
slug        = "valhalla-supermassive"
name        = "Valhalla Supermassive"
vendor      = "Valhalla DSP"
version     = "5.0.0"
description = "Massive reverb and delay with lush modulation."
category    = "effects"
subcategory = "reverb"
license     = "freeware"
tags        = ["reverb", "delay", "free", "freeware"]
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

**Fields:**

| Field | Required | Description |
|-------|----------|-------------|
| `slug` | yes | Unique identifier used in CLI commands |
| `name` | yes | Display name |
| `vendor` | yes | Plugin developer or company |
| `version` | yes | Semver or freeform version string |
| `description` | yes | One or two sentence description |
| `category` | yes | `"instruments"` or `"effects"` |
| `subcategory` | yes | e.g. `"reverb"`, `"synthesizer"`, `"eq"` |
| `license` | yes | SPDX identifier or `"freeware"` |
| `tags` | yes | Array of search keywords |
| `homepage` | yes | Official product page URL |
| `[formats.vst3]` or `[formats.au]` | at least one | Format-specific download info |
| `url` | yes | Direct download URL |
| `sha256` | yes | Hex SHA256 of the downloaded file |
| `install_type` | yes | `"dmg"`, `"pkg"`, or `"zip"` |
| `bundle_path` | for zip | Path inside archive to the plugin bundle |

## Contributing plugins

To add a plugin to the official registry:

1. Fork this repository.
2. Create `registry/plugins/<slug>.toml` following the format above.
3. Download the macOS installer and compute the real SHA256:
   ```sh
   shasum -a 256 /path/to/installer.dmg
   ```
4. Open a pull request. CI will validate the TOML structure.

Guidelines:
- Only include plugins that are genuinely free (no time-limited trials).
- Use the official developer download URL, not a mirror.
- If a download requires account signup, note it with a comment in the TOML.
- One file per plugin slug.

## License

MIT
