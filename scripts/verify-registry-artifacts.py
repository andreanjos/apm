#!/usr/bin/env python3
"""Verify that every machine-fetchable registry artifact matches its metadata.

The plugin registry (`registry/plugins/<vendor>/<slug>.toml`) declares, per
format, a download URL plus an `install_type`, a `bundle_path` and a
`download_type`.  Those fields are only useful when they describe the artifact
that the URL actually serves.  This script re-derives them from the bytes.

What it does, per format entry:

  1. Probes the URL over HTTP (follows redirects; records status,
     content-type, content-length and any `Content-Disposition` filename).
  2. Reads the first bytes (bounded ranged request) to identify the real
     container from its magic (`PK\\x03\\x04` zip, `xar!` pkg, `koly` dmg
     footer, HTML, gzip, ...).
  3. For ZIPs under `--max-bytes`, downloads the whole archive and lists its
     members; for ZIPs over the cap it reads only the tail and parses the
     central directory, which still yields the complete member list.
  4. For PKGs under the cap, parses the xar TOC and the cpio payload to derive
     the bundle names the package installs.
  5. Classifies the artifact and compares the result with the registry fields.

Guarantees / non-goals:

  * Never installs anything, never runs sudo, never mounts a DMG, never writes
    outside the repository (`--cache-dir`, `--out-json`, `--out-report`).
  * Bounded: per-request timeout, per-artifact size cap, modest concurrency.
  * Idempotent: results are deterministic and cached, so a re-run reuses the
    cached bytes and produces the same report.

Usage:

    python3 scripts/verify-registry-artifacts.py                     # direct+managed
    python3 scripts/verify-registry-artifacts.py --scope all         # + manual probe
    python3 scripts/verify-registry-artifacts.py --scope direct --limit 20
    python3 scripts/verify-registry-artifacts.py --apply             # write corrections

Outputs:

    data/registry-verification.json               machine-readable per-entry result
    docs/registry-verification.md                 human-readable report
"""

from __future__ import annotations

import argparse
import bz2
import collections
import concurrent.futures
import dataclasses
import datetime as _dt
import gzip
import hashlib
import io
import json
import lzma
import os
import pathlib
import re
import struct
import sys
import threading
import time
import tomllib
import urllib.error
import urllib.parse
import urllib.request
import zipfile
import zlib

TOOL_VERSION = "1.0.0"
USER_AGENT = "apm-registry-artifact-verifier/1.0 (+https://github.com/andreanjos/apm)"

HEAD_BYTES = 64 * 1024
TAIL_BYTES = 4 * 1024 * 1024
MAX_MEMBERS = 20000
MAX_MEMBERS_IN_REPORT = 60

# Status codes worth another try (rate limits and transient server errors).
RETRY_CODES = {408, 425, 429, 500, 502, 503, 504}
# Status codes whose HEAD answer is confirmed with a ranged GET first, because
# some hosts reject HEAD or block it while serving the artifact to GET.
CONFIRM_CODES = {403, 404, 405, 410, 429, 501}
MAX_RETRY_SLEEP = 15.0

# ── Classification taxonomy ───────────────────────────────────────────────────

ZIP_WITH_BUNDLES = "zip-with-bundles"
ZIP_WITH_PKG = "zip-with-pkg"
ZIP_WITH_DMG = "zip-with-dmg"
ZIP_MISMATCH = "zip-mismatch"
DMG = "dmg"
PKG = "pkg"
WEBPAGE = "webpage"
NOT_FOUND = "not-found"
UNREACHABLE = "unreachable"

ALL_CLASSES = [
    ZIP_WITH_BUNDLES,
    ZIP_WITH_PKG,
    ZIP_WITH_DMG,
    ZIP_MISMATCH,
    DMG,
    PKG,
    WEBPAGE,
    NOT_FOUND,
    UNREACHABLE,
]

# format key in the registry -> bundle extension
FORMAT_EXT = {"vst3": ".vst3", "au": ".component", "app": ".app"}
# every extension that marks an audio-plugin bundle inside an archive
PLUGIN_EXTS = {".vst3", ".component", ".app", ".clap", ".aaxplugin", ".vst"}

SCOPE_ORDER = ["direct", "managed", "manual"]


# ── Data model ────────────────────────────────────────────────────────────────


@dataclasses.dataclass
class Entry:
    """One `[formats.<fmt>]` record (top level or inside a release)."""

    file: str
    slug: str
    vendor: str
    locator: str  # "formats" or "releases[<version>]"
    version: str | None
    fmt: str
    url: str
    sha256: str
    install_type: str
    bundle_path: str
    download_type: str

    @property
    def entry_id(self) -> str:
        return f"{self.slug}@{self.version}:{self.fmt}" if self.version else f"{self.slug}:{self.fmt}"


@dataclasses.dataclass
class Probe:
    status: int | None = None
    content_type: str = ""
    content_length: int | None = None
    final_url: str = ""
    disposition_filename: str | None = None
    method: str = ""
    error: str | None = None


@dataclasses.dataclass
class Artifact:
    size_bytes: int | None = None
    container: str = "unknown"  # zip | dmg | pkg | html | gzip | tar | unknown
    container_evidence: str = "none"  # magic | content-type | url | none
    method: str = "none"  # download | tail-cd | magic-only | pkg-toc
    listing_complete: bool = False
    members: list[str] = dataclasses.field(default_factory=list)
    members_truncated: bool = False
    bundles: dict[str, list[str]] = dataclasses.field(default_factory=dict)
    nested_pkgs: list[str] = dataclasses.field(default_factory=list)
    nested_dmgs: list[str] = dataclasses.field(default_factory=list)
    pkg_payload_bundles: dict[str, list[str]] = dataclasses.field(default_factory=dict)
    pkg_payload_error: str | None = None
    pkg_payload_listed: bool = False
    pkg_components: list[str] = dataclasses.field(default_factory=list)
    sha256: str | None = None
    notes: list[str] = dataclasses.field(default_factory=list)


# ── HTTP helpers ──────────────────────────────────────────────────────────────


def _request(url: str, headers: dict[str, str], timeout: float, method: str = "GET"):
    req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT, **headers}, method=method)
    return urllib.request.urlopen(req, timeout=timeout)


def _read_bounded(fp, limit: int) -> tuple[bytes, bool]:
    """Read at most `limit` bytes; returns (data, truncated)."""
    data = fp.read(limit)
    if len(data) < limit:
        return data, False
    return data, fp.read(1) != b""


def _header(headers, name: str) -> str:
    if headers is None:
        return ""
    try:
        return headers.get(name) or ""
    except Exception:  # pragma: no cover - defensive
        return ""


def _parse_content_range(value: str) -> int | None:
    m = re.search(r"bytes\s+\d+-\d+/(\d+)", value or "")
    return int(m.group(1)) if m else None


def _parse_disposition_filename(value: str) -> str | None:
    m = re.search(r"filename\*?=(?:UTF-8'')?\"?([^\";]+)", value or "", re.IGNORECASE)
    return urllib.parse.unquote(m.group(1)) if m else None


def _retry_delay(headers, attempt: int) -> float:
    retry_after = _header(headers, "Retry-After")
    if retry_after.isdigit():
        return min(float(retry_after), MAX_RETRY_SLEEP)
    return min(1.5 * (attempt + 1), MAX_RETRY_SLEEP)


def fetch_range(url: str, start: int, end: int, timeout: float, attempts: int = 3):
    """Ranged GET. Returns (status, headers, data, truncated, final_url, error).

    Rate limits and transient server errors are retried with backoff, honouring
    `Retry-After` when the host sends one.
    """
    last_error = None
    for attempt in range(attempts):
        try:
            with _request(url, {"Range": f"bytes={start}-{end}"}, timeout) as resp:
                data, truncated = _read_bounded(resp, end - start + 1)
                return resp.status, resp.headers, data, truncated, resp.url, None
        except urllib.error.HTTPError as exc:
            if exc.code in RETRY_CODES and attempt + 1 < attempts:
                time.sleep(_retry_delay(exc.headers, attempt))
                last_error = f"HTTP {exc.code}"
                continue
            body = b""
            try:
                body, _ = _read_bounded(exc, HEAD_BYTES)
            except Exception:
                pass
            return exc.code, exc.headers, body, False, getattr(exc, "url", url) or url, f"HTTP {exc.code}"
        except Exception as exc:  # URLError, timeout, TLS, ...
            last_error = f"{type(exc).__name__}: {exc}"
            if attempt + 1 < attempts:
                time.sleep(min(1.5 * (attempt + 1), MAX_RETRY_SLEEP))
    return None, None, b"", False, url, last_error


def fetch_head(url: str, timeout: float, attempts: int = 3):
    """HEAD probe; falls back to a ranged GET when HEAD is not usable."""
    last_error = None
    for attempt in range(attempts):
        try:
            with _request(url, {}, timeout, method="HEAD") as resp:
                return resp.status, resp.headers, resp.url, None, "head"
        except urllib.error.HTTPError as exc:
            if exc.code in RETRY_CODES and attempt + 1 < attempts:
                time.sleep(_retry_delay(exc.headers, attempt))
                last_error = f"HTTP {exc.code}"
                continue
            return exc.code, exc.headers, getattr(exc, "url", url) or url, f"HTTP {exc.code}", "head"
        except Exception as exc:
            last_error = f"{type(exc).__name__}: {exc}"
            if attempt + 1 < attempts:
                time.sleep(min(1.5 * (attempt + 1), MAX_RETRY_SLEEP))
    status, headers, _data, _truncated, final_url, error = fetch_range(
        url, 0, HEAD_BYTES - 1, timeout, attempts=attempts
    )
    return status, headers, final_url, error, "range"


