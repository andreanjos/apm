#!/usr/bin/env python3
"""
Build the generated registry/ compatibility tree from the normalized
registry-src/ authoring model.

Usage:
    python3 scripts/build-registry.py
"""

from __future__ import annotations

import shutil
import tempfile
import time
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
SOURCE_DIR = ROOT / "registry-src"
OUTPUT_DIR = ROOT / "registry"


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


def toml_list_multiline(values: list[object], indent: int = 2) -> str:
    lines = ["["]
    prefix = " " * indent
    for value in values:
        lines.append(f"{prefix}{toml_value(value)},")
    lines.append("]")
    return "\n".join(lines)


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
            if value is not None:
                lines.append(f"{key} = {toml_value(value)}")

    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines) + "\n")


def render_plugin_file(plugin: dict, vendor_name: str) -> str:
    scalar_fields = [
        ("slug", plugin["slug"]),
        ("name", plugin["name"]),
        ("vendor", vendor_name),
        ("version", plugin["version"]),
        ("description", plugin["description"]),
        ("category", plugin["category"]),
        ("subcategory", plugin.get("subcategory")),
        ("license", plugin["license"]),
        ("tags", plugin.get("tags", [])),
        ("installer", plugin.get("installer")),
        ("homepage", plugin.get("homepage")),
        ("purchase_url", plugin.get("purchase_url")),
        ("bundle_ids", plugin.get("bundle_ids") or None),
        ("is_paid", True if plugin.get("is_paid") else None),
        ("price_cents", plugin.get("price_cents")),
        ("currency", plugin.get("currency")),
    ]

    kept_scalars = [(key, value) for key, value in scalar_fields if value is not None]
    scalar_width = 11
    table_width = 12

    lines: list[str] = []
    for key, value in kept_scalars:
        lines.append(f"{key.ljust(scalar_width)} = {toml_value(value)}")

    comment_lines = plugin.get("_comment_lines", [])
    if comment_lines:
        lines.extend(comment_lines)

    for fmt, fmt_data in plugin.get("formats", {}).items():
        lines.append("")
        lines.append(f"[formats.{fmt}]")
        for key, value in fmt_data.items():
            lines.append(f"{key.ljust(table_width)} = {toml_value(value)}")

    for release in plugin.get("releases", []):
        lines.append("")
        lines.append("[[releases]]")
        lines.append(f"version = {toml_value(release['version'])}")
        for fmt, fmt_data in release.get("formats", {}).items():
            lines.append("")
            lines.append(f"[releases.formats.{fmt}]")
            for key, value in fmt_data.items():
                lines.append(f"{key.ljust(table_width)} = {toml_value(value)}")

    return "\n".join(lines) + "\n"


def render_installers_file(installers_in_order: list[tuple[str, dict]], vendors: dict[str, dict]) -> str:
    lines: list[str] = []
    for index, (installer_id, installer) in enumerate(installers_in_order):
        vendor = vendors.get(installer["vendor"])
        if vendor is None:
            raise SystemExit(f"Unknown vendor id '{installer['vendor']}' for installer {installer_id}")
        if index > 0:
            lines.append("")
        lines.append(f"[{installer_id}]")
        lines.append(f"name = {toml_string(installer['name'])}")
        lines.append(f"vendor = {toml_string(vendor['name'])}")
        app_paths = installer.get("app_paths", [])
        if len(app_paths) > 1:
            lines.append(f"app_paths = {toml_list_multiline(app_paths)}")
        else:
            lines.append(f"app_paths = {toml_value(app_paths)}")
        lines.append(f"download_url = {toml_string(installer['download_url'])}")
        lines.append(f"homepage = {toml_string(installer['homepage'])}")
    return "\n".join(lines) + "\n"


def render_bundle_file(bundle: dict) -> str:
    lines = [
        f"slug = {toml_string(bundle['slug'])}",
        f"name = {toml_string(bundle['name'])}",
        f"description = {toml_string(bundle['description'])}",
        f"plugins = {toml_list_multiline(bundle.get('plugins', []), indent=4)}",
    ]
    return "\n".join(lines) + "\n"


