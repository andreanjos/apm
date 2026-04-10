#!/usr/bin/env python3
"""
Merge Plugin Boutique and MSD catalogs into enriched draft registry TOML files.

This writes draft data under `data/registry/` for review or further
normalization into `registry-src/`.

Priority: PB products (have purchase URLs) > MSD entries (broader coverage).
Cross-references by normalized name+vendor to avoid duplicates.

Usage:
    python3 scripts/enrich-registry.py [--affiliate-id YOUR_ID]
"""

import argparse
import html
import json
import os
import re
import time
from collections import Counter
from pathlib import Path

PB_CATALOG = Path(__file__).parent.parent / "data" / "pb-catalog.json"
MSD_CATALOG = Path(__file__).parent.parent / "data" / "msd-catalog.json"
OUTPUT_DIR = Path(__file__).parent.parent / "data" / "registry"
PLUGINS_DIR = OUTPUT_DIR / "plugins"

PB_BASE = "https://www.pluginboutique.com"

# Map PB categories to our registry categories
PB_CATEGORY_MAP = {
    "effects": "effects",
    "instruments": "instruments",
    "studio tools": "tools",
    "bundles": "bundle",
    "sample packs": "samples",
    "music courses": "education",
    "loopcloud": "samples",
}

MSD_CATEGORY_MAP = {
    "mixing/mastering/production effects": "effects",
    "virtual instruments": "instruments",
    "sample/sound/expansion packs": "samples",
}