def _fill_probe(probe: Probe, status, headers, final_url, error, method) -> None:
    probe.status = status
    probe.method = method
    if final_url:
        probe.final_url = final_url
    probe.error = error
    if headers is None:
        return
    probe.content_type = _header(headers, "Content-Type")
    probe.disposition_filename = _parse_disposition_filename(
        _header(headers, "Content-Disposition")
    )
    total = _parse_content_range(_header(headers, "Content-Range"))
    length = _header(headers, "Content-Length")
    probe.content_length = total or (int(length) if length.isdigit() else None)


def probe_url(url: str, timeout: float) -> Probe:
    probe = Probe()
    # A HEAD answer that looks like a refusal is confirmed with a ranged GET:
    # several CDNs answer 403/404/429 to HEAD but serve the artifact to GET.
    status, headers, final_url, error, method = fetch_head(url, timeout)
    if status is None or status in CONFIRM_CODES:
        get_status, get_headers, data, _, get_final, get_error = fetch_range(
            url, 0, HEAD_BYTES - 1, timeout
        )
        if get_status is not None and (status is None or get_status != status or data):
            status, headers, final_url, error, method = (
                get_status,
                get_headers,
                get_final,
                get_error,
                "range",
            )
    _fill_probe(probe, status, headers, final_url, error, method)

    # Range GET gives a true size even when the server answers 200 to HEAD with 0.
    if probe.status is not None and probe.status < 400 and probe.content_length in (None, 0):
        s2, h2, _, _, fut2, err2 = fetch_range(url, 0, HEAD_BYTES - 1, timeout)
        if s2 is not None:
            _fill_probe(probe, s2, h2, fut2, err2, "range")
    return probe


# ── Container identification ──────────────────────────────────────────────────


def detect_container(head: bytes, tail: bytes, content_type: str, url: str) -> tuple[str, str]:
    """Return (container, evidence) where evidence is magic|content-type|url."""
    if head[:4] in (b"PK\x03\x04", b"PK\x05\x06", b"PK\x07\x08"):
        return "zip", "magic"
    if head[:4] == b"xar!":
        return "pkg", "magic"
    if len(tail) >= 512 and b"koly" in tail[-512:]:
        return "dmg", "magic"
    if head[:2] == b"MZ":
        return "pe", "magic"
    lowered = head[:4096].lstrip().lower()
    if lowered.startswith(b"<!doctype html") or lowered.startswith(b"<html") or b"<head" in lowered[:512]:
        return "html", "magic"
    if content_type.lower().startswith(("text/html",)):
        return "html", "content-type"
    if content_type.lower().startswith("application/json") or lowered.startswith(b"{"):
        return "json", "magic"
    if "diskimage" in content_type.lower():
        return "dmg", "content-type"
    if url.lower().endswith(".dmg"):
        return "dmg", "url"
    if head[:2] == b"\x1f\x8b":
        return "gzip", "magic"
    if head[:3] == b"BZh":
        return "bzip2", "magic"
    if head[:6] == b"\xfd7zXZ\x00":
        return "xz", "magic"
    if head[257:262] == b"ustar":
        return "tar", "magic"
    if head[:4] in (b"\xcf\xfa\xed\xfe", b"\xce\xfa\xed\xfe"):
        return "macho", "magic"
    if content_type.lower().startswith(("text/plain",)) and _looks_like_markup(head):
        return "html", "magic"
    path = urllib.parse.urlparse(url).path.lower()
    for container, ext in (("zip", ".zip"), ("dmg", ".dmg"), ("pkg", ".pkg")):
        if path.endswith(ext):
            return "unknown-" + container, "url"
    if len(tail) >= 2048 and b"koly" in tail[-2048:]:
        return "dmg", "magic"
    return "unknown", "none"


def list_zip_local_headers(head: bytes) -> tuple[list[str], bool]:
    """Best-effort member listing from ZIP local file headers.

    Used when the archive is too large to download and the server ignores
    range requests, so the central directory is unreachable. Local headers
    carry the name and (for non-streaming entries) the sizes needed to walk
    forward; the returned listing is partial by definition.
    """
    names: list[str] = []
    cursor = 0
    seen: set[int] = set()
    while cursor + 30 <= len(head) and len(names) < MAX_MEMBERS:
        if head[cursor : cursor + 4] != b"PK\x03\x04":
            nxt = head.find(b"PK\x03\x04", cursor + 1)
            if nxt < 0:
                break
            cursor = nxt
            if cursor in seen:
                break
            seen.add(cursor)
        name_len, extra_len = struct.unpack_from("<HH", head, cursor + 26)
        compressed = struct.unpack_from("<I", head, cursor + 18)[0]
        raw = head[cursor + 30 : cursor + 30 + name_len]
        try:
            names.append(raw.decode("utf-8"))
        except UnicodeDecodeError:
            names.append(raw.decode("cp437", "replace"))
        if compressed:
            cursor += 30 + name_len + extra_len + compressed
        else:
            cursor += 30 + name_len + extra_len
    return names, True


def _looks_like_markup(head: bytes) -> bool:
    lowered = head[:2048].lstrip().lower()
    return lowered.startswith(b"<") and b">" in lowered


# ── ZIP listing ───────────────────────────────────────────────────────────────


def list_zip_bytes(blob: bytes) -> tuple[list[str], bool, str | None]:
    try:
        with zipfile.ZipFile(io.BytesIO(blob)) as archive:
            names = []
            for info in archive.infolist():
                names.append(info.filename)
                if len(names) >= MAX_MEMBERS:
                    return names, True, None
            return names, False, None
    except Exception as exc:
        return [], False, f"zipfile: {type(exc).__name__}: {exc}"


def list_zip_tail(tail: bytes, total_size: int) -> tuple[list[str], bool, str | None]:
    """Parse the ZIP central directory out of the archive tail."""
    eocd = tail.rfind(b"PK\x05\x06")
    if eocd < 0:
        return [], False, "no end-of-central-directory record in tail"
    cd_size, cd_offset = struct.unpack_from("<II", tail, eocd + 12)
    total_entries = struct.unpack_from("<H", tail, eocd + 10)[0]
    if cd_size == 0xFFFFFFFF or cd_offset == 0xFFFFFFFF or total_entries == 0xFFFF:
        locator = tail.rfind(b"PK\x06\x07", 0, eocd)
        if locator < 0:
            return [], False, "zip64 archive without locator in tail"
        zip64_offset = struct.unpack_from("<Q", tail, locator + 8)[0]
        base = total_size - len(tail)
        rel = zip64_offset - base
        if rel < 0 or rel + 56 > len(tail):
            return [], False, "zip64 end-of-central-directory outside tail window"
        if tail[rel : rel + 4] != b"PK\x06\x06":
            return [], False, "zip64 end-of-central-directory signature mismatch"
        total_entries = struct.unpack_from("<Q", tail, rel + 32)[0]
        cd_size = struct.unpack_from("<Q", tail, rel + 40)[0]
        cd_offset = struct.unpack_from("<Q", tail, rel + 48)[0]

    base = total_size - len(tail)
    rel = cd_offset - base
    if rel < 0 or rel + cd_size > len(tail):
        return [], False, "central directory outside tail window"
    names: list[str] = []
    cursor = rel
    end = rel + cd_size
    while cursor + 46 <= end:
        if tail[cursor : cursor + 4] != b"PK\x01\x02":
            return names, False, f"bad central directory signature at offset {cursor - rel}"
        name_len, extra_len, comment_len = struct.unpack_from("<HHH", tail, cursor + 28)
        raw = tail[cursor + 46 : cursor + 46 + name_len]
        try:
            names.append(raw.decode("utf-8"))
        except UnicodeDecodeError:
            names.append(raw.decode("cp437", "replace"))
        cursor += 46 + name_len + extra_len + comment_len
        if len(names) >= MAX_MEMBERS:
            return names, True, None
    return names, len(names) >= total_entries or total_entries == 0, None


def bundle_roots(members: list[str]) -> dict[str, list[str]]:
    found: dict[str, set[str]] = collections.defaultdict(set)
    for member in members:
        parts = [p for p in member.replace("\\", "/").split("/") if p not in ("", ".")]
        for index, part in enumerate(parts):
            ext = os.path.splitext(part)[1].lower()
            if ext in PLUGIN_EXTS:
                found[ext].add("/".join(parts[: index + 1]))
                break
    return {ext: sorted(paths) for ext, paths in sorted(found.items())}


def find_nested(members: list[str], extensions: tuple[str, ...]) -> list[str]:
    out = []
    for member in members:
        parts = member.split("/")
        for index, part in enumerate(parts):
            if os.path.splitext(part)[1].lower() in extensions:
                out.append("/".join(parts[: index + 1]))
                break
    return sorted(set(out))


# ── PKG (xar + cpio payload) parsing ──────────────────────────────────────────


def parse_xar_files(blob: bytes) -> tuple[list[dict], str | None]:
    """Parse a flat xar/pkg TOC.

    xar stores `<data><offset>` relative to the *heap*, which starts right
    after the compressed TOC, so every offset is rebased here.
    """
    if len(blob) < 28 or blob[:4] != b"xar!":
        return [], "not a xar archive"
    header_size, _version, toc_len_c, _toc_len_u, _cksum = struct.unpack_from(">HHQQI", blob, 4)
    if header_size + toc_len_c > len(blob):
        return [], "xar TOC truncated"
    heap_start = header_size + toc_len_c
    try:
        toc_xml = zlib.decompress(blob[header_size : header_size + toc_len_c])
    except Exception as exc:
        return [], f"xar TOC inflate failed: {exc}"
    import xml.etree.ElementTree as ET

    try:
        root = ET.fromstring(toc_xml)
    except Exception as exc:
        return [], f"xar TOC parse failed: {exc}"
    out: list[dict] = []
    for element in root.iter():
        tag = element.tag.split("}")[-1]
        if tag != "file":
            continue
        record: dict = {
            "name": None,
            "type": None,
            "offset": None,
            "length": None,
            "size": None,
            "encoding": None,
        }
        for child in element:
            ctag = child.tag.split("}")[-1]
            if ctag == "name":
                record["name"] = child.text
            elif ctag == "type":
                record["type"] = child.text
            elif ctag == "data":
                record["encoding"] = child.get("encoding") or None
                for sub in child:
                    stag = sub.tag.split("}")[-1]
                    if stag in ("offset", "length", "size"):
                        try:
                            record[stag] = int(sub.text)
                        except (TypeError, ValueError):
                            pass
        if record["offset"] is not None:
            record["offset"] += heap_start
        if record["name"]:
            out.append(record)
    return out, None


