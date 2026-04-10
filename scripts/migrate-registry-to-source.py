#!/usr/bin/env python3
"""
Migrate the current generated registry/ tree into the normalized registry-src/
authoring model.

Usage:
    python3 scripts/migrate-registry-to-source.py
"""

from __future__ import annotations

import shutil
import tomllib
from pathlib import Path
import re


ROOT = Path(__file__).resolve().parent.parent
REGISTRY_DIR = ROOT / "registry"
SOURCE_DIR = ROOT / "registry-src"


def toml_string(value: str) -> str:
    escaped = value.replace("\\", "\\\\").replace('"', '\\"')
    return f'"{escaped}"'


def toml_value(value) -> str:
    if value is None:
        raise ValueError("None is not a valid TOML scalar here")
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, int):
        return str(value)
    if isinstance(value, str):
        return toml_string(value)
    if isinstance(value, list):
        rendered = ", ".join(toml_value(item) for item in value)
        return f"[{rendered}]"
    raise TypeError(f"Unsupported TOML value: {type(value)!r}")


def write_table_file(path: Path, scalar_fields: list[tuple[str, object]], tables: list[tuple[str, dict]]) -> None:
    lines: list[str] = []
    for key, value in scalar_fields:
        if value is None:
            continue
        lines.append(f"{key} = {toml_value(value)}")

    for table_name, values in tables:
        lines.append("")
        lines.append(f"[{table_name}]")
        for key, value in values.items():
            if value is None:
                continue
            lines.append(f"{key} = {toml_value(value)}")

    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines) + "\n")


def parse_toml_table_order(path: Path) -> list[str]:
    order: list[str] = []
    pattern = re.compile(r"^\[([^\]]+)\]\s*$")
    for line in path.read_text().splitlines():
        match = pattern.match(line.strip())
        if match:
            order.append(match.group(1))
    return order


def extract_plugin_comment_block(text: str) -> list[str]:
    lines = text.splitlines()
    comments: list[str] = []
    collecting = False

    for line in lines:
        stripped = line.strip()
        if stripped.startswith("[formats.") or stripped.startswith("[[releases]]"):
            break
        if stripped.startswith("#"):
            collecting = True
            comments.append(stripped)
            continue
        if collecting and stripped == "":
            comments.append("")
            continue
        if collecting:
            comments = []
            collecting = False

    while comments and comments[-1] == "":
        comments.pop()
    return comments


def render_plugin_source(data: dict, vendor_id: str, comment_lines: list[str]) -> str:
    scalar_fields = [
        ("slug", data["slug"]),
        ("name", data["name"]),
        ("vendor", vendor_id),
        ("version", data["version"]),
        ("description", data["description"]),
        ("category", data["category"]),
        ("subcategory", data.get("subcategory")),
        ("license", data["license"]),
        ("tags", data.get("tags", [])),
        ("installer", data.get("installer")),
        ("homepage", data.get("homepage")),
        ("purchase_url", data.get("purchase_url")),
        ("bundle_ids", data.get("bundle_ids") if "bundle_ids" in data else None),
        ("is_paid", data.get("is_paid") if "is_paid" in data else None),
        ("price_cents", data.get("price_cents") if "price_cents" in data else None),
        ("currency", data.get("currency") if "currency" in data else None),
    ]

    lines: list[str] = []
    for key, value in scalar_fields:
        if value is None:
            continue
        lines.append(f"{key} = {toml_value(value)}")

    if comment_lines:
        lines.append("")
        lines.extend(comment_lines)

    for fmt, fmt_data in data.get("formats", {}).items():
        lines.append("")
        lines.append(f"[formats.{fmt}]")
        for key, value in fmt_data.items():
            if value is not None:
                lines.append(f"{key} = {toml_value(value)}")

    for release in data.get("releases", []):
        lines.append("")
        lines.append("[[releases]]")
        lines.append(f"version = {toml_value(release['version'])}")
        for fmt, fmt_data in release.get("formats", {}).items():
            lines.append("")
            lines.append(f"[releases.formats.{fmt}]")
            for key, value in fmt_data.items():
                if value is not None:
                    lines.append(f"{key} = {toml_value(value)}")

    return "\n".join(lines) + "\n"


def main() -> None:
    if SOURCE_DIR.exists():
        shutil.rmtree(SOURCE_DIR)

    (SOURCE_DIR / "vendors").mkdir(parents=True, exist_ok=True)
    (SOURCE_DIR / "installers").mkdir(parents=True, exist_ok=True)
    (SOURCE_DIR / "bundles").mkdir(parents=True, exist_ok=True)
    (SOURCE_DIR / "plugins").mkdir(parents=True, exist_ok=True)

    vendor_map: dict[str, str] = {}

    plugin_files = sorted((REGISTRY_DIR / "plugins").glob("*/*.toml"))
    for plugin_path in plugin_files:
        vendor_id = plugin_path.parent.name
        raw_text = plugin_path.read_text()
        data = tomllib.loads(raw_text)
        vendor_name = data["vendor"]
        vendor_map.setdefault(vendor_id, vendor_name)
        comment_lines = extract_plugin_comment_block(raw_text)
        target = SOURCE_DIR / "plugins" / vendor_id / plugin_path.name
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(render_plugin_source(data, vendor_id, comment_lines))

    for vendor_id, vendor_name in sorted(vendor_map.items()):
        write_table_file(
            SOURCE_DIR / "vendors" / f"{vendor_id}.toml",
            [("id", vendor_id), ("name", vendor_name), ("aliases", [])],
            [],
        )

    installer_order = parse_toml_table_order(REGISTRY_DIR / "installers.toml")
    installers = tomllib.loads((REGISTRY_DIR / "installers.toml").read_text())
    for installer_id in installer_order:
        installer = installers[installer_id]
        vendor_name = installer["vendor"]
        vendor_id = next((vid for vid, name in vendor_map.items() if name == vendor_name), None)
        if vendor_id is None:
            vendor_id = installer_id if installer_id in vendor_map else installer_id.rsplit("-", 1)[0]
            vendor_map.setdefault(vendor_id, vendor_name)
            vendor_file = SOURCE_DIR / "vendors" / f"{vendor_id}.toml"
            if not vendor_file.exists():
                write_table_file(
                    vendor_file,
                    [("id", vendor_id), ("name", vendor_name), ("aliases", [])],
                    [],
                )

        write_table_file(
            SOURCE_DIR / "installers" / f"{installer_id}.toml",
            [
                ("id", installer_id),
                ("name", installer["name"]),
                ("vendor", vendor_id),
                ("app_paths", installer.get("app_paths", [])),
                ("download_url", installer["download_url"]),
                ("homepage", installer["homepage"]),
            ],
            [],
        )

    for bundle_path in sorted((REGISTRY_DIR / "bundles").glob("*.toml")):
        data = tomllib.loads(bundle_path.read_text())
        write_table_file(
            SOURCE_DIR / "bundles" / bundle_path.name,
            [
                ("slug", data["slug"]),
                ("name", data["name"]),
                ("description", data["description"]),
                ("plugins", data.get("plugins", [])),
            ],
            [],
        )

    manifest_lines = [
        "schema_version = 1",
        'generated_dir = "registry"',
        "",
        f"installer_order = {toml_value(installer_order)}",
        "",
    ]
    (SOURCE_DIR / "manifest.toml").write_text("\n".join(manifest_lines))

    print(f"Migrated {len(plugin_files)} plugins into {SOURCE_DIR}")


if __name__ == "__main__":
    main()
