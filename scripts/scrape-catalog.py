#!/usr/bin/env python3
"""
Scrape the musicsoftwaredeals.com WordPress REST API to build a plugin catalog.

Downloads deals, manufacturers, categories, and source websites, then outputs
a JSON catalog file that can be converted to registry TOML files.

Usage:
    python3 scripts/scrape-catalog.py [--category effects,instruments] [--limit 1000]
"""

import argparse
import json
import os
import sys
import time
import urllib.request
import urllib.error
import urllib.parse
from pathlib import Path

API_BASE = "https://musicsoftwaredeals.com/wp-json/wp/v2"
PER_PAGE = 100  # WP REST API max
OUTPUT_DIR = Path(__file__).parent.parent / "data"


def fetch_json(url: str, retries: int = 3) -> dict | list:
    """Fetch JSON from a URL with retry logic."""
    for attempt in range(retries):
        try:
            req = urllib.request.Request(url, headers={"User-Agent": "apm-catalog-builder/0.1"})
            with urllib.request.urlopen(req, timeout=30) as resp:
                return json.loads(resp.read().decode())
        except (urllib.error.URLError, urllib.error.HTTPError, TimeoutError) as e:
            if attempt < retries - 1:
                wait = 2 ** attempt
                print(f"  Retry {attempt + 1}/{retries} in {wait}s: {e}", file=sys.stderr)
                time.sleep(wait)
            else:
                raise


def fetch_total(endpoint: str) -> int:
    """Get total item count from WP API headers."""
    url = f"{API_BASE}/{endpoint}?per_page=1"
    req = urllib.request.Request(url, headers={"User-Agent": "apm-catalog-builder/0.1"})
    with urllib.request.urlopen(req, timeout=30) as resp:
        return int(resp.headers.get("X-WP-Total", 0))


def fetch_all_pages(endpoint: str, params: dict | None = None, limit: int | None = None) -> list:
    """Fetch all pages of a paginated WP REST API endpoint."""
    items = []
    page = 1
    params = params or {}

    total = fetch_total(endpoint)
    print(f"  Total: {total} items", file=sys.stderr)

    while True:
        query = urllib.parse.urlencode({**params, "per_page": PER_PAGE, "page": page})
        url = f"{API_BASE}/{endpoint}?{query}"
        print(f"  Page {page} ({len(items)} fetched)...", file=sys.stderr, end="\r")

        try:
            batch = fetch_json(url)
        except urllib.error.HTTPError as e:
            if e.code == 400:
                # Past the last page
                break
            raise

        if not batch:
            break

        items.extend(batch)
        page += 1

        if limit and len(items) >= limit:
            items = items[:limit]
            break

        # Be polite
        time.sleep(0.25)

    print(f"  Fetched {len(items)} items.            ", file=sys.stderr)
    return items


def fetch_taxonomy(name: str) -> dict[int, dict]:
    """Fetch a taxonomy and return as {id: {name, slug, count, parent}}."""
    print(f"\nFetching {name}...", file=sys.stderr)
    items = fetch_all_pages(name)
    return {
        item["id"]: {
            "name": item["name"],
            "slug": item["slug"],
            "count": item.get("count", 0),
            "parent": item.get("parent", 0),
        }
        for item in items
    }


def build_category_tree(categories: dict[int, dict]) -> dict[int, str]:
    """Build full category paths like 'effects / reverb'."""
    paths = {}
    for cat_id, cat in categories.items():
        parts = [cat["name"]]
        parent_id = cat["parent"]
        while parent_id and parent_id in categories:
            parts.insert(0, categories[parent_id]["name"])
            parent_id = categories[parent_id]["parent"]
        paths[cat_id] = " / ".join(parts)
    return paths


def slugify(name: str) -> str:
    """Convert a plugin name to a registry slug."""
    slug = name.lower().strip()
    # Replace common separators with hyphens
    for ch in [" ", "_", ".", "–", "—"]:
        slug = slug.replace(ch, "-")
    # Remove non-alphanumeric (keep hyphens)
    slug = "".join(c for c in slug if c.isalnum() or c == "-")
    # Collapse multiple hyphens
    while "--" in slug:
        slug = slug.replace("--", "-")
    return slug.strip("-")