def _decompress_stream(raw: bytes, limit: int) -> tuple[bytes, str]:
    """Decompress a payload blob (gzip/bzip2/xz/zlib) with a hard size limit."""
    magic = raw[:6]
    try:
        if magic[:2] == b"\x1f\x8b":
            return gzip.decompress(raw[: limit * 4 + 4096]), "gzip"
        if magic[:3] == b"BZh":
            return bz2.decompress(raw[: limit * 4 + 4096]), "bzip2"
        if magic[:6] == b"\xfd7zXZ\x00":
            return lzma.decompress(raw[: limit * 4 + 4096]), "xz"
        if magic[:2] in (b"\x78\x01", b"\x78\x9c", b"\x78\xda"):
            return zlib.decompress(raw[: limit * 4 + 4096]), "zlib"
    except Exception as exc:
        return b"", f"decompress failed: {type(exc).__name__}: {exc}"
    return raw[:limit], "none"


def cpio_member_names(payload: bytes, limit: int = 400000) -> list[str]:
    """List member names from a cpio archive (newc '070701' or odc '070707')."""
    if payload[:6] == b"070707":
        return _cpio_odc_names(payload, limit)
    return _cpio_newc_names(payload, limit)


def _cpio_newc_names(payload: bytes, limit: int) -> list[str]:
    names: list[str] = []
    cursor = 0
    size = len(payload)
    while cursor + 110 <= size and len(names) < limit:
        magic = payload[cursor : cursor + 6]
        if magic not in (b"070701", b"070702"):
            break
        try:
            filesize = int(payload[cursor + 54 : cursor + 62], 16)
            namesize = int(payload[cursor + 94 : cursor + 102], 16)
        except ValueError:
            break
        name_bytes = payload[cursor + 110 : cursor + 110 + namesize]
        name = name_bytes.split(b"\x00", 1)[0].decode("utf-8", "replace")
        if name == "TRAILER!!!":
            break
        names.append(name)
        cursor += 110 + namesize
        cursor += (-cursor) % 4
        cursor += filesize
        cursor += (-cursor) % 4
    return names


def _cpio_odc_names(payload: bytes, limit: int) -> list[str]:
    """Old (odc) cpio format: 76-byte octal header, no padding."""
    names: list[str] = []
    cursor = 0
    size = len(payload)
    while cursor + 76 <= size and len(names) < limit:
        if payload[cursor : cursor + 6] != b"070707":
            break
        try:
            namesize = int(payload[cursor + 59 : cursor + 65], 8)
            filesize = int(payload[cursor + 65 : cursor + 76], 8)
        except ValueError:
            break
        name_bytes = payload[cursor + 76 : cursor + 76 + max(namesize - 1, 0)]
        name = name_bytes.decode("utf-8", "replace")
        if name == "TRAILER!!!":
            break
        names.append(name)
        cursor += 76 + namesize + filesize
    return names


def pkg_payload_bundles(blob: bytes, cap: int) -> tuple[dict[str, list[str]], str | None, list[str]]:
    """Return (bundle paths, error, component package names) for a flat pkg."""
    files, error = parse_xar_files(blob)
    if error:
        return {}, error, []
    components = sorted(
        {
            f["name"]
            for f in files
            if f.get("type") == "directory" and (f["name"] or "").endswith((".pkg", ".mpkg"))
        }
    )
    payloads = [f for f in files if f.get("name") == "Payload" and f.get("type") == "file"]
    if not payloads:
        return {}, "pkg has no 'Payload' file (component or script-only package)", components
    merged: dict[str, set[str]] = collections.defaultdict(set)
    for record in payloads:
        offset, length = record.get("offset"), record.get("length")
        if offset is None or length is None:
            continue
        if offset + length > len(blob):
            return {}, "pkg payload lies outside the downloaded bytes", components
        if length > cap or (record.get("size") or 0) > cap * 4:
            return {}, "pkg payload exceeds size cap", components
        payload, how = _decompress_stream(blob[offset : offset + length], cap)
        if how.startswith("decompress failed"):
            return {}, f"pkg payload not usable ({how})", components
        names = cpio_member_names(payload)
        if not names:
            return {}, "pkg payload is not a cpio archive", components
        for ext, paths in bundle_roots(names).items():
            merged[ext].update(paths)
    return (
        {ext: sorted(paths) for ext, paths in sorted(merged.items())},
        None,
        components,
    )


def pkg_bundles_from_zip_members(
    members: dict[str, bytes], cap: int
) -> tuple[dict[str, list[str]], str | None]:
    """Handle bundle-style `.pkg` directories inside a zip (pkg/Payload members)."""
    merged: dict[str, set[str]] = collections.defaultdict(set)
    found = False
    for name, blob in members.items():
        if not name.endswith("/Payload"):
            continue
        found = True
        if len(blob) > cap:
            return {}, "nested pkg payload exceeds size cap"
        payload, how = _decompress_stream(blob, cap)
        if not payload:
            return {}, f"nested pkg payload not usable ({how})"
        names = cpio_member_names(payload)
        if not names:
            return {}, "nested pkg payload is not a cpio archive"
        for ext, paths in bundle_roots(names).items():
            merged[ext].update(paths)
    if not found:
        return {}, "no nested pkg Payload member found"
    return {ext: sorted(paths) for ext, paths in sorted(merged.items())}, None


# ── Evidence cache ────────────────────────────────────────────────────────────


class Cache:
    def __init__(self, root: pathlib.Path):
        self.root = root
        self.blobs = root / "blobs"
        self.blobs.mkdir(parents=True, exist_ok=True)
        self.lock = threading.Lock()

    def _key(self, url: str) -> str:
        return hashlib.sha1(url.encode("utf-8")).hexdigest()

    def path(self, url: str) -> pathlib.Path:
        return self.blobs / f"{self._key(url)}.bin"

    def get(self, url: str) -> bytes | None:
        path = self.path(url)
        if path.exists():
            try:
                return path.read_bytes()
            except Exception:
                return None
        return None

    def put(self, url: str, blob: bytes) -> None:
        path = self.path(url)
        with self.lock:
            tmp = path.with_suffix(".tmp")
            tmp.write_bytes(blob)
            os.replace(tmp, path)


# ── Per-artifact analysis ─────────────────────────────────────────────────────


