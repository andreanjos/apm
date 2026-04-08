#!/usr/bin/env python3
"""
Scrape Plugin Boutique manufacturer pages to build a product catalog.

Uses Playwright to handle JS-rendered pages. Extracts product URLs,
names, categories, and prices from manufacturer listing pages.

Usage:
    python3 scripts/scrape-pluginboutique.py [--limit 10]
"""

import argparse
import json
import os
import re
import sys
import time
from pathlib import Path

from playwright.sync_api import sync_playwright

OUTPUT = Path(__file__).parent.parent / "data" / "pb-catalog.json"
SITEMAP_URL = "https://www.pluginboutique.com/sitemap.xml"


def extract_manufacturer_urls(page) -> list[dict]:
    """Fetch the sitemap and extract all manufacturer page URLs."""
    import urllib.request
    req = urllib.request.Request(SITEMAP_URL, headers={"User-Agent": "apm-catalog-builder/0.1"})
    with urllib.request.urlopen(req, timeout=30) as resp:
        xml = resp.read().decode()

    urls = []
    for match in re.finditer(r'<loc>(https://www\.pluginboutique\.com/manufacturers/(\d+)-([^<]+))</loc>', xml):
        urls.append({
            "url": match.group(1),
            "id": int(match.group(2)),
            "slug": match.group(3),
        })
    return urls


def scrape_manufacturer_page(page, url: str, mfr_slug: str) -> list[dict]:
    """Scrape a manufacturer page for product listings."""
    try:
        page.goto(url, wait_until="load", timeout=30000)
        page.wait_for_timeout(6000)
    except Exception as e:
        print(f"    ERROR loading page: {e}", file=sys.stderr)
        return []

    # Scroll to trigger lazy loading
    for _ in range(3):
        page.evaluate("window.scrollTo(0, document.body.scrollHeight)")
        page.wait_for_timeout(1500)

    content = page.content()

    # Extract product URLs and names from href patterns
    products = []
    seen = set()

    # Match /product/CAT_ID-CAT/SUBCAT_ID-SUBCAT/PROD_ID-NAME
    for match in re.finditer(
        r'/product/(\d+)-([^/]+)/(\d+)-([^/]+)/(\d+)-([^"\'&?]+)',
        content,
    ):
        cat_id, cat_slug = match.group(1), match.group(2)
        subcat_id, subcat_slug = match.group(3), match.group(4)
        prod_id, prod_slug = match.group(5), match.group(6)

        if prod_id in seen:
            continue
        seen.add(prod_id)

        # Clean the product name from the slug
        name = prod_slug.replace("-", " ").strip()
        # Remove trailing truncation artifacts
        name = re.sub(r'\s+$', '', name)

        products.append({
            "pb_id": int(prod_id),
            "name": name,
            "slug": prod_slug.lower(),
            "category": cat_slug.replace("-", " "),
            "subcategory": subcat_slug.replace("-", " "),
            "url": f"/product/{cat_id}-{cat_slug}/{subcat_id}-{subcat_slug}/{prod_id}-{prod_slug}",
            "manufacturer": mfr_slug.replace("-", " "),
        })

    # Also match /meta_product/ URLs (upgrades, crossgrades)
    for match in re.finditer(
        r'/meta_product/(\d+)-([^/]+)/(\d+)-([^/]+)/(\d+)-([^"\'&?]+)',
        content,
    ):
        prod_id = match.group(5)
        if prod_id not in seen:
            seen.add(prod_id)
            prod_slug = match.group(6)
            products.append({
                "pb_id": int(prod_id),
                "name": prod_slug.replace("-", " ").strip(),
                "slug": prod_slug.lower(),
                "category": match.group(2).replace("-", " "),
                "subcategory": match.group(4).replace("-", " "),
                "url": f"/meta_product/{match.group(1)}-{match.group(2)}/{match.group(3)}-{match.group(4)}/{prod_id}-{prod_slug}",
                "manufacturer": mfr_slug.replace("-", " "),
                "is_upgrade": True,
            })

    return products


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--limit", type=int, help="Max manufacturers to scrape")
    args = parser.parse_args()

    # Get manufacturer URLs from sitemap
    print("Fetching sitemap...", file=sys.stderr)
    manufacturers = extract_manufacturer_urls(None)
    print(f"Found {len(manufacturers)} manufacturer pages", file=sys.stderr)

    if args.limit:
        manufacturers = manufacturers[:args.limit]
        print(f"Limited to {args.limit} manufacturers", file=sys.stderr)

    all_products = []
    failed = []

    with sync_playwright() as p:
        browser = p.chromium.launch(headless=True)
        page = browser.new_page()

        for i, mfr in enumerate(manufacturers):
            pct = (i + 1) / len(manufacturers) * 100
            print(
                f"  [{i+1}/{len(manufacturers)} {pct:.0f}%] {mfr['slug']}...",
                file=sys.stderr,
                end="",
            )

            try:
                products = scrape_manufacturer_page(page, mfr["url"], mfr["slug"])
                all_products.extend(products)
                print(f" {len(products)} products", file=sys.stderr)
            except Exception as e:
                print(f" FAILED: {e}", file=sys.stderr)
                failed.append(mfr["slug"])

            # Be polite - 1s between pages
            time.sleep(1)

        browser.close()

    # Deduplicate by pb_id
    seen_ids = set()
    unique = []
    for prod in all_products:
        if prod["pb_id"] not in seen_ids:
            seen_ids.add(prod["pb_id"])
            unique.append(prod)

    catalog = {
        "scraped_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "source": "pluginboutique.com",
        "manufacturers_scraped": len(manufacturers),
        "manufacturers_failed": failed,
        "total_products": len(unique),
        "products": unique,
    }

    os.makedirs(os.path.dirname(OUTPUT), exist_ok=True)
    with open(OUTPUT, "w") as f:
        json.dump(catalog, f, indent=2, ensure_ascii=False)

    print(f"\nDone! {len(unique)} unique products from {len(manufacturers)} manufacturers.", file=sys.stderr)
    print(f"Failed: {len(failed)} manufacturers", file=sys.stderr)
    print(f"Output: {OUTPUT}", file=sys.stderr)


if __name__ == "__main__":
    main()