def deal_to_plugin(deal: dict, manufacturers: dict, categories: dict, cat_paths: dict) -> dict | None:
    """Convert a WP deal to a plugin catalog entry."""
    title = deal.get("title", {}).get("rendered", "").strip()
    if not title:
        return None

    # Resolve manufacturer
    mfr_ids = deal.get("manufacturers", [])
    vendor = "Unknown"
    if mfr_ids and mfr_ids[0] in manufacturers:
        vendor = manufacturers[mfr_ids[0]]["name"]

    # Resolve categories
    cat_ids = deal.get("categories", [])
    category = ""
    subcategory = ""
    for cid in cat_ids:
        if cid in cat_paths:
            path = cat_paths[cid]
            parts = path.split(" / ")
            if len(parts) >= 1:
                category = parts[0].lower()
            if len(parts) >= 2:
                subcategory = parts[-1].lower()
            break

    return {
        "slug": slugify(f"{title}-by-{vendor}" if vendor != "Unknown" else title),
        "name": title,
        "vendor": vendor,
        "category": category or "uncategorized",
        "subcategory": subcategory or None,
        "source_url": deal.get("link", ""),
        "wp_id": deal["id"],
    }


def main():
    parser = argparse.ArgumentParser(description="Scrape musicsoftwaredeals.com catalog")
    parser.add_argument(
        "--categories",
        help="Comma-separated category slugs to filter (e.g. effects,instruments)",
    )
    parser.add_argument(
        "--limit", type=int, help="Max number of deals to fetch"
    )
    parser.add_argument(
        "--output", default=str(OUTPUT_DIR / "msd-catalog.json"),
        help="Output JSON file path",
    )
    args = parser.parse_args()

    # Fetch taxonomies first
    manufacturers = fetch_taxonomy("manufacturers")
    categories = fetch_taxonomy("categories")
    cat_paths = build_category_tree(categories)

    # Resolve category filter to IDs
    deal_params = {}
    if args.categories:
        filter_slugs = [s.strip() for s in args.categories.split(",")]
        filter_ids = [
            cid for cid, cat in categories.items()
            if cat["slug"] in filter_slugs
        ]
        if filter_ids:
            deal_params["categories"] = ",".join(str(i) for i in filter_ids)
            print(f"\nFiltering to categories: {filter_slugs} (IDs: {filter_ids})", file=sys.stderr)
        else:
            print(f"\nWarning: no matching categories for {filter_slugs}", file=sys.stderr)

    # Fetch deals
    print("\nFetching deals...", file=sys.stderr)
    deals = fetch_all_pages(
        "deals",
        params={**deal_params, "_fields": "id,title,slug,manufacturers,categories,link"},
        limit=args.limit,
    )

    # Convert to catalog entries
    plugins = []
    seen_slugs = set()
    for deal in deals:
        entry = deal_to_plugin(deal, manufacturers, categories, cat_paths)
        if entry and entry["slug"] not in seen_slugs:
            plugins.append(entry)
            seen_slugs.add(entry["slug"])

    # Build output
    catalog = {
        "scraped_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "source": "musicsoftwaredeals.com",
        "total_deals": len(deals),
        "unique_plugins": len(plugins),
        "manufacturers": {
            str(k): v for k, v in manufacturers.items()
        },
        "categories": {
            str(k): {**v, "path": cat_paths.get(k, v["name"])}
            for k, v in categories.items()
        },
        "plugins": plugins,
    }

    # Write output
    os.makedirs(os.path.dirname(args.output), exist_ok=True)
    with open(args.output, "w") as f:
        json.dump(catalog, f, indent=2, ensure_ascii=False)

    print(f"\nDone! {len(plugins)} unique plugins from {len(deals)} deals.", file=sys.stderr)
    print(f"Output: {args.output}", file=sys.stderr)
    print(f"Manufacturers: {len(manufacturers)}", file=sys.stderr)
    print(f"Categories: {len(categories)}", file=sys.stderr)


if __name__ == "__main__":
    main()