def analyze(url: str, timeout: float, cap: int, cache: Cache) -> tuple[Probe, Artifact]:
    artifact = Artifact()
    probe = probe_url(url, timeout)
    if probe.status is None:
        artifact.notes.append(probe.error or "no response")
        return probe, artifact
    if probe.status in (404, 410):
        return probe, artifact
    if probe.status >= 400:
        artifact.notes.append(f"HTTP {probe.status} {probe.content_type}".strip())
        return probe, artifact

    head_status, head_headers, head, _, _, head_error = fetch_range(url, 0, HEAD_BYTES - 1, timeout)
    if head_status is not None:
        total = _parse_content_range(_header(head_headers, "Content-Range"))
        if total:
            probe.content_length = total
        if _header(head_headers, "Content-Type"):
            probe.content_type = _header(head_headers, "Content-Type")
    if head_error and not head:
        artifact.notes.append(f"head fetch: {head_error}")

    tail = b""
    tail_available = False
    size = probe.content_length
    if size and size > HEAD_BYTES:
        tail_status, tail_headers, tail, _, _, tail_error = fetch_range(
            url, max(0, size - TAIL_BYTES), size - 1, timeout
        )
        if tail_error:
            artifact.notes.append(f"tail fetch: {tail_error}")
        if tail_status == 206 and _parse_content_range(_header(tail_headers, "Content-Range")):
            tail_available = True
        else:
            artifact.notes.append(
                f"server did not honour the range request for the tail (HTTP {tail_status}); "
                "the archive tail was not read"
            )
            tail = b""
    else:
        tail = head
        tail_available = True

    artifact.size_bytes = size if size else (len(head) or None)
    container, evidence = detect_container(head, tail, probe.content_type, url)
    artifact.container = container
    artifact.container_evidence = evidence
    if evidence != "magic":
        artifact.notes.append(f"container inferred from {evidence}, not from file magic")

    if container in ("zip",):
        if size is not None and size <= cap:
            blob = cache.get(url)
            if blob is None:
                blob = download_whole(url, size, cap, timeout)
                if blob is not None:
                    cache.put(url, blob)
            if blob is None:
                artifact.method = "magic-only"
                artifact.notes.append("full download failed; fell back to magic bytes")
            else:
                artifact.method = "download"
                artifact.size_bytes = len(blob)
                artifact.sha256 = hashlib.sha256(blob).hexdigest()
                members, truncated, error = list_zip_bytes(blob)
                if error:
                    artifact.notes.append(error)
                artifact.members = members
                artifact.members_truncated = truncated
                artifact.listing_complete = not truncated and error is None
                artifact.bundles = bundle_roots(members)
                artifact.nested_pkgs = find_nested(members, (".pkg", ".mpkg"))
                artifact.nested_dmgs = find_nested(members, (".dmg",))
                if artifact.nested_pkgs:
                    nested_payloads = read_zip_members(blob, artifact.nested_pkgs, cap)
                    if nested_payloads:
                        bundles, err = pkg_bundles_from_zip_members(nested_payloads, cap)
                        if err:
                            artifact.pkg_payload_error = err
                        else:
                            artifact.pkg_payload_bundles = bundles
                    for pkg in artifact.nested_pkgs:
                        member_blob = read_zip_member(blob, pkg)
                        if member_blob is not None and len(member_blob) <= cap:
                            flat, err, components = pkg_payload_bundles(member_blob, cap)
                            artifact.pkg_components.extend(components)
                            if err is None and flat:
                                for ext, paths in flat.items():
                                    artifact.pkg_payload_bundles.setdefault(ext, [])
                                    artifact.pkg_payload_bundles[ext] = sorted(
                                        set(artifact.pkg_payload_bundles[ext]) | set(paths)
                                    )
                                artifact.pkg_payload_listed = True
                            elif err and not artifact.pkg_payload_error:
                                artifact.pkg_payload_error = err
                        elif member_blob is not None:
                            artifact.pkg_payload_error = "nested pkg exceeds size cap"
        elif size is not None:
            if tail_available:
                names, complete, error = list_zip_tail(tail, size)
                artifact.method = "tail-cd"
                artifact.members = names
                artifact.listing_complete = complete and error is None
                if error:
                    artifact.notes.append(error)
                artifact.notes.append(
                    f"archive over size cap ({size} bytes); listed via central directory only"
                )
            else:
                names, _partial = list_zip_local_headers(head)
                artifact.method = "local-headers"
                artifact.members = names
                artifact.listing_complete = False
                artifact.notes.append(
                    f"archive over size cap ({size} bytes) and no usable tail; "
                    "listed the leading local file headers only"
                )
            artifact.bundles = bundle_roots(names)
            artifact.nested_pkgs = find_nested(names, (".pkg", ".mpkg"))
            artifact.nested_dmgs = find_nested(names, (".dmg",))
        else:
            artifact.method = "magic-only"
            artifact.notes.append("unknown archive size")
    elif container == "pkg":
        if size is not None and size <= cap:
            blob = cache.get(url)
            if blob is None:
                blob = download_whole(url, size, cap, timeout)
                if blob is not None:
                    cache.put(url, blob)
            if blob is not None:
                artifact.method = "download"
                artifact.size_bytes = len(blob)
                artifact.sha256 = hashlib.sha256(blob).hexdigest()
                bundles, error, components = pkg_payload_bundles(blob, cap)
                artifact.pkg_components = components
                if error:
                    artifact.pkg_payload_error = error
                else:
                    artifact.pkg_payload_bundles = bundles
                    artifact.pkg_payload_listed = True
                    artifact.listing_complete = True
        else:
            files, error = parse_xar_files(head)
            artifact.method = "pkg-toc"
            if error:
                artifact.notes.append(error)
            else:
                artifact.members = [f["name"] for f in files if f.get("name")]
            artifact.notes.append("pkg over size cap or size unknown; payload not listed")
    elif container == "dmg":
        artifact.method = "magic-only"
        artifact.notes.append("DMG contents are not inspected (never mounted)")

    return probe, artifact


def read_zip_member(blob: bytes, name: str) -> bytes | None:
    with zipfile.ZipFile(io.BytesIO(blob)) as archive:
        try:
            return archive.read(name)
        except Exception:
            return None


def read_zip_members(blob: bytes, names: list[str], cap: int) -> dict[str, bytes]:
    """Read `<pkg>/Payload` members for bundle-style packages inside a zip."""
    out: dict[str, bytes] = {}
    with zipfile.ZipFile(io.BytesIO(blob)) as archive:
        wanted = {n + "/Payload" for n in names}
        for info in archive.infolist():
            if info.filename in wanted and info.file_size <= cap:
                try:
                    out[info.filename] = archive.read(info.filename)
                except Exception:
                    continue
    return out


def download_whole(url: str, size: int, cap: int, timeout: float) -> bytes | None:
    if size > cap:
        return None
    status, headers, data, truncated, _, error = fetch_range(url, 0, size - 1, timeout)
    if status != 200 and status != 206:
        if error:
            return None
    if not data:
        return None
    expected = _parse_content_range(_header(headers, "Content-Range")) or size
    if truncated or (expected and len(data) < min(expected, size)):
        # keep a partial blob only when it is a valid archive on its own
        if len(data) < size:
            return None
    return data


# ── Classification + mismatch derivation ─────────────────────────────────────


def classify(entry: Entry, probe: Probe, artifact: Artifact) -> str:
    if probe.status is None:
        return UNREACHABLE
    if probe.status in (404, 410):
        return NOT_FOUND
    if probe.status >= 400:
        return UNREACHABLE
    if artifact.container == "zip":
        # A nested installer (pkg/dmg) is the real payload whenever the plugin
        # bundle itself is not directly in the archive.
        if artifact.nested_pkgs and (artifact.pkg_payload_bundles or not artifact.bundles):
            return ZIP_WITH_PKG
        if artifact.nested_dmgs and not artifact.bundles:
            return ZIP_WITH_DMG
        if artifact.bundles:
            return ZIP_WITH_BUNDLES
        return ZIP_MISMATCH
    if artifact.container == "dmg":
        return DMG
    if artifact.container == "pkg":
        return PKG
    if artifact.container == "html":
        return WEBPAGE
    if artifact.container.startswith("unknown-"):
        return {
            "unknown-zip": ZIP_MISMATCH,
            "unknown-dmg": DMG,
            "unknown-pkg": PKG,
        }[artifact.container]
    if artifact.container in ("json", "gzip", "bzip2", "xz", "tar", "macho", "pe", "ico"):
        # A response that is not an installable archive at all.
        return WEBPAGE if artifact.container == "json" else ZIP_MISMATCH
    return UNREACHABLE


def bundle_sources(artifact: Artifact) -> tuple[dict[str, list[str]], dict[str, list[str]]]:
    """Return (authoritative, secondary) bundle listings for this artifact.

    When an archive wraps a PKG whose payload was decoded, that payload — not
    the archive's loose members — is what the macOS installer places, so it is
    authoritative. A ZIP that bundles both macOS (inside the PKG) and
    Linux/Windows builds otherwise offers two same-named candidates.
    """
    if artifact.pkg_payload_bundles:
        return artifact.pkg_payload_bundles, artifact.bundles
    return artifact.bundles, artifact.pkg_payload_bundles


def format_bundles(artifact: Artifact, fmt: str) -> list[str]:
    ext = FORMAT_EXT.get(fmt)
    if not ext:
        return []
    primary, _secondary = bundle_sources(artifact)
    return sorted(set(primary.get(ext, [])))


def other_format_bundles(artifact: Artifact, fmt: str) -> list[str]:
    primary, _secondary = bundle_sources(artifact)
    ext = FORMAT_EXT.get(fmt)
    out: list[str] = []
    for found_ext, paths in primary.items():
        if found_ext != ext:
            out.extend(paths)
    return sorted(set(out))