TOML_TEMPLATE = """\
slug = "{slug}"
name = "{name}"
vendor = "{vendor}"
version = "1.0.0"
description = "{description}"
category = "{category}"
{subcategory_line}license = "commercial"
tags = [{tags}]
is_paid = true
{homepage_line}{purchase_url_line}
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


def clean_text(s: str) -> str:
    """Decode HTML entities and clean whitespace."""
    s = html.unescape(s)
    s = re.sub(r"\s+", " ", s).strip()
    return s


def normalize_for_matching(s: str) -> str:
    """Normalize a name for fuzzy matching."""
    s = clean_text(s).lower()
    # Remove version numbers
    s = re.sub(r"\s+v?\d+(\.\d+)*\s*$", "", s)
    # Remove common suffixes
    for suffix in [" bundle", " collection", " upgrade", " crossgrade", " native"]:
        s = s.removesuffix(suffix)
    # Remove punctuation
    s = re.sub(r"[^a-z0-9 ]", "", s)
    s = re.sub(r"\s+", " ", s).strip()
    return s


def slugify(name: str) -> str:
    """Convert a name to a registry slug."""
    s = clean_text(name).lower()
    for ch in [" ", "_", ".", "–", "—", "/"]:
        s = s.replace(ch, "-")
    s = re.sub(r"[^a-z0-9-]", "", s)
    s = re.sub(r"-+", "-", s)
    return s.strip("-")


def vendor_dir(vendor: str) -> str:
    """Convert a vendor name to a stable directory name."""
    return slugify(vendor) or "unknown-vendor"


def escape_toml(s: str) -> str:
    """Escape a string for TOML double-quoted values."""
    return s.replace("\\", "\\\\").replace('"', '\\"')


def build_pb_entries(pb_data: dict, affiliate_id: str) -> list[dict]:
    """Convert PB products into registry entries."""
    entries = []
    for prod in pb_data["products"]:
        name = clean_text(prod["name"])
        vendor = clean_text(prod["manufacturer"])
        cat_raw = prod.get("category", "").lower()
        subcat_raw = clean_text(prod.get("subcategory", ""))
        category = PB_CATEGORY_MAP.get(cat_raw, cat_raw)

        # Skip non-plugin categories
        if category in ("education", "samples", "bundle"):
            continue

        # Skip upgrades/crossgrades
        if prod.get("is_upgrade") or re.search(r"upgrade|crossgrade", name, re.I):
            continue

        # Build purchase URL
        url_path = prod["url"]
        if affiliate_id:
            purchase_url = f"{PB_BASE}{url_path}?a={affiliate_id}"
        else:
            purchase_url = f"{PB_BASE}{url_path}"

        entries.append({
            "name": name,
            "vendor": vendor,
            "category": category,
            "subcategory": subcat_raw.lower() if subcat_raw else None,
            "purchase_url": purchase_url,
            "source": "pluginboutique",
            "_match_key": normalize_for_matching(name),
            "_vendor_key": normalize_for_matching(vendor),
        })

    return entries


def build_msd_entries(msd_data: dict) -> list[dict]:
    """Convert MSD plugins into registry entries."""
    entries = []
    for plugin in msd_data["plugins"]:
        name = clean_text(plugin["name"])
        vendor = clean_text(plugin["vendor"])
        cat_raw = plugin.get("category", "").lower()
        subcat_raw = plugin.get("subcategory")
        category = MSD_CATEGORY_MAP.get(cat_raw, cat_raw)

        if category in ("samples",):
            continue

        entries.append({
            "name": name,
            "vendor": vendor,
            "category": category,
            "subcategory": subcat_raw,
            "purchase_url": None,
            "source": "msd",
            "_match_key": normalize_for_matching(name),
            "_vendor_key": normalize_for_matching(vendor),
        })

    return entries


def merge_entries(pb_entries: list[dict], msd_entries: list[dict]) -> list[dict]:
    """Merge PB and MSD entries, preferring PB (has purchase URLs)."""
    # Index PB entries by match key
    pb_index: dict[str, dict] = {}
    for entry in pb_entries:
        key = entry["_match_key"]
        pb_index[key] = entry

    # Also index by (name, vendor) for stricter matching
    pb_nv_index: dict[tuple, dict] = {}
    for entry in pb_entries:
        key = (entry["_match_key"], entry["_vendor_key"])
        pb_nv_index[key] = entry

    merged = list(pb_entries)  # Start with all PB entries
    added_from_msd = 0
    skipped_duplicates = 0

    for entry in msd_entries:
        key = entry["_match_key"]
        nv_key = (entry["_match_key"], entry["_vendor_key"])

        # Skip if PB already has this (by name or name+vendor)
        if key in pb_index or nv_key in pb_nv_index:
            skipped_duplicates += 1
            continue

        merged.append(entry)
        added_from_msd += 1

    print(f"  PB entries: {len(pb_entries)}")
    print(f"  MSD entries: {len(msd_entries)}")
    print(f"  Duplicates skipped: {skipped_duplicates}")
    print(f"  Added from MSD: {added_from_msd}")
    print(f"  Total merged: {len(merged)}")

    return merged


def write_registry(entries: list[dict]):
    """Write registry TOML files and index."""
    os.makedirs(PLUGINS_DIR, exist_ok=True)

    # Handle slug collisions
    slug_counts: Counter = Counter()
    final = []

    for entry in entries:
        slug = slugify(entry["name"])
        if not slug:
            continue

        slug_counts[slug] += 1
        if slug_counts[slug] > 1:
            vendor_slug = slugify(entry["vendor"])
            slug = f"{slug}-{vendor_slug}"
            if slug_counts.get(slug, 0) > 0:
                continue
        slug_counts[slug] = slug_counts.get(slug, 0)

        entry["slug"] = slug
        final.append(entry)

    # Write TOML files
    for entry in final:
        subcat = entry.get("subcategory")
        subcategory_line = f'subcategory = "{escape_toml(subcat)}"\n' if subcat else ""
        homepage_line = ""
        purchase_url_line = ""
        if entry.get("purchase_url"):
            purchase_url_line = f'purchase_url = "{escape_toml(entry["purchase_url"])}"\n'

        tags_str = ""

        content = TOML_TEMPLATE.format(
            slug=escape_toml(entry["slug"]),
            name=escape_toml(entry["name"]),
            vendor=escape_toml(entry["vendor"]),
            description="",
            category=escape_toml(entry["category"]),
            subcategory_line=subcategory_line,
            tags=tags_str,
            homepage_line=homepage_line,
            purchase_url_line=purchase_url_line,
        )

        vendor_path = PLUGINS_DIR / vendor_dir(entry["vendor"])
        os.makedirs(vendor_path, exist_ok=True)
        path = vendor_path / f"{entry['slug']}.toml"
        with open(path, "w") as f:
            f.write(content)

    # Write index
    now = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
    index_lines = [f'version = 1\n', f'generated = "{now}"\n', ""]
    for entry in sorted(final, key=lambda e: e["slug"]):
        index_lines.extend([
            "[[plugins]]",
            f'name = "{entry["slug"]}"',
            f'path = "plugins/{vendor_dir(entry["vendor"])}/{entry["slug"]}.toml"',
            'version = "1.0.0"',
            "",
        ])

    with open(OUTPUT_DIR / "index.toml", "w") as f:
        f.write("\n".join(index_lines))

    return final


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--affiliate-id", default="", help="Plugin Boutique affiliate ID")
    args = parser.parse_args()

    print("Loading catalogs...")
    with open(PB_CATALOG) as f:
        pb_data = json.load(f)
    with open(MSD_CATALOG) as f:
        msd_data = json.load(f)

    print(f"  PB: {pb_data['total_products']} products")
    print(f"  MSD: {msd_data['unique_plugins']} plugins")

    print("\nBuilding entries...")
    pb_entries = build_pb_entries(pb_data, args.affiliate_id)
    msd_entries = build_msd_entries(msd_data)
    print(f"  PB (after filtering): {len(pb_entries)}")
    print(f"  MSD (after filtering): {len(msd_entries)}")

    print("\nMerging...")
    merged = merge_entries(pb_entries, msd_entries)

    print("\nWriting registry...")
    final = write_registry(merged)

    # Stats
    sources = Counter(e["source"] for e in final)
    categories = Counter(e["category"] for e in final)
    with_purchase_url = sum(1 for e in final if e.get("purchase_url"))

    print(f"\n{'='*50}")
    print(f"Registry: {len(final)} plugins")
    print(f"  With purchase URL: {with_purchase_url}")
    print(f"  Without: {len(final) - with_purchase_url}")
    print(f"\nBy source:")
    for src, count in sources.most_common():
        print(f"  {src:20s} {count}")
    print(f"\nBy category:")
    for cat, count in categories.most_common():
        print(f"  {cat:20s} {count}")


if __name__ == "__main__":
    main()
