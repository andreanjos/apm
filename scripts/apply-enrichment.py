#!/usr/bin/env python3
"""
Apply enriched plugin data (descriptions, homepages, versions) back to
registry source TOML files.

Reads *-enriched.json files from /tmp/apm-enrich/ and patches the
corresponding TOML files in registry-src/plugins/.

Usage:
    python3 scripts/apply-enrichment.py
"""

import json
import os
import re
import sys
from pathlib import Path

ENRICH_DIR = Path("/tmp/apm-enrich")
PLUGINS_DIR = Path(__file__).parent.parent / "registry-src" / "plugins"


def escape_toml(s: str) -> str:
    return s.replace("\\", "\\\\").replace('"', '\\"')


def slugify(name: str) -> str:
    s = name.lower().strip()
    for ch in [" ", "_", ".", "–", "—", "/"]:
        s = s.replace(ch, "-")
    s = re.sub(r"[^a-z0-9-]", "", s)
    s = re.sub(r"-+", "-", s)
    return s.strip("-")


def vendor_dir(vendor: str) -> str:
    return slugify(vendor) or "unknown-vendor"


def patch_toml(toml_path: Path, updates: dict) -> bool:
    """Patch a TOML file with enriched data. Returns True if changed."""
    if not toml_path.exists():
        return False

    content = toml_path.read_text()
    original = content

    # Patch description
    desc = updates.get("description", "")
    if desc:
        content = re.sub(
            r'^description = ".*"$',
            f'description = "{escape_toml(desc)}"',
            content,
            count=1,
            flags=re.MULTILINE,
        )

    # Patch homepage
    homepage = updates.get("homepage", "")
    if homepage:
        if 'homepage = "' in content:
            content = re.sub(
                r'^homepage = ".*"$',
                f'homepage = "{escape_toml(homepage)}"',
                content,
                count=1,
                flags=re.MULTILINE,
            )
        else:
            # Insert homepage before is_paid
            content = content.replace(
                'is_paid = true',
                f'homepage = "{escape_toml(homepage)}"\nis_paid = true',
            )

    # Patch version
    version = updates.get("version", "")
    if version and version != "1.0.0":
        content = re.sub(
            r'^version = ".*"$',
            f'version = "{escape_toml(version)}"',
            content,
            count=1,
            flags=re.MULTILINE,
        )

    if content != original:
        toml_path.write_text(content)
        return True
    return False


def main():
    enriched_files = sorted(ENRICH_DIR.glob("*-enriched.json"))
    if not enriched_files:
        print("No enriched files found in /tmp/apm-enrich/", file=sys.stderr)
        return

    total_updated = 0
    total_skipped = 0
    total_missing = 0

    for ef in enriched_files:
        vendor = ef.stem.replace("-enriched", "")
        print(f"\n{vendor}:", file=sys.stderr)

        try:
            with open(ef) as f:
                entries = json.load(f)
        except (json.JSONDecodeError, IOError) as e:
            print(f"  ERROR reading {ef}: {e}", file=sys.stderr)
            continue

        updated = 0
        skipped = 0
        missing = 0

        for entry in entries:
            slug = entry.get("slug", "")
            if not slug:
                skipped += 1
                continue

            vendor = entry.get("vendor", vendor.replace("-", " ").title())
            toml_path = PLUGINS_DIR / vendor_dir(vendor) / f"{slug}.toml"
            if not toml_path.exists():
                missing += 1
                continue

            if patch_toml(toml_path, entry):
                updated += 1
            else:
                skipped += 1

        print(f"  {updated} updated, {skipped} unchanged, {missing} missing", file=sys.stderr)
        total_updated += updated
        total_skipped += skipped
        total_missing += missing

    print(f"\n{'='*50}", file=sys.stderr)
    print(f"Total: {total_updated} updated, {total_skipped} unchanged, {total_missing} missing", file=sys.stderr)


if __name__ == "__main__":
    main()