def derive_mismatches(entry: Entry, probe: Probe, artifact: Artifact, classification: str):
    """Return (mismatches, unverified_reason). Mismatches are evidence-backed."""
    mismatches: list[dict] = []
    unverified: str | None = None

    def add(field: str, declared: str, actual: str, evidence: str):
        mismatches.append(
            {
                "field": field,
                "declared": declared,
                "actual": actual,
                "evidence": evidence,
            }
        )

    # ── download_type ─────────────────────────────────────────────────────────
    if entry.download_type == "direct":
        if classification == WEBPAGE:
            add(
                "download_type",
                entry.download_type,
                "manual",
                f"URL serves {probe.content_type or 'a non-archive page'} "
                f"(HTTP {probe.status}); no installable artifact to fetch",
            )
        elif classification == NOT_FOUND:
            add(
                "download_type",
                entry.download_type,
                "manual",
                f"URL returns HTTP {probe.status}; the declared direct download does not exist",
            )

    # ── install_type ──────────────────────────────────────────────────────────
    #
    # `install_type` describes how apm handles the downloaded archive, so a
    # nested container is only re-typed when no apm path can consume the
    # wrapper. apm's ZIP pipeline already routes a nested PKG to the privileged
    # pkg handler (prompting for admin), so a zip wrapping a PKG stays `zip`;
    # nothing unwraps a DMG that is stored inside a ZIP, so that artifact's real
    # installer type is `dmg`.
    expected_install: str | None = None
    nested_note = ""
    if classification in (ZIP_WITH_BUNDLES, ZIP_WITH_PKG, ZIP_MISMATCH):
        expected_install = "zip"
    elif classification == ZIP_WITH_DMG:
        expected_install = "dmg"
        nested_note = (
            "the archive wraps a DMG and no apm path unwraps a nested DMG; "
            "the artifact's installer type is DMG"
        )
    elif classification == PKG:
        expected_install = "pkg"
    elif classification == DMG:
        expected_install = "dmg"

    if expected_install and expected_install != entry.install_type:
        detail = {
            "zip": "packaged bundles",
            "pkg": "a PKG installer payload",
            "dmg": "a DMG image",
        }[expected_install]
        evidence = (
            f"artifact is {classification} (contains {detail}); "
            f"observed container={artifact.container}, size={artifact.size_bytes}, "
            f"method={artifact.method}"
        )
        if nested_note:
            evidence = f"{evidence}; {nested_note}"
        add("install_type", entry.install_type, expected_install, evidence)
    elif classification == ZIP_MISMATCH and entry.install_type == "zip":
        members = artifact.members[:8]
        prefix = "" if artifact.listing_complete else "listing incomplete; "
        unverified = (
            prefix
            + "ZIP payload matched no plugin bundle, PKG or DMG "
            f"(members: {', '.join(members) if members else 'unavailable'})"
        )

    # ── bundle_path / format presence ─────────────────────────────────────────
    # A format may only be dropped when the listing is complete *and* the archive
    # carries the plugin bundles directly: a nested pkg/dmg could still hold the
    # missing format.
    ext = FORMAT_EXT.get(entry.fmt)
    wraps_installer = bool(artifact.nested_pkgs or artifact.nested_dmgs)
    if ext and artifact.method in ("download", "tail-cd") and artifact.listing_complete:
        candidates = format_bundles(artifact, entry.fmt)
        declared_name = os.path.basename(entry.bundle_path.strip()) if entry.bundle_path else ""
        if not candidates:
            others = other_format_bundles(artifact, entry.fmt)
            pkg_proof = bool(artifact.nested_pkgs) and artifact.pkg_payload_listed
            if others and (not wraps_installer or pkg_proof):
                add(
                    "formats." + entry.fmt,
                    "present",
                    "absent",
                    f"artifact ({artifact.method}) contains no {ext} bundle but does contain "
                    f"{', '.join(others[:6])}",
                )
            elif wraps_installer:
                unverified = (
                    f"artifact is {classification}; the nested "
                    f"{'pkg' if artifact.nested_pkgs else 'dmg'} payload does not list a {ext} bundle"
                    if artifact.pkg_payload_listed
                    else f"artifact is {classification}; the nested "
                    f"{'pkg' if artifact.nested_pkgs else 'dmg'} contents were not listed"
                )
            else:
                unverified = f"no {ext} bundle and no other plugin bundle found in the listing"
        else:
            names = {}
            for path in candidates:
                names.setdefault(os.path.basename(path), path)
            if declared_name and declared_name in names:
                pass  # declared file name is present; apm resolves it by file name
            elif len(candidates) == 1:
                actual = candidates[0]
                if not declared_name or os.path.basename(actual) != declared_name:
                    add(
                        "bundle_path",
                        entry.bundle_path or "<empty>",
                        os.path.basename(actual),
                        f"artifact contains {', '.join(candidates)} (method={artifact.method})",
                    )
            else:
                unverified = (
                    f"multiple {entry.fmt} bundles in the artifact "
                    f"({', '.join(candidates[:6])}); no single name to pin"
                )
    elif ext and artifact.method == "local-headers":
        unverified = (
            "archive listing is partial (leading ZIP entries only), so the declared "
            "bundle_path cannot be confirmed or refuted"
        )
    elif ext and artifact.method == "magic-only" and classification in (DMG, ZIP_WITH_DMG):
        unverified = "DMG contents are not inspected, so the declared bundle_path cannot be confirmed"
    elif ext and artifact.method == "pkg-toc":
        unverified = artifact.pkg_payload_error or "PKG payload not listed (over size cap)"
    elif classification in (WEBPAGE, NOT_FOUND, UNREACHABLE):
        unverified = f"no artifact to inspect (classification={classification})"

    return mismatches, unverified


def analyze_entry(entry: Entry, timeout: float, cap: int, cache: Cache) -> dict:
    started = time.time()
    probe, artifact = analyze(entry.url, timeout, cap, cache)
    classification = classify(entry, probe, artifact)
    mismatches, unverified = derive_mismatches(entry, probe, artifact, classification)

    if artifact.sha256 and entry.sha256 and not entry.sha256.startswith("manual"):
        if artifact.sha256.lower() == entry.sha256.lower():
            sha_state = "match"
        else:
            sha_state = "mismatch"
    elif artifact.sha256:
        sha_state = "no-declared-checksum"
    else:
        sha_state = "not-computed"

    observations: list[str] = []
    if classification == ZIP_WITH_PKG:
        observations.append(
            "archive wraps a PKG installer: installing it requires administrator access "
            "(the CLI prompts through the nested-pkg path; the shared engine hands it off)"
        )
        ext = FORMAT_EXT.get(entry.fmt)
        loose = artifact.bundles.get(ext or "", []) if ext else []
        in_payload = artifact.pkg_payload_bundles.get(ext or "", []) if ext else []
        if loose and in_payload:
            observations.append(
                "the archive also carries loose non-macOS builds "
                f"({', '.join(loose[:3])}); apm's ZIP path resolves bundles by name and can pick one"
            )
    elif classification == ZIP_WITH_DMG:
        observations.append(
            "archive wraps a DMG: no apm archive path can install it, so it is not a "
            "machine-installable artifact"
        )
    elif classification == DMG:
        observations.append("DMG contents were not inspected (DMGs are never mounted)")
    if artifact.container_evidence != "magic" and classification in (DMG, PKG):
        observations.append(
            f"container identified from {artifact.container_evidence}, not from file magic"
        )
    if sha_state == "mismatch":
        observations.append(
            "declared sha256 does not match the served bytes; apm refuses to install on a "
            "checksum mismatch"
        )
    if classification == NOT_FOUND:
        observations.append("the declared download URL no longer exists (HTTP 404/410)")
    if classification == UNREACHABLE:
        observations.append(
            f"the download URL could not be fetched (HTTP {probe.status or 'no response'})"
        )

    return {
        "entry_id": entry.entry_id,
        "slug": entry.slug,
        "vendor": entry.vendor,
        "file": entry.file,
        "locator": entry.locator,
        "version": entry.version,
        "format": entry.fmt,
        "url": entry.url,
        "scope": entry.download_type,
        "declared": {
            "install_type": entry.install_type,
            "bundle_path": entry.bundle_path,
            "download_type": entry.download_type,
            "sha256": entry.sha256,
        },
        "probe": dataclasses.asdict(probe),
        "artifact": {
            "size_bytes": artifact.size_bytes,
            "container": artifact.container,
            "container_evidence": artifact.container_evidence,
            "method": artifact.method,
            "listing_complete": artifact.listing_complete,
            "members_truncated": artifact.members_truncated,
            "member_count": len(artifact.members),
            "members_sample": artifact.members[:MAX_MEMBERS_IN_REPORT],
            "bundles": artifact.bundles,
            "nested_pkgs": artifact.nested_pkgs,
            "nested_dmgs": artifact.nested_dmgs,
            "pkg_payload_bundles": artifact.pkg_payload_bundles,
            "pkg_payload_error": artifact.pkg_payload_error,
            "pkg_payload_listed": artifact.pkg_payload_listed,
            "pkg_components": artifact.pkg_components,
            "sha256": artifact.sha256,
            "notes": artifact.notes,
        },
        "classification": classification,
        "sha256_state": sha_state,
        "mismatches": mismatches,
        "unverified_reason": unverified,
        "observations": observations,
        "elapsed_s": round(time.time() - started, 2),
    }


# ── Registry parsing ─────────────────────────────────────────────────────────


def load_entries(
    registry_dir: pathlib.Path, report_base: pathlib.Path | None = None
) -> tuple[list[Entry], list[str]]:
    entries: list[Entry] = []
    errors: list[str] = []
    for path in sorted(registry_dir.rglob("*.toml")):
        if path.name in ("index.toml", "bundle_ids.toml", "installers.toml"):
            continue
        if report_base is not None and path.is_relative_to(report_base):
            rel = str(path.relative_to(report_base))
        else:
            rel = str(path.relative_to(registry_dir.parent))
        try:
            data = tomllib.loads(path.read_text(encoding="utf-8"))
        except Exception as exc:
            errors.append(f"{rel}: {exc}")
            continue
        if "formats" not in data and "releases" not in data:
            continue
        slug = data.get("slug") or path.stem
        vendor = data.get("vendor") or "?"
        for fmt, source in sorted((data.get("formats") or {}).items()):
            entries.append(make_entry(rel, slug, vendor, "formats", None, fmt, source))
        for release in data.get("releases") or []:
            version = str(release.get("version") or "?")
            for fmt, source in sorted((release.get("formats") or {}).items()):
                entries.append(
                    make_entry(rel, slug, vendor, f"releases[{version}]", version, fmt, source)
                )
    return entries, errors


def make_entry(rel, slug, vendor, locator, version, fmt, source) -> Entry:
    return Entry(
        file=rel,
        slug=slug,
        vendor=vendor,
        locator=locator,
        version=version,
        fmt=fmt,
        url=(source.get("url") or "").strip(),
        sha256=(source.get("sha256") or "").strip(),
        install_type=(source.get("install_type") or "").strip(),
        bundle_path=(source.get("bundle_path") or "").strip(),
        download_type=(source.get("download_type") or "").strip(),
    )


# ── Applying corrections ─────────────────────────────────────────────────────


def plan_corrections(results: list[dict]) -> list[dict]:
    """Collect the rewrites to apply.

    Only `download_type = "direct"` entries are rewritten: for `managed` and
    `manual` records the URL is a pointer the vendor flow opens rather than the
    artifact apm installs, so a probe of it cannot prove what apm would install.
    Those mismatches are still reported.
    """
    corrections: list[dict] = []
    for result in results:
        if result["scope"] != "direct":
            continue
        for mismatch in result["mismatches"]:
            corrections.append(
                {
                    "file": result["file"],
                    "entry_id": result["entry_id"],
                    "slug": result["slug"],
                    "locator": result["locator"],
                    "version": result["version"],
                    "format": result["format"],
                    "field": mismatch["field"],
                    "from": mismatch["declared"],
                    "to": mismatch["actual"],
                    "evidence": mismatch["evidence"],
                    "classification": result["classification"],
                    "url": result["url"],
                }
            )
    corrections.sort(key=lambda c: (c["file"], c["entry_id"], c["field"]))
    return corrections