def load_toml_files(directory: Path) -> list[dict]:
    return [tomllib.loads(path.read_text()) for path in sorted(directory.glob("*.toml"))]


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


def main() -> None:
    manifest = tomllib.loads((SOURCE_DIR / "manifest.toml").read_text())
    schema_version = manifest["schema_version"]
    installer_order = manifest.get("installer_order", [])

    vendors = {data["id"]: data for data in load_toml_files(SOURCE_DIR / "vendors")}
    installers = {data["id"]: data for data in load_toml_files(SOURCE_DIR / "installers")}
    bundles = load_toml_files(SOURCE_DIR / "bundles")

    plugins: list[dict] = []
    for plugin_path in sorted((SOURCE_DIR / "plugins").glob("*/*.toml")):
        if plugin_path.parent.name == plugin_path.stem:
            continue
        raw_text = plugin_path.read_text()
        data = tomllib.loads(raw_text)
        data["_vendor_id"] = data["vendor"]
        data["_vendor_dir"] = plugin_path.parent.name
        data["_comment_lines"] = extract_plugin_comment_block(raw_text)
        plugins.append(data)

    temp_root = Path(tempfile.mkdtemp(prefix="apm-registry-build-", dir=ROOT))
    build_root = temp_root / "registry"
    (build_root / "plugins").mkdir(parents=True, exist_ok=True)
    (build_root / "bundles").mkdir(parents=True, exist_ok=True)

    for plugin in plugins:
        vendor_id = plugin["_vendor_id"]
        vendor = vendors.get(vendor_id)
        if vendor is None:
            raise SystemExit(f"Unknown vendor id '{vendor_id}' for plugin {plugin['slug']}")

        installer_key = plugin.get("installer")
        if installer_key is not None and installer_key not in installers:
            raise SystemExit(f"Unknown installer '{installer_key}' for plugin {plugin['slug']}")

        path = build_root / "plugins" / plugin["_vendor_dir"] / f"{plugin['slug']}.toml"
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(render_plugin_file(plugin, vendor["name"]))

    seen_installers: set[str] = set()
    ordered_installers: list[tuple[str, dict]] = []
    for installer_id in installer_order:
        installer = installers.get(installer_id)
        if installer is None:
            raise SystemExit(f"Manifest references unknown installer '{installer_id}'")
        ordered_installers.append((installer_id, installer))
        seen_installers.add(installer_id)
    for installer_id in sorted(installers):
        if installer_id not in seen_installers:
            ordered_installers.append((installer_id, installers[installer_id]))
    (build_root / "installers.toml").write_text(render_installers_file(ordered_installers, vendors))

    for bundle in bundles:
        (build_root / "bundles" / f"{bundle['slug']}.toml").write_text(render_bundle_file(bundle))

    generated = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
    index_lines = [
        f"version = {schema_version}",
        f'generated = "{generated}"',
        "",
    ]
    for plugin in sorted(plugins, key=lambda item: item["slug"]):
        index_lines.extend(
            [
                "[[plugins]]",
                f'name = "{plugin["slug"]}"',
                f'path = "plugins/{plugin["_vendor_dir"]}/{plugin["slug"]}.toml"',
                f'version = "{plugin["version"]}"',
                "",
            ]
        )
    (build_root / "index.toml").write_text("\n".join(index_lines))

    backup_root = OUTPUT_DIR.with_name(f"{OUTPUT_DIR.name}.bak")
    if backup_root.exists():
        shutil.rmtree(backup_root)
    if OUTPUT_DIR.exists():
        OUTPUT_DIR.replace(backup_root)
    build_root.replace(OUTPUT_DIR)
    if backup_root.exists():
        shutil.rmtree(backup_root)
    shutil.rmtree(temp_root, ignore_errors=True)

    print(
        f"Built registry from registry-src: {len(plugins)} plugins, "
        f"{len(installers)} installers, {len(bundles)} bundles"
    )


if __name__ == "__main__":
    main()
