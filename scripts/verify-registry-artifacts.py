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
  * One probe per distinct URL: entries that share a URL share the probe and
    the artifact listing, since the result depends on the URL alone.
  * `--apply` rewrites `download_type = "direct"` records only.  A `direct`
    record claims apm fetches the URL itself, so the bytes can prove it wrong
    (HTML or a 404 for a `direct` record means the record is really `manual`).
    A `managed` record claims the plugin is installed through a vendor manager
    app and lists that app's paths in `registry/installers.toml`, and a
    `manual` record's URL is where apm sends the user, so for those the probe
    is evidence but not a field correction: `managed` URLs are reported, and
    `manual` URLs that answer with an artifact themselves are listed as
    promotion proposals.

Usage:

    python3 scripts/verify-registry-artifacts.py                     # direct+managed
    python3 scripts/verify-registry-artifacts.py --scope all         # + manual probe
    python3 scripts/verify-registry-artifacts.py --scope direct --limit 20
    python3 scripts/verify-registry-artifacts.py --scope managed --host-interval 3
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

TOOL_VERSION = "1.1.0"
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

# Classes where the URL answered with an artifact apm can inspect or install,
# as opposed to a product page. A ZIP that wraps a DMG is included here (the
# bytes are an artifact) but can never become an install target, because no apm
# path unwraps a nested DMG.
ARTIFACT_CLASSES = (ZIP_WITH_BUNDLES, ZIP_WITH_PKG, ZIP_WITH_DMG, DMG, PKG)

# format key in the registry -> bundle extension
FORMAT_EXT = {"vst3": ".vst3", "au": ".component", "app": ".app"}
# every extension that marks an audio-plugin bundle inside an archive
PLUGIN_EXTS = {".vst3", ".component", ".app", ".clap", ".aaxplugin", ".vst"}
# URL shapes that look like a direct download rather than a product page
ARCHIVE_EXTS = (".dmg", ".zip", ".pkg", ".mpkg", ".tar.gz", ".tgz", ".tar", ".rar", ".7z", ".sit", ".sitx")
# Filenames of a vendor-wide downloader/manager app rather than one plugin's
# artifact. Phrase forms only, so "<product>-1.2-osx-installer.dmg" is not one.
VENDOR_TOOL_RE = re.compile(
    r"(?i)(offline installer|installation manager|software center|native access|"
    r"ua connect|waves central|roland cloud|product manager|product portal|"
    r"creative tools|download manager|install center)"
)

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


class HostPacer:
    """Minimum spacing between requests to the same host (`0` disables it).

    Some hosts sit behind a rate limiter that answers a burst with `429` for
    every request in it, so retrying harder cannot help: spacing the requests
    does. `--host-interval` applies an interval to every host; a host that
    answers `429` gets `--pace-on-429` from then on instead, so only the hosts
    that need spacing pay for it.
    """

    def __init__(self, interval: float, on_429: float = 0.0):
        self.interval = max(0.0, interval)
        self.on_429 = max(0.0, on_429)
        self._lock = threading.Lock()
        self._next: dict[str, float] = {}
        self._host_interval: dict[str, float] = {}

    def _host(self, url: str) -> str:
        return urllib.parse.urlparse(url).netloc.lower()

    def _interval_for(self, host: str) -> float:
        return max(self.interval, self._host_interval.get(host, 0.0))

    def wait(self, url: str) -> None:
        host = self._host(url)
        while True:
            with self._lock:
                span = self._interval_for(host)
                if not span:
                    return
                now = time.monotonic()
                ready = self._next.get(host, 0.0)
                if now >= ready:
                    self._next[host] = now + span
                    return
                delay = ready - now
            time.sleep(min(delay, 0.5))

    def penalize(self, url: str, retry_after: float | None = None) -> None:
        """The host answered `429`: space it out from now on."""
        if not self.on_429:
            return
        host = self._host(url)
        with self._lock:
            self._host_interval[host] = max(
                self._host_interval.get(host, 0.0), self.on_429
            )
            cooldown = retry_after if retry_after and retry_after > 0 else 5.0
            self._next[host] = max(self._next.get(host, 0.0), time.monotonic() + cooldown)


# Replaced by `main` from `--host-interval` / `--pace-on-429`; unpaced by default.
PACER = HostPacer(0.0, 0.0)


def _request(url: str, headers: dict[str, str], timeout: float, method: str = "GET"):
    PACER.wait(url)
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


def _retry_after(headers) -> float | None:
    value = _header(headers, "Retry-After").strip()
    if value.isdigit():
        return float(value)
    return None


def _retry_delay(headers, attempt: int) -> float:
    retry_after = _retry_after(headers)
    if retry_after is not None:
        return min(retry_after, MAX_RETRY_SLEEP)
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
                if exc.code == 429:
                    PACER.penalize(url, _retry_after(exc.headers))
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
                if exc.code == 429:
                    PACER.penalize(url, _retry_after(exc.headers))
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