def split_unsafe_removals(
    corrections: list[dict], results: list[dict]
) -> tuple[list[dict], list[dict]]:
    """Separate removals that would leave a record with no formats at all.

    Dropping every declared format would turn an installable record into one apm
    cannot select anything from, so those are reported instead of applied: the
    artifact proves the formats are wrong, but the fix needs a new format block
    (for example `[formats.app]` for an installer-app package), which is out of
    scope for a field correction.
    """
    declared = collections.Counter(
        (result["file"], result["locator"]) for result in results if result["scope"] == "direct"
    )
    removals = collections.Counter(
        (correction["file"], correction["locator"])
        for correction in corrections
        if correction["field"].startswith("formats.")
    )
    keep: list[dict] = []
    deferred: list[dict] = []
    for correction in corrections:
        key = (correction["file"], correction["locator"])
        if (
            correction["field"].startswith("formats.")
            and declared.get(key, 0) > 0
            and removals.get(key, 0) >= declared[key]
        ):
            deferred.append(correction)
            continue
        keep.append(correction)
    return keep, deferred


def apply_corrections(
    corrections: list[dict], repo_root: pathlib.Path
) -> tuple[list[dict], list[str]]:
    """Apply field corrections / block removals to the registry TOMLs.

    Every edit is scoped to the exact TOML table the entry lives in (top-level
    `[formats.<fmt>]` or `[releases.formats.<fmt>]` under a specific release),
    so a correction can never touch a sibling format or a historical release.
    Returns the corrections that were written plus any problems encountered.
    """
    by_file: dict[str, list[dict]] = collections.defaultdict(list)
    for correction in corrections:
        by_file[correction["file"]].append(correction)

    applied: list[dict] = []
    problems: list[str] = []
    for rel, items in sorted(by_file.items()):
        path = repo_root / rel
        if not path.exists():
            problems.append(f"{rel}: missing file")
            continue
        lines = path.read_text(encoding="utf-8").split("\n")
        file_applied, file_problems = apply_file_corrections(lines, rel, items)
        applied.extend(file_applied)
        problems.extend(file_problems)
        if lines and lines[-1].strip() != "":
            lines.append("")
        path.write_text("\n".join(lines), encoding="utf-8")
    return applied, problems


def locate_blocks(lines: list[str]) -> list[tuple[str, str | None, str | None, int, int]]:
    """Return (kind, release_version, format, start, end) for every TOML table.

    `start` is the header line and `end` is exclusive. `kind` is one of
    `formats`, `releases`, `releases.formats`, `other`.
    """
    headers: list[tuple[int, str, str | None, str | None]] = []
    release_version: str | None = None
    for index, line in enumerate(lines):
        stripped = line.strip()
        if stripped == "[[releases]]":
            release_version = None
            headers.append((index, "releases", None, None))
        elif stripped.startswith("[releases.formats.") and stripped.endswith("]"):
            headers.append((index, "releases.formats", release_version, stripped[18:-1]))
        elif stripped.startswith("[formats.") and stripped.endswith("]"):
            headers.append((index, "formats", None, stripped[9:-1]))
        elif stripped.startswith("["):
            headers.append((index, "other", None, None))
        elif (
            stripped.startswith("version")
            and "=" in stripped
            and release_version is None
            and headers
            and headers[-1][1] == "releases"
            and index == headers[-1][0] + 1
        ):
            release_version = stripped.split("=", 1)[1].strip().strip('"')
    blocks: list[tuple[str, str | None, str | None, int, int]] = []
    for position, (start, kind, version, fmt) in enumerate(headers):
        end = headers[position + 1][0] if position + 1 < len(headers) else len(lines)
        blocks.append((kind, version, fmt, start, end))
    return blocks


def find_block(
    blocks: list[tuple[str, str | None, str | None, int, int]], locator: str, fmt: str
):
    kind = "formats" if locator == "formats" else "releases.formats"
    version = None
    match = re.match(r"releases\[(.+)\]", locator)
    if match:
        version = match.group(1)
    for block in blocks:
        if block[0] == kind and block[2] == fmt and (version is None or block[1] == version):
            return block
    return None


def apply_file_corrections(
    lines: list[str], rel: str, items: list[dict]
) -> tuple[list[dict], list[str]]:
    applied: list[dict] = []
    problems: list[str] = []
    blocks = locate_blocks(lines)
    removals = [c for c in items if c["field"].startswith("formats.")]
    edits = [c for c in items if not c["field"].startswith("formats.")]

    for correction in removals:
        fmt = correction["field"].split(".", 1)[1]
        block = find_block(blocks, correction["locator"], fmt)
        if block is None:
            # Already gone: re-applying a correction list is a no-op.
            continue
        start, end = block[3], block[4]
        del lines[start:end]
        if start > 0 and lines[start - 1].strip() == "" and (start >= len(lines) or lines[start].strip() == ""):
            del lines[start - 1]
        blocks = locate_blocks(lines)
        applied.append(correction)

    for correction in edits:
        key = correction["field"]
        block = find_block(blocks, correction["locator"], correction["format"])
        if block is None:
            problems.append(
                f"{rel}: no [{correction['format']}] block in {correction['locator']} "
                f"for {correction['entry_id']}"
            )
            continue
        start, end = block[3], block[4]
        pattern = re.compile(r'^(\s*' + re.escape(key) + r'\s*=\s*)"([^"]*)"(\s*)$')
        replaced = False
        for index in range(start + 1, end):
            match = pattern.match(lines[index])
            if match:
                lines[index] = f'{match.group(1)}"{correction["to"]}"{match.group(3)}'
                replaced = True
                break
        if not replaced:
            # The key is absent: add it in the same column as the other keys.
            columns = [
                line.index("=")
                for line in lines[start + 1 : end]
                if re.match(r"^\s*[A-Za-z_][A-Za-z0-9_]*\s*=", line)
            ]
            width = max(max(columns, default=0), len(key) + 1)
            lines.insert(end, f'{key.ljust(width)}= "{correction["to"]}"')
            if end < len(lines) and lines[end].startswith("["):
                lines.insert(end + 1, "")
        applied.append(correction)
        blocks = locate_blocks(lines)
    return applied, problems


# ── Reporting ────────────────────────────────────────────────────────────────


def build_totals(results: list[dict]) -> dict:
    by_class = collections.Counter(r["classification"] for r in results)
    by_scope = collections.Counter(r["scope"] for r in results)
    by_scope_class: dict[str, dict[str, int]] = collections.defaultdict(collections.Counter)
    for result in results:
        by_scope_class[result["scope"]][result["classification"]] += 1
    mismatched_fields = collections.Counter()
    for result in results:
        for mismatch in result["mismatches"]:
            mismatched_fields[mismatch["field"].split(".")[0]] += 1
    return {
        "entries": len(results),
        "by_class": {k: by_class.get(k, 0) for k in ALL_CLASSES},
        "by_scope": {k: by_scope.get(k, 0) for k in SCOPE_ORDER},
        "by_scope_class": {
            scope: {k: by_scope_class[scope].get(k, 0) for k in ALL_CLASSES}
            for scope in SCOPE_ORDER
            if scope in by_scope_class
        },
        "mismatched_fields": dict(mismatched_fields),
        "sha256": {
            state: sum(1 for r in results if r["sha256_state"] == state)
            for state in ("match", "mismatch", "no-declared-checksum", "not-computed")
        },
    }


def table_row(cells: list[str]) -> str:
    return "| " + " | ".join(c.replace("|", "\\|") for c in cells) + " |"


def scraped_source_stats(repo: pathlib.Path, registry_dir: pathlib.Path, scraped_dir: pathlib.Path) -> dict | None:
    """Compare the curated registry with a secondary (scraped) source on disk."""
    if not scraped_dir.exists():
        return None
    scraped_plugins = scraped_dir / "plugins"
    if not scraped_plugins.exists():
        return None

    curated_entries, _ = load_entries(registry_dir)
    machine = {
        e.slug
        for e in curated_entries
        if e.download_type in ("direct", "managed")
    }

    records = 0
    commercial = 0
    paid = 0
    slugs: set[str] = set()
    for path in sorted(scraped_plugins.rglob("*.toml")):
        try:
            data = tomllib.loads(path.read_text(encoding="utf-8"))
        except Exception:
            continue
        records += 1
        if (data.get("license") or "").lower() == "commercial":
            commercial += 1
        if data.get("is_paid") is True:
            paid += 1
        slugs.add(data.get("slug") or path.stem)

    curated_slugs = {e.slug for e in curated_entries}
    collisions = slugs & curated_slugs
    return {
        "path": str(scraped_dir.relative_to(repo)) if scraped_dir.is_relative_to(repo) else str(scraped_dir),
        "records": records,
        "commercial": commercial,
        "is_paid_true": paid,
        "collisions": len(collisions),
        "curated_records": len(curated_slugs),
        "shadowed_machine_fetchable": len(machine & collisions),
    }


