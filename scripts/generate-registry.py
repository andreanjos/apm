#!/usr/bin/env python3
"""
Generate draft registry TOML files from the scraped MSD catalog.

This writes bootstrap data under `data/registry/` for review or further
normalization into `registry-src/`.

Usage:
    python3 scripts/generate-registry.py
"""

import json
import os
import re
import time
from collections import Counter
from pathlib import Path

CATALOG_PATH = Path(__file__).parent.parent / "data" / "msd-catalog.json"
OUTPUT_DIR = Path(__file__).parent.parent / "data" / "registry"
PLUGINS_DIR = OUTPUT_DIR / "plugins"

CATEGORY_MAP = {
    "mixing/mastering/production effects": "effects",
    "virtual instruments": "instruments",
}

PLUGIN_TOML_TEMPLATE = """\
slug = "{slug}"
name = "{name}"
vendor = "{vendor}"
version = "1.0.0"
description = ""
category = "{category}"
{subcategory_line}license = "commercial"
tags = []
is_paid = true
homepage = ""

[formats.vst3]
url = ""
sha256 = ""
install_type = "zip"
bundle_path = ""
download_type = "manual"

[formats.au]
url = ""
sha256 = ""
install_type = "zip"
bundle_path = ""
download_type = "manual"
"""


def slugify(name: str) -> str:
    """Convert a plugin name to a clean slug."""
    s = name.lower().strip()
    for ch in [" ", "_", ".", "–", "—", "/"]:
        s = s.replace(ch, "-")
    s = re.sub(r"[^a-z0-9-]", "", s)
    s = re.sub(r"-+", "-", s)
    return s.strip("-")


def vendor_dir(vendor: str) -> str:
    """Convert a vendor name to a stable directory name."""
    return slugify(vendor) or "unknown-vendor"


def escape_toml_string(s: str) -> str:
    """Escape a string for TOML."""
    return s.replace("\\", "\\\\").replace('"', '\\"')


def main():
    with open(CATALOG_PATH) as f:
        catalog = json.load(f)

    plugins = catalog["plugins"]
    print(f"Loaded {len(plugins)} plugins from catalog")

    # Filter out sample/expansion packs
    filtered = [
        p for p in plugins
        if "sample" not in p["category"] and "expansion" not in p["category"]
    ]
    print(f"After filtering samples/expansions: {len(filtered)} plugins")

    # Deduplicate and handle slug collisions
    slug_counts: Counter = Counter()
    slug_to_plugin: dict[str, dict] = {}
    collisions = 0

    for p in filtered:
        slug = slugify(p["name"])
        if not slug:
            continue

        if slug in slug_to_plugin:
            # Collision — append vendor
            vendor_slug = slugify(p["vendor"])
            slug = f"{slug}-{vendor_slug}"
            if slug in slug_to_plugin:
                continue  # true duplicate
            collisions += 1

        slug_to_plugin[slug] = p

    print(f"Unique slugs: {len(slug_to_plugin)} ({collisions} disambiguated with vendor)")

    # Generate TOML files
    os.makedirs(PLUGINS_DIR, exist_ok=True)

    index_entries = []

    for slug, p in sorted(slug_to_plugin.items()):
        category = CATEGORY_MAP.get(p["category"], p["category"])
        subcategory = p.get("subcategory")
        subcategory_line = f'subcategory = "{escape_toml_string(subcategory)}"\n' if subcategory else ""

        toml_content = PLUGIN_TOML_TEMPLATE.format(
            slug=escape_toml_string(slug),
            name=escape_toml_string(p["name"]),
            vendor=escape_toml_string(p["vendor"]),
            category=escape_toml_string(category),
            subcategory_line=subcategory_line,
        )

        vendor_path = PLUGINS_DIR / vendor_dir(p["vendor"])
        os.makedirs(vendor_path, exist_ok=True)
        toml_path = vendor_path / f"{slug}.toml"
        with open(toml_path, "w") as f:
            f.write(toml_content)

        index_entries.append((slug, f"plugins/{vendor_dir(p['vendor'])}/{slug}.toml"))

    # Generate index.toml
    now = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
    index_lines = [
        f'version = 1\n',
        f'generated = "{now}"\n',
        "",
    ]
    for slug, path in index_entries:
        index_lines.append("[[plugins]]")
        index_lines.append(f'name = "{slug}"')
        index_lines.append(f'path = "{path}"')
        index_lines.append('version = "1.0.0"')
        index_lines.append("")

    with open(OUTPUT_DIR / "index.toml", "w") as f:
        f.write("\n".join(index_lines))

    # Summary
    categories = Counter(
        CATEGORY_MAP.get(p["category"], p["category"])
        for p in slug_to_plugin.values()
    )
    vendors = Counter(p["vendor"] for p in slug_to_plugin.values())

    print(f"\nGenerated {len(slug_to_plugin)} TOML files in {PLUGINS_DIR}")
    print(f"Index: {OUTPUT_DIR / 'index.toml'}")
    print(f"\nBy category:")
    for cat, count in categories.most_common():
        print(f"  {cat:30s} {count}")
    print(f"\nTop 15 vendors:")
    for v, count in vendors.most_common(15):
        print(f"  {v:30s} {count}")


if __name__ == "__main__":
    main()
