#!/usr/bin/env python3
"""
Scan installed plugins, match them against the registry using the apm CLI,
and learn bundle IDs to populate the registry's bundle_ids field.

Reads the scanned plugins' Info.plist files to extract CFBundleIdentifier,
then writes the learned mappings back to the `registry-src/` plugin sources.

Usage:
    python3 scripts/learn-bundle-ids.py
"""

import json
import os
import plistlib
import re
import subprocess
import sys
from pathlib import Path

REGISTRY_DIR = Path(__file__).parent.parent / "registry-src" / "plugins"

PLUGIN_DIRS = [
    Path.home() / "Library/Audio/Plug-Ins/Components",
    Path("/Library/Audio/Plug-Ins/Components"),
    Path.home() / "Library/Audio/Plug-Ins/VST3",
    Path("/Library/Audio/Plug-Ins/VST3"),
]


def scan_bundle_ids() -> list[dict]:
    """Scan all plugin directories and extract bundle IDs + names."""
    results = []
    for plugin_dir in PLUGIN_DIRS:
        if not plugin_dir.exists():
            continue
        for entry in sorted(plugin_dir.iterdir()):
            if not entry.is_dir():
                continue
            ext = entry.suffix.lower()
            if ext not in ('.component', '.vst3'):
                continue
            plist_path = entry / "Contents" / "Info.plist"
            if not plist_path.exists():
                continue
            try:
                with open(plist_path, 'rb') as f:
                    info = plistlib.load(f)
                name = info.get('CFBundleName', entry.stem)
                bundle_id = info.get('CFBundleIdentifier', '')
                vendor = ''
                # Extract vendor from AU AudioComponents
                if ext == '.component':
                    components = info.get('AudioComponents', [])
                    if components and isinstance(components[0], dict):
                        au_name = components[0].get('name', '')
                        if ':' in au_name:
                            vendor = au_name.split(':')[0].strip()
                results.append({
                    'name': name,
                    'bundle_id': bundle_id,
                    'vendor': vendor,
                    'format': 'AU' if ext == '.component' else 'VST3',
                    'path': str(entry),
                })
            except Exception as e:
                print(f"  Warning: {entry.name}: {e}", file=sys.stderr)
    return results


def normalize(s: str) -> str:
    """Normalize for matching — same logic as the Rust matcher."""
    s = s.lower()
    # Strip trailing version
    s = re.sub(r'\s+v?\d+(\.\d+)*\s*$', '', s)
    # Remove all non-alphanumeric
    s = re.sub(r'[^a-z0-9]', '', s)
    return s


def extract_bundle_id_prefix(bundle_id: str) -> str:
    """Extract the stable prefix of a bundle ID, removing format/version suffixes.

    e.g., "com.fabfilter.Pro-Q.AU.4" -> "com.fabfilter.Pro-Q"
          "com.fabfilter.Pro-Q.Vst3.4" -> "com.fabfilter.Pro-Q"
    """
    # Remove trailing .AU.N, .Vst3.N, .MusicDevice.component, .vst3 etc.
    bid = bundle_id
    bid = re.sub(r'\.(AU|Vst3|VST3|AAX|MusicDevice|MusicEffect|audiounit)\.\d+$', '', bid, flags=re.I)
    bid = re.sub(r'\.(AU|Vst3|VST3|AAX|MusicDevice|MusicEffect|audiounit|component|vst3)$', '', bid, flags=re.I)
    # Remove trailing version number
    bid = re.sub(r'\.\d+$', '', bid)
    return bid


def main():
    print("Scanning installed plugins...", file=sys.stderr)
    scanned = scan_bundle_ids()
    print(f"  Found {len(scanned)} plugin bundles", file=sys.stderr)

    # Group by bundle ID prefix to deduplicate AU/VST3 variants
    by_prefix: dict[str, dict] = {}
    for p in scanned:
        if not p['bundle_id']:
            continue
        prefix = extract_bundle_id_prefix(p['bundle_id'])
        if prefix not in by_prefix:
            by_prefix[prefix] = p

    print(f"  {len(by_prefix)} unique bundle ID prefixes", file=sys.stderr)

    # Load registry TOML files and build name index
    registry_by_norm: dict[str, list[str]] = {}  # normalized name -> [toml filenames]
    registry_files: dict[str, str] = {}  # filename -> content

    for f in sorted(REGISTRY_DIR.iterdir()):
        if not f.name.endswith('.toml'):
            continue
        content = f.read_text()
        registry_files[f.name] = content
        for line in content.split('\n'):
            if line.startswith('name = "'):
                name = line.split('"')[1]
                norm = normalize(name)
                registry_by_norm.setdefault(norm, []).append(f.name)
                break

    # Match scanned plugins to registry entries
    matched = 0
    updated = 0

    for prefix, plugin in sorted(by_prefix.items()):
        norm_name = normalize(plugin['name'])

        candidates = registry_by_norm.get(norm_name, [])
        if not candidates:
            continue

        matched += 1

        # Update the first matching TOML file with the bundle ID prefix
        toml_file = candidates[0]
        content = registry_files[toml_file]

        # Check if bundle_ids already has this prefix
        if f'"{prefix}"' in content:
            continue

        # Add or update bundle_ids field
        if 'bundle_ids = [' in content:
            # Append to existing list
            content = content.replace(
                'bundle_ids = [',
                f'bundle_ids = ["{prefix}", ',
            )
        elif 'bundle_ids = []' in content:
            content = content.replace(
                'bundle_ids = []',
                f'bundle_ids = ["{prefix}"]',
            )
        else:
            # Insert before is_paid
            content = content.replace(
                'is_paid = true',
                f'bundle_ids = ["{prefix}"]\nis_paid = true',
            )

        toml_path = REGISTRY_DIR / toml_file
        toml_path.write_text(content)
        registry_files[toml_file] = content
        updated += 1
        print(f"  {plugin['name']:35s} -> {toml_file:40s} ({prefix})")

    print(f"\nMatched: {matched}, Updated: {updated}", file=sys.stderr)


if __name__ == "__main__":
    main()