def write_report(path: pathlib.Path, payload: dict, applied_corrections: list[dict] | None):
    results = payload["results"]
    totals = payload["totals"]
    lines: list[str] = []
    add = lines.append

    add("# Registry artifact verification")
    add("")
    add(
        "This report is generated by `scripts/verify-registry-artifacts.py`. It re-derives, from "
        "the bytes each registry URL actually serves, whether `install_type`, `bundle_path` and "
        "`download_type` describe the artifact they point at."
    )
    add("")
    add(f"- Generated: `{payload['generated_at']}` (tool v{payload['tool_version']})")
    add(f"- Registry root: `{payload['registry_root']}`")
    add(
        f"- Limits: download cap `{payload['max_bytes']}` bytes, request timeout "
        f"`{payload['timeout']}s`, concurrency `{payload['concurrency']}`"
    )
    add("- Nothing is installed, no DMG is mounted, no privileged command is run.")
    add(
        f"- This run proposes {len(payload['corrections'])} field corrections "
        "(`download_type = \"direct\"` records only); a run after `--apply` should propose none "
        "for the fields it already fixed."
    )
    add("")
    add("## Method and limits")
    add("")
    add(
        "Each format entry is probed over HTTP with redirects followed (status, content-type, "
        "content-length, `Content-Disposition`); the first bytes identify the container from its "
        "magic, and ZIPs are listed either from a full download (under the cap) or from the "
        "archive's central directory in the tail (over the cap). PKGs are decoded from their xar "
        "TOC and cpio payload, so the bundle names a package installs are known without installing "
        "it. DMGs are never mounted, so their contents stay unverified."
    )
    add("")
    add(
        "- Re-runnable and idempotent: downloaded bytes are cached under "
        "`data/registry-verification-cache/`, results are sorted, and no state outside the repo "
        "changes."
    )
    add("- No plugin is installed, no bytes are written under `~/.apm` or the apm data dir, no `sudo`.")
    add(
        f"- Bounded: {payload['max_bytes']} byte download cap per artifact, "
        f"{payload['timeout']}s per request, {payload['concurrency']} concurrent workers."
    )
    add("- `--apply` is the only mode that edits the registry, and only from recorded evidence.")
    add("")
    add("### Container policy for `install_type`")
    add("")
    add(
        "`install_type` says how apm handles the downloaded archive, so a nested container is only "
        "re-typed when no apm path can consume the wrapper:"
    )
    add("")
    add("| Artifact | `install_type` | Why |")
    add("| --- | --- | --- |")
    add(
        "| ZIP of plugin bundles | `zip` | core extracts and places the bundle |"
    )
    add(
        "| ZIP wrapping a PKG | `zip` | `apm-core`'s ZIP path finds the nested `.pkg` and routes it "
        "to the privileged pkg handler, so `apm install` prompts for admin; re-typing it as `pkg` "
        "would hand a `.zip` to `installer -pkg` and break installs that work today |"
    )
    add(
        "| ZIP wrapping a DMG | `dmg` | no apm path unwraps a nested DMG, so the artifact's real "
        "installer type is a DMG (it remains non-installable by apm either way) |"
    )
    add("| Flat PKG (xar) | `pkg` | `installer -pkg`, privileged |")
    add("| DMG | `dmg` | mounted and copied |")
    add("| URL serving HTML/404 | unchanged; `download_type` becomes `manual` | nothing to install |")
    add("")
    add("## Priority order")
    add("")
    add("| Wave | Scope | Entries | Unique URLs | What was done |")
    add("| --- | --- | --- | --- | --- |")
    scope_urls = payload["scope_urls"]
    ran = [scope for scope in SCOPE_ORDER if totals["by_scope"].get(scope, 0)]
    wave_notes = {
        "direct": "every unique URL downloaded and inspected under the cap",
        "managed": (
            "every unique URL probed; artifacts listed/inspected under the cap, the rest reported "
            "with sizes"
        ),
        "manual": (
            "probed only, to count how many already point at a fetchable artifact (no rewrites)"
        ),
    }
    for index, scope in enumerate(ran, start=1):
        add(
            f"| {index} | `download_type = \"{scope}\"` | {totals['by_scope'][scope]} | "
            f"{scope_urls.get(scope, 0)} | {wave_notes[scope]} |"
        )
    for scope in SCOPE_ORDER:
        if scope not in ran:
            add(
                f"| – | `download_type = \"{scope}\"` | not run | – | "
                "re-run with `--scope all` (`managed`/`manual` are probe-only waves) |"
            )
    add("")
    manual_scoped = [r for r in results if r["scope"] == "manual"]
    if manual_scoped:
        manual_fetchable = [
            r
            for r in manual_scoped
            if r["classification"] in (ZIP_WITH_BUNDLES, ZIP_WITH_PKG, ZIP_WITH_DMG, DMG, PKG)
        ]
        add(
            f"{len(manual_fetchable)} of {len(manual_scoped)} `manual` entries already point at a "
            "fetchable artifact (an archive or installer rather than a product page). They are "
            "listed under per-entry status as promotion candidates; none of them were rewritten."
        )
        add("")
    add("## Totals per class")
    add("")
    add("| Class | Entries |")
    add("| --- | --- |")
    for name in ALL_CLASSES:
        add(f"| `{name}` | {totals['by_class'].get(name, 0)} |")
    add(f"| **total** | **{totals['entries']}** |")
    add("")
    add("| Class | direct | managed | manual |")
    add("| --- | --- | --- | --- |")
    for name in ALL_CLASSES:
        row = [totals["by_scope_class"].get(scope, {}).get(name, 0) for scope in SCOPE_ORDER]
        add(f"| `{name}` | {row[0]} | {row[1]} | {row[2]} |")
    add("")
    add("## Checksum state")
    add("")
    sha = totals["sha256"]
    add(
        f"- `match`: {sha.get('match', 0)} entries whose downloaded bytes hash to the declared "
        "`sha256`"
    )
    add(f"- `mismatch`: {sha.get('mismatch', 0)} entries whose declared `sha256` is wrong")
    add(f"- `not-computed`: {sha.get('not-computed', 0)} entries where no bytes were hashed")
    add("")
    add("## Mismatched fields")
    add("")
    add("| Field | Entries |")
    add("| --- | --- |")
    for field, count in sorted(totals["mismatched_fields"].items()):
        add(f"| `{field}` | {count} |")
    add("")

    stats = payload.get("scraped_source")
    if stats:
        add("## Registry source precedence (second fix)")
        add("")
        add(
            "The configured sources disagree per slug. On this machine the curated registry "
            "(`registry/plugins/**`, source `official`) and a scraped catalogue "
            f"(`{stats['path']}`, source `scraped`) both define most slugs, and the scraped record "
            "always wins when it is loaded last:"
        )
        add("")
        add(
            f"- scraped records: {stats['records']} "
            f"({stats['commercial']} declare `license = \"commercial\"`, "
            f"{stats['is_paid_true']} declare `is_paid = true`). That is the scraper's template "
            "default (`scripts/generate-registry.py`, `scripts/enrich-registry.py`), not evidence "
            "about the plugin, and the scraped records also ship empty download URLs. The scraped "
            "catalogue and the scrapers live in gitignored paths (`data/`, `scripts/`), so the "
            "durable fix belongs in the merge rule rather than in that data."
        )
        add(
            f"- slugs defined by both sources: {stats['collisions']} "
            f"of {stats['curated_records']} curated slugs"
        )
        add(
            f"- curated machine-fetchable format entries shadowed by a scraped record: "
            f"{stats['shadowed_machine_fetchable']} — those installs resolve to an empty URL and "
            "`download_type = \"manual\"`"
        )
        add("")
        add(
            "Precedence is now explicit and deterministic: sources are ordered by priority "
            "(built-in `official` first, then user sources in `config.toml` order) and the "
            "**highest-priority source that defines a slug owns the merged record**. Lower-priority "
            "sources only add slugs the official registry does not define; their records stay "
            "readable through `Registry::find_in_source` / `plugins_by_source`. To make a fork "
            "authoritative, point `default_registry_url` at it. This is pinned by "
            "`lower_priority_source_cannot_reclassify_a_curated_free_plugin` and "
            "`load_all_sources_tracks_source_specific_provenance` in "
            "`crates/apm-core/src/registry/mod.rs`."
        )
        add("")

    if applied_corrections:
        add("## Corrections applied in this change")
        add("")
        add("| File | Entry | Field | Before | After | Evidence |")
        add("| --- | --- | --- | --- | --- | --- |")
        for correction in applied_corrections:
            add(
                table_row(
                    [
                        f"`{correction['file']}`",
                        f"`{correction['entry_id']}`",
                        f"`{correction['field']}`",
                        f"`{correction['from']}`",
                        f"`{correction['to']}`",
                        correction["evidence"],
                    ]
                )
            )
        add("")

    deferred = [
        (result, mismatch)
        for result in results
        if result["scope"] != "direct"
        for mismatch in result["mismatches"]
    ]
    add("## Observed but not applied")
    add("")
    add(
        "For `managed` and `manual` records the URL is where apm sends the user, not the artifact "
        "apm installs, so these mismatches are reported and deliberately left unchanged "
        f"({len(deferred)} fields)."
    )
    add("")
    if deferred:
        add("| Entry | Scope | Field | Declared | Observed | Evidence |")
        add("| --- | --- | --- | --- | --- | --- |")
        for result, mismatch in deferred[:200]:
            add(
                table_row(
                    [
                        f"`{result['entry_id']}`",
                        f"`{result['scope']}`",
                        f"`{mismatch['field']}`",
                        f"`{mismatch['declared']}`",
                        f"`{mismatch['actual']}`",
                        mismatch["evidence"],
                    ]
                )
            )
        if len(deferred) > 200:
            add(f"| … | | | | | {len(deferred) - 200} more (see the JSON report) |")
    add("")

    not_applied = payload.get("corrections_not_applied") or []
    if not_applied:
        add("### Format claims that need a new format block")
        add("")
        add(
            "For these records every declared format is absent from the artifact, so removing them "
            "would leave the record with no formats at all. The artifact proves the declared "
            "formats are wrong; the fix is to declare the format the artifact actually carries "
            "(for example `[formats.app]` for an installer-app package), which is a schema change "
            "rather than a field correction, so it is reported instead of applied."
        )
        add("")
        add("| Entry | Field | Evidence |")
        add("| --- | --- | --- |")
        for correction in not_applied:
            add(
                table_row(
                    [
                        f"`{correction['entry_id']}`",
                        f"`{correction['field']}`",
                        correction["evidence"],
                    ]
                )
            )
        add("")

    unverified = [r for r in results if r["unverified_reason"]]
    add("## Could not be verified")
    add("")
    add(
        f"{len(unverified)} of {len(results)} entries could not be fully verified. Reasons are "
        "grouped below; no field was changed for any of them."
    )
    add("")
    groups: dict[str, list[dict]] = collections.defaultdict(list)
    for result in unverified:
        groups[result["unverified_reason"]].append(result)
    for reason, items in sorted(groups.items(), key=lambda kv: (-len(kv[1]), kv[0])):
        add(f"### {reason} ({len(items)})")
        add("")
        add("| Entry | Class | Size | URL |")
        add("| --- | --- | --- | --- |")
        for result in items[:80]:
            add(
                table_row(
                    [
                        f"`{result['entry_id']}`",
                        f"`{result['classification']}`",
                        str(result["artifact"]["size_bytes"] or "-"),
                        f"`{short_url(result['url'])}`",
                    ]
                )
            )
        if len(items) > 80:
            add(f"| … | | | {len(items) - 80} more (see the JSON report) |")
        add("")

    add("## Per-entry status")
    add("")
    add(
        "Full machine-readable detail (probe headers, listing samples, per-field mismatches) for "
        "every entry is in `data/registry-verification.json`. Every machine-fetchable entry "
        "(`direct` + `managed`) is listed below; for the `manual` wave only the entries whose URL "
        "already serves an installable artifact are listed, since the rest are not install targets."
    )
    add("")
    for scope in SCOPE_ORDER:
        scoped = [r for r in results if r["scope"] == scope]
        if not scoped:
            continue
        listed = scoped
        if scope == "manual":
            listed = [
                r
                for r in scoped
                if r["classification"]
                in (ZIP_WITH_BUNDLES, ZIP_WITH_PKG, ZIP_WITH_DMG, DMG, PKG)
            ]
        add(f"### `download_type = \"{scope}\"` ({len(scoped)} entries, {len(listed)} listed)")
        add("")
        add("| Entry | Format | Class | Declared install_type | Size | Listing | Evidence | Note |")
        add("| --- | --- | --- | --- | --- | --- | --- | --- |")
        for result in listed:
            listing = result["artifact"]
            listing_note = f"{listing['member_count']} members"
            if listing["method"] == "magic-only":
                listing_note = "not listed"
            elif listing["method"] == "pkg-toc":
                listing_note = "xar TOC only"
            elif listing["method"] == "local-headers":
                listing_note = f"{listing['member_count']} leading members"
            evidence = "; ".join(
                f"{m['field']}: {m['declared']} -> {m['actual']}" for m in result["mismatches"]
            )
            if not evidence:
                evidence = result["unverified_reason"] or "consistent"
            add(
                table_row(
                    [
                        f"`{result['entry_id']}`",
                        f"`{result['format']}`",
                        f"`{result['classification']}`",
                        f"`{result['declared']['install_type']}`",
                        str(listing["size_bytes"] or "-"),
                        listing_note,
                        evidence,
                        "; ".join(result.get("observations", [])) or "-",
                    ]
                )
            )
        add("")

    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def short_url(url: str, width: int = 72) -> str:
    return url if len(url) <= width else url[: width - 1] + "…"