def probe_url(url: str, timeout: float, attempts: int = 3) -> Probe:
    probe = Probe()
    # A HEAD answer that looks like a refusal is confirmed with a ranged GET:
    # several CDNs answer 403/404/429 to HEAD but serve the artifact to GET.
    status, headers, final_url, error, method = fetch_head(url, timeout, attempts)
    if status is None or status in CONFIRM_CODES:
        get_status, get_headers, data, _, get_final, get_error = fetch_range(
            url, 0, HEAD_BYTES - 1, timeout, attempts
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
        s2, h2, _, _, fut2, err2 = fetch_range(url, 0, HEAD_BYTES - 1, timeout, attempts)
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


def analyze(
    url: str, timeout: float, cap: int, cache: Cache, attempts: int = 3
) -> tuple[Probe, Artifact]:
    artifact = Artifact()
    probe = probe_url(url, timeout, attempts)
    if probe.status is None:
        artifact.notes.append(probe.error or "no response")
        return probe, artifact
    if probe.status in (404, 410):
        return probe, artifact
    if probe.status >= 400:
        artifact.notes.append(f"HTTP {probe.status} {probe.content_type}".strip())
        return probe, artifact

    head_status, head_headers, head, _, _, head_error = fetch_range(
        url, 0, HEAD_BYTES - 1, timeout, attempts
    )
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
            url, max(0, size - TAIL_BYTES), size - 1, timeout, attempts
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
                blob = download_whole(url, size, cap, timeout, attempts)
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
                blob = download_whole(url, size, cap, timeout, attempts)
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


def download_whole(url: str, size: int, cap: int, timeout: float, attempts: int = 3) -> bytes | None:
    if size > cap:
        return None
    status, headers, data, truncated, _, error = fetch_range(url, 0, size - 1, timeout, attempts)
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


def analyze_entry(
    entry: Entry, timeout: float, cap: int, cache: Cache, attempts: int = 3
) -> dict:
    probe, artifact = analyze(entry.url, timeout, cap, cache, attempts)
    return entry_result(entry, probe, artifact)


def entry_result(entry: Entry, probe: Probe, artifact: Artifact) -> dict:
    """Turn one shared (probe, artifact) pair into this entry's result record.

    The probe is derived from the URL only, so entries that share a URL share
    the probe and the artifact listing; only the declared metadata and the
    mismatches derived from it are per entry.
    """
    started = time.time()
    classification = classify(entry, probe, artifact)
    mismatches, unverified = derive_mismatches(entry, probe, artifact, classification)

    sha_state = sha256_state(entry.sha256, artifact.sha256)

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


def rerun_sha_state(result: dict) -> None:
    """Re-derive the checksum verdict of an earlier run under the current rules.

    A merged payload carries the verdict its own run computed, so an entry
    probed before a rule changed would keep the old verdict in a merged report.
    Only the digest comparison is re-derived, from the bytes and the declaration
    the earlier run recorded.
    """
    declared = (result.get("declared") or {}).get("sha256")
    served = (result.get("artifact") or {}).get("sha256")
    state = sha256_state(declared or "", served)
    if state == result.get("sha256_state"):
        return
    result["sha256_state"] = state
    if state != "mismatch":
        result["observations"] = [
            note
            for note in result.get("observations") or []
            if "does not match the served bytes" not in note
        ]


def error_result(entry: Entry, exc: Exception) -> dict:
    """Result for an entry whose analysis raised, so the run never drops one."""
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
        "probe": {"error": f"{type(exc).__name__}: {exc}"},
        "artifact": {"method": "none", "container": "unknown", "size_bytes": None},
        "classification": UNREACHABLE,
        "sha256_state": "not-computed",
        "mismatches": [],
        "unverified_reason": f"verifier error: {type(exc).__name__}: {exc}",
        "elapsed_s": 0.0,
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


def is_placeholder_sha256(sha256: str) -> bool:
    """Empty, `manual`, or all-zero: the markers apm treats as "no checksum"."""
    value = (sha256 or "").strip()
    return not value or value.lower() == "manual" or set(value) == {"0"}


def sha256_state(declared: str, served: str | None) -> str:
    """How the served bytes relate to the declared digest."""
    if served and not is_placeholder_sha256(declared):
        return "match" if served.lower() == (declared or "").lower() else "mismatch"
    return "no-declared-checksum" if served else "not-computed"


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


# classification -> the install_type the served artifact implies
PROMOTION_INSTALL_TYPE = {
    ZIP_WITH_BUNDLES: "zip",
    ZIP_WITH_PKG: "zip",
    DMG: "dmg",
    PKG: "pkg",
    ZIP_WITH_DMG: "dmg",
}


def plan_promotions(results: list[dict]) -> list[dict]:
    """Promotion proposals for `manual` records whose URL serves the artifact.

    A `manual` record is a claim that the user has to fetch the plugin by hand;
    when the record's URL actually answers with the installer, the bytes prove
    the URL is not a landing page. These stay proposals: the URL of a `manual`
    record is where apm points the user, so re-typing it changes the install
    path and belongs to the maintainer, not to the verifier.

    Each proposal carries the field changes the artifact supports and the
    blockers the artifact cannot settle (a checksum apm would need for a direct
    record, a bundle_path inside a DMG that is never mounted).
    """
    groups: dict[str, list[dict]] = {}
    for result in results:
        if result["scope"] != "manual":
            continue
        groups.setdefault(result["url"], []).append(result)

    proposals: list[dict] = []
    for url, items in sorted(groups.items()):
        fetchable = [r for r in items if r["classification"] in ARTIFACT_CLASSES]
        if not fetchable:
            continue
        classification = fetchable[0]["classification"]
        artifact = fetchable[0]["artifact"]
        observed = promotion_observation(artifact)
        changes: list[dict] = []
        blockers: list[str] = []

        def block(reason: str) -> None:
            if reason not in blockers:
                blockers.append(reason)

        for result in fetchable:
            for mismatch in result["mismatches"]:
                if mismatch["field"] == "download_type":
                    continue
                if mismatch["field"].startswith("formats."):
                    # Dropping the only declared format on a promotion would be
                    # a schema change, not a field correction.
                    block(
                        f"{result['entry_id']}: {mismatch['evidence']} "
                        "(needs a new format block, not a promotion)"
                    )
                    continue
                changes.append(
                    {
                        "entry_id": result["entry_id"],
                        "field": mismatch["field"],
                        "from": mismatch["declared"],
                        "to": mismatch["actual"],
                        "evidence": mismatch["evidence"],
                    }
                )
            if result["unverified_reason"] and not any(
                mismatch["field"].startswith("bundle_path") for mismatch in result["mismatches"]
            ):
                block(f"{result['entry_id']}: {result['unverified_reason']}")

        base = {
            "url": url,
            "entries": [r["entry_id"] for r in fetchable],
            "files": sorted({r["file"] for r in fetchable}),
            "classification": classification,
            "observed": observed,
            "changes": changes,
        }

        if classification == ZIP_WITH_DMG:
            proposals.append(
                {
                    **base,
                    "target_download_type": None,
                    "blockers": [
                        "the served archive wraps a DMG and no apm path unwraps a nested "
                        "DMG, so the record stays `manual`"
                    ],
                }
            )
            continue

        # A filename that names a vendor-wide downloader is not the plugin's own
        # artifact: the record belongs to the manager-app flow, like the records
        # already typed `managed`. This is a URL/filename signal, not the bytes.
        vendor_tool = VENDOR_TOOL_RE.search(urllib.parse.unquote(pathlib.PurePosixPath(
            urllib.parse.urlparse(url).path
        ).name))
        if vendor_tool and classification in (DMG, PKG):
            block(
                "a `managed` record needs an `installer` key in `registry/installers.toml` "
                "naming the vendor app and its `/Applications` paths; the bytes cannot "
                "supply that"
            )
            changes.append(
                {
                    "entry_id": f"{len(fetchable)} entr{'y' if len(fetchable) == 1 else 'ies'}",
                    "field": "download_type",
                    "from": "manual",
                    "to": "managed",
                    "evidence": (
                        f"URL serves {classification} whose filename names a vendor-wide "
                        f"downloader ({vendor_tool.group(0)}); {observed}"
                    ),
                }
            )
            proposals.append(
                {**base, "target_download_type": "managed", "blockers": blockers}
            )
            continue

        if not artifact.get("sha256"):
            block(
                "the served bytes were not hashed (only ZIP/PKG under the size cap are "
                "downloaded; DMGs are neither downloaded nor mounted), and a `direct` "
                "record must declare a real sha256"
            )
        if classification == DMG and not any(
            "DMG contents are not inspected" in reason for reason in blockers
        ):
            block(
                "DMG contents are not inspected (never mounted), so the declared "
                "bundle_path cannot be confirmed from the bytes"
            )
        changes.append(
            {
                "entry_id": f"{len(fetchable)} entr{'y' if len(fetchable) == 1 else 'ies'}",
                "field": "download_type",
                "from": "manual",
                "to": "direct",
                "evidence": (
                    f"URL serves {classification} ({observed}) rather than a product page"
                ),
            }
        )
        declared_sha = fetchable[0]["declared"]["sha256"]
        served_sha = artifact.get("sha256")
        if served_sha and is_placeholder_sha256(declared_sha):
            changes.insert(
                0,
                {
                    "entry_id": f"{len(fetchable)} entr{'y' if len(fetchable) == 1 else 'ies'}",
                    "field": "sha256",
                    "from": declared_sha or "<empty>",
                    "to": served_sha,
                    "evidence": (
                        f"the downloaded bytes hash to {served_sha} "
                        f"({artifact.get('size_bytes')} bytes), and a `direct` record must "
                        "declare a real checksum"
                    ),
                },
            )
        proposals.append(
            {
                **base,
                "target_download_type": "direct",
                "install_type": PROMOTION_INSTALL_TYPE[classification],
                "blockers": blockers,
            }
        )
    proposals.sort(key=lambda p: (p["target_download_type"] is None, p["url"]))
    return proposals


def promotion_observation(artifact: dict) -> str:
    size = artifact.get("size_bytes")
    parts = [
        f"container={artifact.get('container')}",
        f"size={size or '?'}",
        f"method={artifact.get('method')}",
    ]
    if artifact.get("sha256"):
        parts.append(f"sha256={artifact['sha256']}")
    return ", ".join(parts)


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


def _registrable_host(url: str) -> str:
    host = urllib.parse.urlparse(url).netloc.lower().split("@")[-1].split(":")[0]
    labels = [label for label in host.split(".") if label]
    return ".".join(labels[-3:] if len(labels) >= 3 and labels[-2] in ("co", "com", "org", "net") else labels[-2:])


def managed_evidence(repo: pathlib.Path, registry_dir: pathlib.Path, results: list[dict]) -> dict:
    """Why a `managed` URL is a vendor page by design, not a mislabelled download.

    `managed` does not claim the URL serves an artifact: it claims the plugin is
    installed through a vendor manager app. The evidence is the `installer`
    reference on the record and the app paths behind that key in
    `registry/installers.toml`.
    """
    installers_path = registry_dir.parent / "installers.toml"
    if not installers_path.exists():
        return {}
    installers = tomllib.loads(installers_path.read_text(encoding="utf-8"))

    plugin_installer: dict[str, str | None] = {}
    for path in sorted(registry_dir.rglob("*.toml")):
        if path.name in ("index.toml", "bundle_ids.toml", "installers.toml"):
            continue
        try:
            data = tomllib.loads(path.read_text(encoding="utf-8"))
        except Exception:
            continue
        sources = list((data.get("formats") or {}).values()) + [
            source
            for release in (data.get("releases") or [])
            for source in (release.get("formats") or {}).values()
        ]
        if not any((source.get("download_type") or "") == "managed" for source in sources):
            continue
        plugin_installer[data.get("slug") or path.stem] = data.get("installer")

    managed = [r for r in results if r["scope"] == "managed"]
    refs = collections.Counter()
    unknown: list[str] = []
    missing_app_paths: list[str] = []
    own_host = 0
    installer_page = 0
    checked = 0
    for result in managed:
        key = plugin_installer.get(result["slug"])
        refs[key] += 1
        if not key or key not in installers:
            unknown.append(result["slug"])
            continue
        if not installers[key].get("app_paths"):
            missing_app_paths.append(key)
        home = installers[key].get("homepage") or ""
        own = installers[key].get("download_url") or ""
        if own and result["url"].rstrip("/").lower() == own.rstrip("/").lower():
            installer_page += 1
        if home and result["url"]:
            checked += 1
            if _registrable_host(result["url"]) == _registrable_host(home):
                own_host += 1
    return {
        "plugins": len(plugin_installer),
        "entries": len(managed),
        "installers": sorted({k for k in refs if k}),
        "unbacked": sorted(set(unknown)),
        "missing_app_paths": sorted(set(missing_app_paths)),
        "installer_entries": {k: refs[k] for k in sorted(refs, key=lambda k: -refs[k]) if k},
        "vendor_host_entries": own_host,
        "installer_page_entries": installer_page,
        "host_checked_entries": checked,
        "hosts": sorted({_registrable_host(r["url"]) for r in managed if r["url"]}),
    }


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


def load_policy(path: pathlib.Path) -> dict | None:
    """Read the applied-decision record, or report why it could not be read."""
    if not path.exists():
        return None
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except Exception as exc:
        print(f"cannot read {path}: {type(exc).__name__}: {exc}", file=sys.stderr)
        return None


def render_policy_section(add, policy: dict) -> None:
    """Render the maintainer's decisions about the stale-checksum wave.

    The checksum table is evidence; how the registry answers it is a maintainer
    decision, recorded in `docs/registry-verification-policy.json`. Rendering it
    from that record keeps the report reproducible after a re-run, when the
    mismatch table it came from is empty because the fix removed it.
    """
    stale = policy.get("stale_markings") or []
    rolls = policy.get("version_rolls") or []
    applied = policy.get("promotions_applied") or []
    left = policy.get("promotions_left") or {}
    add("## Policy applied in this change")
    add("")
    add(
        "Wave 1 found the mismatches and wave 2 reported them, leaving three ways out on the "
        "table: leave them, re-verify the digest in place, or mark the record stale. "
        "Re-verifying in place is rejected for these rows — it blesses whatever the vendor "
        "served and re-versions the record while its `version` field still names the old build. "
        f"What was applied is {policy.get('decision', '')}, by hand from the recorded evidence. "
        f"The rule that separates the two cases is *{policy.get('principle', '')}*"
    )
    add("")
    if stale:
        add("### Marked stale: `download_type = \"manual\"`, `sha256 = \"manual\"`")
        add("")
        add(
            "The URL serves a same-named file whose bytes no longer match, and no digest survives "
            "the vendor's next re-package, so the record stops promising a verified download and "
            "keeps the URL, which is what `apm install` hands the user:"
        )
        add("")
        add("| URL | Entries | Declared sha256 | Served sha256 | Version | Why stale, not rolled |")
        add("| --- | --- | --- | --- | --- | --- |")
        for row in stale:
            version = row.get("artifact_version")
            declared = row.get("record_version") or ""
            version_cell = (
                f"artifact `{version}`, record `{declared}`"
                if version
                else f"record `{declared}`, artifact not decoded"
            )
            add(
                table_row(
                    [
                        f"`{short_url(row['url'])}`",
                        str(len(row.get("entries") or [])),
                        f"`{(row.get('declared_sha256') or '')[:16]}…`",
                        f"`{(row.get('served_sha256') or '')[:16]}…`",
                        version_cell,
                        row.get("note") or "",
                    ]
                )
            )
        add("")
    if rolls:
        add("### Curated as a version roll: new URL, digest and version")
        add("")
        add(
            "Here the redirect resolves to a differently-named, version-named build, so there is "
            "an artifact URL that can be pinned durably. The record was updated as a new release "
            "— URL, digest and `version` together — and legitimately returns to `direct`:"
        )
        add("")
        add("| File | Entries | URL before | URL after | Version | sha256 | How the version was established |")
        add("| --- | --- | --- | --- | --- | --- | --- |")
        for row in rolls:
            add(
                table_row(
                    [
                        f"`{row['file']}`",
                        str(len(row.get("entries") or [])),
                        f"`{short_url(row['url_before'])}`",
                        f"`{short_url(row['url_after'])}`",
                        f"`{row.get('version_before')}` -> `{row.get('version_after')}`",
                        f"`{(row.get('sha256_after') or '')[:16]}…`",
                        row.get("how_version_established") or "",
                    ]
                )
            )
        add("")
    if applied:
        add("### Promotions applied")
        add("")
        add(
            "A `manual` record whose URL serves the artifact itself, whose blockers are empty and "
            "whose served build matches the version the record declares is install coverage "
            "gained honestly: the digest is *established* where the placeholder said there was "
            "none, not changed. Any served build newer than the record would be a version roll "
            "instead (see above), not a promotion:"
        )
        add("")
        add("| URL | Entries | Version check | sha256 declared now | Version evidence |")
        add("| --- | --- | --- | --- | --- |")
        for row in applied:
            add(
                table_row(
                    [
                        f"`{short_url(row['url'])}`",
                        str(len(row.get("entries") or [])),
                        f"artifact `{row.get('artifact_version')}` = record `{row.get('declared_version')}`",
                        f"`{(row.get('sha256') or '')[:16]}…`",
                        row.get("version_evidence") or "",
                    ]
                )
            )
        add("")
    if left:
        add("### Promotion candidates left alone")
        add("")
        add(
            f"{left.get('count')} of the {len(applied) + left.get('count', 0)} proposals wave 2 "
            f"made were left as they were (the other {len(applied)} are the promotions above). "
            f"{left.get('reason', '')}"
        )
        add("")
    add(
        "The u-he records that carry placeholder digests (`sha256 = \"0000…\"`) are not part of "
        "the stale set: a placeholder means *no digest was ever declared*, so the verifier "
        "reports `no-declared-checksum` rather than a mismatch (commit `e454de3a`). They stay "
        "that way wherever the artifact's version could not be confirmed; the three whose "
        "blockers were empty were promoted above, which is a pin established, not a pin changed."
    )
    add("")
    add(
        "One consequence of the stale markings is visible in the promotion tables above: those "
        "records now declare `download_type = \"manual\"` and their URL still answers with the "
        "artifact, so the verifier proposes promoting them back. That proposal is the tool "
        "answering from the bytes alone; the decision above stands, because re-pinning is the "
        "silent re-versioning this policy refuses."
    )
    add("")
    add(
        f"Records were changed by hand from this evidence, not by `--apply`: "
        f"{len(stale)} URL{'s' if len(stale) != 1 else ''} marked stale, "
        f"{len(rolls)} curated as a version roll, {len(applied)} promoted "
        f"({sum(len(r.get('entries') or []) for r in stale)} + "
        f"{sum(len(r.get('entries') or []) for r in rolls)} + "
        f"{sum(len(r.get('entries') or []) for r in applied)} format entries). This record is "
        "`docs/registry-verification-policy.json`, which is what this section renders, so a "
        "re-run reproduces it instead of losing it with the mismatch table."
    )
    add("")


def write_report(
    path: pathlib.Path,
    payload: dict,
    applied_corrections: list[dict] | None,
    policy: dict | None = None,
):

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
        f"`{payload['timeout']}s`, concurrency `{payload['concurrency']}`, "
        f"{payload.get('attempts') or 3} attempts per request, host spacing "
        f"`{payload.get('host_interval') or 0}s`, spacing after a `429` "
        f"`{payload.get('pace_on_429') or 0}s`"
    )
    add("- Nothing is installed, no DMG is mounted, no privileged command is run.")
    if payload.get("merged_from"):
        for merged in payload["merged_from"]:
            scopes = ", ".join(f"`{scope}`" for scope in merged.get("scopes") or []) or "?"
            add(
                f"- Merged from `{merged['path']}` ({scopes}, {merged['entries']} entries, "
                f"generated `{merged['generated_at']}`): cap `{merged['max_bytes']}` bytes, "
                f"timeout `{merged['timeout']}s`, concurrency `{merged['concurrency']}`, "
                f"{merged.get('attempts') or 3} attempts, host spacing "
                f"`{merged.get('host_interval') or 0}s`, spacing after a `429` "
                f"`{merged.get('pace_on_429') or 0}s`"
            )
    add(
        f"- This run proposes {len(payload['corrections'])} field corrections, all on "
        "`download_type = \"direct\"` records — the only scope `--apply` rewrites, because a "
        "`direct` record claims apm can fetch the URL itself. A run after `--apply` should "
        "propose none for the fields it already fixed."
    )
    promotions = payload.get("promotions") or []
    if promotions:
        proposable = [p for p in promotions if p["target_download_type"]]
        applied_promotions = len((policy or {}).get("promotions_applied") or [])
        add(
            f"- It also lists {len(promotions)} `manual` record"
            f"{'' if len(promotions) == 1 else 's'} whose URL answers with an artifact; "
            f"{len(proposable)} {'is a' if len(proposable) == 1 else 'are'} promotion "
            f"candidate{'' if len(proposable) == 1 else 's'}, of which "
            f"{applied_promotions} {'was' if applied_promotions == 1 else 'were'} applied in this "
            "change; the rest are reported, with their blockers where the bytes cannot settle "
            "one (see *Policy applied in this change*)."
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
    add(
        "- Rate limits and transient server errors (408/425/429/5xx) are retried up to three "
        "times, honouring `Retry-After` when the host sends one; a `HEAD` that a host rejects is "
        "confirmed with a ranged `GET`."
    )
    add(
        "- A host that answers `429` is spaced out from then on (`--pace-on-429`, default 3s): a "
        "rate limiter that rejects every request in a burst cannot be retried out of, so the "
        "burst is what has to change. `--host-interval` applies the same spacing to every host."
    )
    add(
        "- One probe is made per distinct URL, and every format entry pointing at that URL reuses "
        "it: a plugin's formats and releases share one artifact, and a vendor page is often the "
        "download page for a whole catalogue."
    )
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
    add(
        "| URL serving HTML/404 on a `direct` record | type unchanged; `download_type` becomes "
        "`manual` | nothing to install |"
    )
    add(
        "| URL serving HTML on a `managed` record | unchanged | `managed` means the vendor "
        "manager app installs it, which the record's `installer` declares |"
    )
    add("")
    evidence = payload.get("managed_evidence")
    if evidence and evidence.get("entries"):
        add("### Why the `managed` wave is reported but never retyped")
        add("")
        add(
            "`download_type = \"managed\"` does not claim the URL serves an artifact; it claims "
            "the plugin is installed through a vendor manager app. Every managed record here is "
            "backed by an `installer` key in `registry/installers.toml` that carries that app's "
            "`/Applications` paths, and the URL is the vendor's own page, so an HTML answer "
            "confirms the record instead of refuting it. Retyping one to `manual` would drop the "
            "vendor-manager handoff that makes the record installable:"
        )
        add("")
        add(
            f"- managed format entries: {evidence['entries']} across {evidence['plugins']} plugins"
        )
        add(
            f"- entries whose plugin declares an installer key present in `installers.toml` with "
            f"non-empty `app_paths`: {evidence['entries'] - len(evidence['unbacked'])} of "
            f"{evidence['entries']}"
        )
        if evidence["unbacked"]:
            add(
                "- managed entries with no usable installer key (candidates for retyping): "
                + ", ".join(f"`{slug}`" for slug in evidence["unbacked"][:40])
            )
        add(
            f"- entries whose URL host is the vendor's own host (as declared by the installer's "
            f"`homepage`): {evidence['vendor_host_entries']} of {evidence['host_checked_entries']}"
        )
        add(
            f"- entries whose URL is exactly the page `installers.toml` lists as the installer's "
            f"`download_url`: {evidence['installer_page_entries']}; the rest are that vendor's "
            "product or support pages for the individual plugin"
        )
        add(
            "- installer keys in play: "
            + ", ".join(f"`{key}` ({count})" for key, count in evidence["installer_entries"].items())
        )
        add("")
        add(
            "A managed URL that 404s is a broken pointer in a vendor page, not proof that the "
            "plugin has no artifact, so it is reported under per-entry status and left alone."
        )
        add("")
    add("## Priority order")
    add("")
    add("| Wave | Scope | Entries | Unique URLs | Unique hosts | What was done |")
    add("| --- | --- | --- | --- | --- | --- |")
    scope_urls = payload["scope_urls"]
    ran = [scope for scope in SCOPE_ORDER if totals["by_scope"].get(scope, 0)]
    wave_notes = {
        "direct": "every unique URL downloaded and inspected under the cap",
        "managed": (
            "every unique URL probed; artifacts listed/inspected under the cap. No rewrite: the "
            "URL is the vendor page the manager-app flow opens, not the artifact apm installs"
        ),
        "manual": (
            "every unique URL probed; the ones that answer with an artifact are listed as "
            "promotion candidates, and the promotions among them were applied by hand (see "
            "*Policy applied in this change*)"
        ),
    }
    for index, scope in enumerate(ran, start=1):
        hosts = {
            urllib.parse.urlparse(r["url"]).netloc
            for r in results
            if r["scope"] == scope and r["url"]
        }
        add(
            f"| {index} | `download_type = \"{scope}\"` | {totals['by_scope'][scope]} | "
            f"{scope_urls.get(scope, 0)} | {len(hosts)} | {wave_notes[scope]} |"
        )
    for scope in SCOPE_ORDER:
        if scope not in ran:
            add(
                f"| – | `download_type = \"{scope}\"` | not run | – | – | "
                "re-run with `--scope all` (`managed`/`manual` are probe-only waves) |"
            )
    add("")
    manual_scoped = [r for r in results if r["scope"] == "manual"]
    if manual_scoped:
        manual_fetchable = [r for r in manual_scoped if r["classification"] in ARTIFACT_CLASSES]
        add(
            f"{len(manual_fetchable)} of {len(manual_scoped)} `manual` entries already point at a "
            "fetchable artifact (an archive or installer rather than a product page). They are "
            "listed under per-entry status as promotion candidates; the ones whose blockers were "
            "empty and whose served version matches the record are promoted in *Policy applied in "
            "this change*, and the rest are reported."
        )
        add("")
    add("## Promotion candidates in the `manual` wave")
    add("")
    add(
        "`download_type = \"manual\"` says the user fetches the plugin by hand; a record whose URL "
        "answers with the installer itself is mislabelled, and the returned bytes say so. These "
        "are proposals: the URL of a `manual` record is where apm sends the user, so retyping it "
        "changes the install path and belongs to the maintainer. The proposals whose blockers "
        "were empty were applied by hand in this change (*Policy applied in this change*); the "
        "rest are listed with their blockers. Candidates are probed with the same limits as the "
        "rest of the run, so a blocker that names the download cap or the DMG rule needs a "
        "different pass, not a different judgment."
    )
    add("")
    promotions = payload.get("promotions") or []
    if not promotions:
        add("No `manual` URL served a fetchable artifact in this run.")
        add("")
    else:
        proposable = [p for p in promotions if p["target_download_type"]]
        held = [p for p in promotions if not p["target_download_type"]]
        add(
            f"{len(promotions)} distinct URLs serve an artifact. {len(proposable)} are promotion "
            f"candidates; {len(held)} cannot become an apm install target at all."
        )
        add("")
        add("| URL | Entries | Class | Proposed `download_type` | Observed | Field changes |")
        add("| --- | --- | --- | --- | --- | --- |")
        for proposal in proposable:
            changes = "; ".join(
                f"`{change['entry_id']}` `{change['field']}` {change['from']} -> {change['to']}"
                for change in proposal["changes"]
            )
            add(
                table_row(
                    [
                        f"`{short_url(proposal['url'])}`",
                        str(len(proposal["entries"])),
                        f"`{proposal['classification']}`",
                        f"`{proposal['target_download_type']}`",
                        proposal["observed"],
                        changes or "`download_type` only",
                    ]
                )
            )
        add("")
        blocked = [p for p in proposable if p["blockers"]]
        if blocked:
            add("### Promotion candidates with a blocker")
            add("")
            add(
                "The bytes prove the URL serves the artifact, but not everything a `direct` "
                "record must declare. These need a decision or a deeper read before promotion:"
            )
            add("")
            add("| URL | Class | Blocker |")
            add("| --- | --- | --- |")
            for proposal in blocked:
                for blocker in proposal["blockers"]:
                    add(
                        table_row(
                            [
                                f"`{short_url(proposal['url'])}`",
                                f"`{proposal['classification']}`",
                                blocker,
                            ]
                        )
                    )
            add("")
        if held:
            add("### Archive-shaped URLs that stay `manual`")
            add("")
            add("| URL | Class | Why it is not an apm install target |")
            add("| --- | --- | --- |")
            for proposal in held:
                add(
                    table_row(
                        [
                            f"`{short_url(proposal['url'])}`",
                            f"`{proposal['classification']}`",
                            "; ".join(proposal["blockers"]),
                        ]
                    )
                )
            add("")
    archive_shaped = [
        r
        for r in manual_scoped
        if r["url"].lower().split("?")[0].endswith(ARCHIVE_EXTS)
        and r["classification"] not in ARTIFACT_CLASSES
    ]
    if archive_shaped:
        seen: set[str] = set()
        add("### Archive-shaped URLs that do not serve an artifact")
        add("")
        add(
            "These `manual` URLs end in an archive extension but answered with something else, so "
            "they stay `manual` and were not rewritten:"
        )
        add("")
        add("| URL | Class | HTTP | Final URL |")
        add("| --- | --- | --- | --- |")
        for result in archive_shaped:
            if result["url"] in seen:
                continue
            seen.add(result["url"])
            probe = result["probe"]
            final = probe.get("final_url") or ""
            add(
                table_row(
                    [
                        f"`{short_url(result['url'])}`",
                        f"`{result['classification']}`",
                        str(probe.get("status") or probe.get("error") or "-"),
                        f"`{short_url(final)}`" if final and final != result["url"] else "-",
                    ]
                )
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
    add(
        f"- `no-declared-checksum`: {sha.get('no-declared-checksum', 0)} entries whose declared "
        "`sha256` is a placeholder (empty, `manual`, or all zeros) — the vocabulary "
        "`is_placeholder_sha256` recognises, so there is nothing to compare"
    )
    add(f"- `not-computed`: {sha.get('not-computed', 0)} entries where no bytes were hashed")
    add("")
    stale = [r for r in results if r["sha256_state"] == "mismatch"]
    if stale:
        add("### Entries whose declared checksum no longer matches the served bytes")
        add("")
        add(
            "`apm` verifies the digest after downloading and deletes the archive on a mismatch "
            "(`ApmError::Checksum`, `crates/apm-core/src/engine/install_download.rs`), so these "
            "records cannot install today. The served bytes are real; the declared digest is for "
            "an older build of the same URL."
        )
        add("")
        add(
            "| URL | Entries | Declared sha256 | Served sha256 | Size | Resolved URL |"
        )
        add("| --- | --- | --- | --- | --- | --- |")
        by_url: dict[str, list[dict]] = {}
        for result in stale:
            by_url.setdefault(result["url"], []).append(result)
        for url, items in sorted(by_url.items()):
            first = items[0]
            artifact = first["artifact"]
            probe = first["probe"]
            final = probe.get("final_url") or ""
            add(
                table_row(
                    [
                        f"`{short_url(url)}`",
                        str(len(items)),
                        f"`{(first['declared']['sha256'] or '')[:16]}…`",
                        f"`{(artifact.get('sha256') or '')[:16]}…`",
                        str(artifact.get("size_bytes") or "-"),
                        f"`{short_url(final)}`"
                        if final and final != url
                        else "`" + short_url(url) + "` (no redirect)",
                    ]
                )
            )
        add("")
        add(
            "How the registry answers these rows — the three options wave 2 weighed and the one "
            "applied — is recorded under *Policy applied in this change* below."
        )
        add("")

    if policy:
        render_policy_section(add, policy)
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
        if not payload["corrections"]:
            add(
                "This pass applied no new field corrections from a probe: every `direct` record "
                "already matches the bytes it serves. The policy decisions it did apply — the "
                "stale markings, the version rolls and the promotions — are recorded under "
                "*Policy applied in this change*. The table below is the record of the "
                "corrections the first probe pass applied."
            )
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
        "A probe of the URL cannot retype a `managed` or `manual` record: a `managed` record is "
        "installed by the vendor's manager app through its `installer` entry (see above), and a "
        "`manual` URL is where apm sends the user rather than what apm installs. These mismatches "
        f"are therefore reported and deliberately left unchanged ({len(deferred)} fields)."
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

    unreachable = [r for r in results if r["classification"] == UNREACHABLE]
    if unreachable:
        add("### Unreachable URLs by host and status")
        add("")
        add(
            "A probe that never got a usable answer says nothing about the record, so no field "
            "was touched for these. Retries have already been applied; what is left is the host's "
            "final answer:"
        )
        add("")
        add("| Host | Final answer | Entries | Example URL |")
        add("| --- | --- | --- | --- |")
        buckets: dict[tuple[str, str], list[dict]] = collections.defaultdict(list)
        for result in unreachable:
            probe = result["probe"]
            answer = probe.get("status")
            answer = f"HTTP {answer}" if answer else (probe.get("error") or "no response")
            host = urllib.parse.urlparse(result["url"]).netloc or "<no host>"
            buckets[(host, answer)].append(result)
        ordered = sorted(buckets.items(), key=lambda kv: (-len(kv[1]), kv[0]))
        shown = 0
        for (host, answer), items in ordered:
            if shown >= 60:
                add(f"| … | | {len(unreachable) - shown} entries in {len(ordered) - shown} more host/answer pairs | |")
                break
            add(
                table_row(
                    [
                        f"`{host}`",
                        answer,
                        str(len(items)),
                        f"`{short_url(items[0]['url'])}`",
                    ]
                )
            )
            shown += len(items)
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
            listed = [r for r in scoped if r["classification"] in ARTIFACT_CLASSES]
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
    parser.add_argument(
        "--attempts",
        type=int,
        default=3,
        help="attempts per request before a URL is called unreachable (default 3)",
    )
    parser.add_argument(
        "--pace-on-429",
        type=float,
        default=3.0,
        help=(
            "seconds between requests to a host after it answers HTTP 429 (0 disables). A "
            "rate-limited host cannot be retried out of a burst; it has to be spaced"
        ),
    )
    parser.add_argument(
        "--host-interval",
        type=float,
        default=0.0,
        help=(
            "minimum seconds between requests to the same host (0 = no pacing). Hosts that "
            "answer a burst with HTTP 429 need spacing; retrying harder does not help"
        ),
    )
    parser.add_argument("--cache-dir", default=None)
    parser.add_argument("--out-json", default=None)
    parser.add_argument("--out-report", default=None)
    parser.add_argument("--applied-corrections", default=None)
    parser.add_argument(
        "--policy-json",
        default=None,
        help=(
            "record of the maintainer's decisions about the stale-checksum wave, rendered into "
            "the report (default `docs/registry-verification-policy.json`). A decision is not "
            "something a probe can re-derive, so it lives beside the evidence rather than in it"
        ),
    )
    parser.add_argument(
        "--scraped-source",
        default=None,
        help="secondary (scraped) registry checkout to compare against, for the precedence section",
    )
    parser.add_argument(
        "--merge-json",
        action="append",
        default=None,
        help=(
            "another run's payload to fold into this report (repeatable). Results are merged by "
            "entry id, with the current run winning, so the three waves can be probed as separate "
            "runs and reported together"
        ),
    )
    parser.add_argument("--limit", type=int, default=0)
    parser.add_argument(
        "--url-contains",
        action="append",
        default=None,
        help="only probe URLs containing this substring (repeatable; any match)",
    )
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
    policy_path = (
        pathlib.Path(args.policy_json)
        if args.policy_json
        else repo / "docs" / "registry-verification-policy.json"
    )
    scraped_source = (
        pathlib.Path(args.scraped_source) if args.scraped_source else repo / "data" / "registry"
    )

    global PACER
    PACER = HostPacer(args.host_interval, args.pace_on_429)

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
        selected = [e for e in selected if any(part in e.url for part in args.url_contains)]
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

    # Every entry sharing a URL shares its probe and artifact listing: several
    # formats (and several releases) of one plugin point at one artifact, and a
    # vendor page is often the download page for a whole catalogue. Grouping by
    # URL keeps the network work proportional to distinct URLs without changing
    # what is measured.
    groups: dict[str, list[Entry]] = {}
    for entry in selected:
        groups.setdefault(entry.url, []).append(entry)

    def analyze_group(url: str, entries: list[Entry]) -> list[dict]:
        probe, artifact = analyze(url, args.timeout, args.max_bytes, cache, args.attempts)
        return [entry_result(entry, probe, artifact) for entry in entries]

    with concurrent.futures.ThreadPoolExecutor(max_workers=max(1, args.concurrency)) as pool:
        futures = {
            pool.submit(analyze_group, url, entries): entries
            for url, entries in groups.items()
        }
        done = 0
        for future in concurrent.futures.as_completed(futures):
            entries = futures[future]
            try:
                results.extend(future.result())
            except Exception as exc:  # pragma: no cover - defensive
                results.extend(error_result(entry, exc) for entry in entries)
            done += len(entries)
            if done % 100 < len(entries) or done == len(selected):
                print(f"  {done}/{len(selected)} entries", flush=True)

    results.sort(key=lambda r: (SCOPE_ORDER.index(r["scope"]), r["file"], r["entry_id"]))

    # Fold in earlier runs' results (one wave each), current run winning.
    merged_from: list[dict] = []
    for merge_path in args.merge_json or []:
        path = pathlib.Path(merge_path)
        try:
            earlier = json.loads(path.read_text(encoding="utf-8"))
        except Exception as exc:
            print(f"cannot merge {path}: {type(exc).__name__}: {exc}", file=sys.stderr)
            continue
        known = {r["entry_id"] for r in results}
        added = 0
        for result in earlier.get("results") or []:
            if result.get("entry_id") in known:
                continue
            known.add(result["entry_id"])
            rerun_sha_state(result)
            results.append(result)
            added += 1
        merged_from.append(
            {
                "path": str(path),
                "scopes": earlier.get("scopes") or [],
                "generated_at": earlier.get("generated_at") or "",
                "max_bytes": earlier.get("max_bytes"),
                "timeout": earlier.get("timeout"),
                "concurrency": earlier.get("concurrency"),
                "host_interval": earlier.get("host_interval"),
                "pace_on_429": earlier.get("pace_on_429"),
                "attempts": earlier.get("attempts"),
                "entries": len(earlier.get("results") or []),
            }
        )
        print(f"merged {added} entries from {path}", flush=True)
    if merged_from:
        results.sort(key=lambda r: (SCOPE_ORDER.index(r["scope"]), r["file"], r["entry_id"]))
        scope_urls = {
            scope: len({r["url"] for r in results if r["scope"] == scope}) for scope in SCOPE_ORDER
        }

    corrections = plan_corrections(results)
    corrections, deferred_removals = split_unsafe_removals(corrections, results)
    promotions = plan_promotions(results)
    payload = {
        "tool_version": TOOL_VERSION,
        "generated_at": _dt.datetime.now(_dt.timezone.utc).isoformat(timespec="seconds"),
        "registry_root": str(registry_dir),
        "scopes": sorted(scopes),
        "max_bytes": args.max_bytes,
        "timeout": args.timeout,
        "concurrency": args.concurrency,
        "host_interval": args.host_interval,
        "pace_on_429": args.pace_on_429,
        "attempts": args.attempts,
        "scope_urls": scope_urls,
        "merged_from": merged_from,
        "scraped_source": scraped_source_stats(repo, registry_dir, scraped_source),
        "managed_evidence": managed_evidence(repo, registry_dir, results),
        "totals": build_totals(results),
        "corrections": corrections,
        "corrections_not_applied": deferred_removals,
        "promotions": promotions,
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

    write_report(out_report, payload, applied_corrections, load_policy(policy_path))
    print(f"wrote {out_report}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