# ── CLI ──────────────────────────────────────────────────────────────────────


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--repo", default=str(pathlib.Path(__file__).resolve().parent.parent))
    parser.add_argument("--scope", default="machine", choices=["machine", "direct", "managed", "manual", "all"])
    parser.add_argument("--max-bytes", type=int, default=64 * 1024 * 1024)
    parser.add_argument("--timeout", type=float, default=20.0)
    parser.add_argument("--concurrency", type=int, default=6)
    parser.add_argument("--cache-dir", default=None)
    parser.add_argument("--out-json", default=None)
    parser.add_argument("--out-report", default=None)
    parser.add_argument("--applied-corrections", default=None)
    parser.add_argument(
        "--scraped-source",
        default=None,
        help="secondary (scraped) registry checkout to compare against, for the precedence section",
    )
    parser.add_argument("--limit", type=int, default=0)
    parser.add_argument("--url-contains", default=None)
    parser.add_argument("--slug", action="append", default=None)
    parser.add_argument("--apply", action="store_true")
    args = parser.parse_args(argv)

    repo = pathlib.Path(args.repo).resolve()
    registry_dir = repo / "registry" / "plugins"
    cache_dir = pathlib.Path(args.cache_dir) if args.cache_dir else repo / "data" / "registry-verification-cache"
    out_json = pathlib.Path(args.out_json) if args.out_json else repo / "data" / "registry-verification.json"
    out_report = (
        pathlib.Path(args.out_report)
        if args.out_report
        else repo / "docs" / "registry-verification.md"
    )
    applied_path = (
        pathlib.Path(args.applied_corrections)
        if args.applied_corrections
        else repo / "data" / "registry-verification-corrections.json"
    )
    scraped_source = (
        pathlib.Path(args.scraped_source) if args.scraped_source else repo / "data" / "registry"
    )

    entries, parse_errors = load_entries(registry_dir, repo)
    if parse_errors:
        for error in parse_errors[:20]:
            print(f"registry parse error: {error}", file=sys.stderr)

    if args.scope == "machine":
        scopes = {"direct", "managed"}
    elif args.scope == "all":
        scopes = set(SCOPE_ORDER)
    else:
        scopes = {args.scope}
    selected = [e for e in entries if e.download_type in scopes]
    if args.slug:
        wanted = set(args.slug)
        selected = [e for e in selected if e.slug in wanted]
    if args.url_contains:
        selected = [e for e in selected if args.url_contains in e.url]
    selected.sort(key=lambda e: (SCOPE_ORDER.index(e.download_type), e.file, e.entry_id))

    if args.limit:
        selected = selected[: args.limit]
        print(f"limited to {len(selected)} entries", flush=True)

    scope_urls: dict[str, int] = {}
    for scope in SCOPE_ORDER:
        scope_urls[scope] = len({e.url for e in selected if e.download_type == scope})

    print(
        f"verifying {len(selected)} entries "
        f"({', '.join(f'{s}={sum(1 for e in selected if e.download_type == s)}' for s in SCOPE_ORDER if any(e.download_type == s for e in selected))})",
        flush=True,
    )

    cache = Cache(cache_dir)
    results: list[dict] = []
    started = time.time()
    with concurrent.futures.ThreadPoolExecutor(max_workers=max(1, args.concurrency)) as pool:
        futures = {
            pool.submit(analyze_entry, entry, args.timeout, args.max_bytes, cache): entry
            for entry in selected
        }
        done = 0
        for future in concurrent.futures.as_completed(futures):
            entry = futures[future]
            try:
                results.append(future.result())
            except Exception as exc:  # pragma: no cover - defensive
                results.append(
                    {
                        "entry_id": entry.entry_id,
                        "slug": entry.slug,
                        "vendor": entry.vendor,
                        "file": entry.file,
                        "locator": entry.locator,
                        "version": entry.version,
                        "format": entry.fmt,
                        "url": entry.url,
                        "scope": entry.download_type,
                        "declared": {
                            "install_type": entry.install_type,
                            "bundle_path": entry.bundle_path,
                            "download_type": entry.download_type,
                            "sha256": entry.sha256,
                        },
                        "probe": {"error": f"{type(exc).__name__}: {exc}"},
                        "artifact": {"method": "none", "container": "unknown", "size_bytes": None},
                        "classification": UNREACHABLE,
                        "sha256_state": "not-computed",
                        "mismatches": [],
                        "unverified_reason": f"verifier error: {type(exc).__name__}: {exc}",
                        "elapsed_s": 0.0,
                    }
                )
            done += 1
            if done % 100 == 0 or done == len(selected):
                print(f"  {done}/{len(selected)} entries", flush=True)

    results.sort(key=lambda r: (SCOPE_ORDER.index(r["scope"]), r["file"], r["entry_id"]))
    corrections = plan_corrections(results)
    corrections, deferred_removals = split_unsafe_removals(corrections, results)
    payload = {
        "tool_version": TOOL_VERSION,
        "generated_at": _dt.datetime.now(_dt.timezone.utc).isoformat(timespec="seconds"),
        "registry_root": str(registry_dir),
        "scopes": sorted(scopes),
        "max_bytes": args.max_bytes,
        "timeout": args.timeout,
        "concurrency": args.concurrency,
        "scope_urls": scope_urls,
        "scraped_source": scraped_source_stats(repo, registry_dir, scraped_source),
        "totals": build_totals(results),
        "corrections": corrections,
        "corrections_not_applied": deferred_removals,
        "results": results,
        "duration_s": round(time.time() - started, 1),
    }
    out_json.parent.mkdir(parents=True, exist_ok=True)
    out_json.write_text(json.dumps(payload, indent=1, sort_keys=False) + "\n", encoding="utf-8")

    print(f"\nclassification totals: {json.dumps(payload['totals']['by_class'])}")
    print(f"corrections proposed: {len(corrections)}")
    print(f"wrote {out_json}")

    applied_corrections: list[dict] | None = None
    if args.apply:
        applied, problems = apply_corrections(corrections, repo)
        print(f"applied {len(applied)} of {len(corrections)} corrections to the registry")
        for problem in problems:
            print(f"  problem: {problem}", file=sys.stderr)
        applied_path.write_text(json.dumps(applied, indent=1) + "\n", encoding="utf-8")
        print(f"wrote {applied_path}")
        applied_corrections = applied or None
    elif applied_path.exists():
        try:
            applied_corrections = json.loads(applied_path.read_text(encoding="utf-8"))
        except Exception:
            applied_corrections = None

    write_report(out_report, payload, applied_corrections)
    print(f"wrote {out_report}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
