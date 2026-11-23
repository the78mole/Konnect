#!/usr/bin/env python3
"""Merge the per-platform PCM zips into one package for every platform.

KiCad's official addon repository allows exactly one download per version
number, and KiCad's PCM client looks versions up by their version string, so
three zips that all say "0.14.0" cannot be listed side by side. This script
takes the Windows, macOS and Linux zips that build-pcm.ps1 / build-pcm.sh
produce and folds them into a single package:

    metadata.json                 one version entry, no "platforms" (= all)
    plugins/
      __init__.py, settings_dialog.py, native_bridge.py, plugin.json, ...
      resources/icon.png
      bin/windows/kidig-mcp.exe   (+ schematic-viewer.exe)
      bin/macos/kidig-mcp         universal Mach-O (+ schematic-viewer)
      bin/linux/kidig-mcp         ELF (+ schematic-viewer)
    resources/icon.png

plugins/__init__.py picks bin/<platform>/ at load time and points the KiCad 10
IPC action (plugin.json) at the same binary.

Besides the zip it writes `<zip>.pcm-version.json`: the version entry with
download_url / download_sha256 / download_size / install_size filled in. The
release workflow publishes it next to the zip; the self-hosted PCM repository
and the kicad-addons submission metadata are built from these files.

Usage:
  python packaging/build-pcm-universal.py --version 0.14.0 \\
      --windows dist/kidig-mcp-pcm-v0.14.0-windows.zip \\
      --macos   dist/kidig-mcp-pcm-v0.14.0-macos.zip \\
      --linux   dist/kidig-mcp-pcm-v0.14.0-linux.zip \\
      [--status stable|testing] [--out dist] [--download-base URL]
"""

import argparse
import hashlib
import json
import sys
import zipfile
from pathlib import Path

PLATFORMS = ("windows", "macos", "linux")
# Default IPC entrypoint stamped into plugin.json. __init__.py rewrites it for
# the running platform; Windows is the default because it is the platform
# where a wrong entrypoint is most likely to be noticed by users first.
DEFAULT_ENTRYPOINT = "bin/windows/kidig-mcp.exe"
# Fixed timestamp keeps the zip byte-identical for identical inputs.
ZIP_DATE = (2020, 1, 1, 0, 0, 0)
REPO = "https://github.com/the78mole/KopperDigger"


def read_platform_zip(path: Path, platform: str):
    """Return (shared_files, binaries) from one per-platform package."""
    shared, binaries = {}, {}
    with zipfile.ZipFile(path) as z:
        for info in z.infolist():
            name = info.filename.replace("\\", "/")  # PS5 Compress-Archive
            if name.endswith("/"):
                continue
            data = z.read(info)
            if name.startswith("plugins/bin/"):
                binaries[name[len("plugins/bin/"):]] = data
            else:
                shared[name] = data
    if not any(n.startswith("kidig-mcp") for n in binaries):
        sys.exit(f"{path}: no plugins/bin/kidig-mcp* binary in {platform} package")
    return shared, binaries


def add(z: zipfile.ZipFile, name: str, data: bytes, executable: bool = False):
    info = zipfile.ZipInfo(name, date_time=ZIP_DATE)
    info.create_system = 3  # unix, so the mode below is honoured on extraction
    info.external_attr = (0o100755 if executable else 0o100644) << 16
    info.compress_type = zipfile.ZIP_DEFLATED
    z.writestr(info, data, compresslevel=9)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--version", required=True)
    for p in PLATFORMS:
        ap.add_argument(f"--{p}", type=Path, required=True, help=f"{p} PCM zip")
    ap.add_argument("--status", default="stable",
                    choices=["stable", "testing", "development", "deprecated"])
    ap.add_argument("--out", type=Path, default=Path("dist"))
    ap.add_argument("--download-base", default=None,
                    help="URL prefix of the release assets "
                         f"(default {REPO}/releases/download/v<version>)")
    args = ap.parse_args()

    per_platform = {p: read_platform_zip(getattr(args, p), p) for p in PLATFORMS}

    # Shared files come from the Linux package; plugin.json differs per
    # platform only in its entrypoint, which is restamped below. Everything
    # else must be identical, or the packages were built from different trees.
    shared = dict(per_platform["linux"][0])
    for p in ("windows", "macos"):
        other = per_platform[p][0]
        for name in set(shared) | set(other):
            if name in ("metadata.json", "plugins/plugin.json"):
                continue
            # Windows runners check out with CRLF line endings; that alone is
            # not a difference in content.
            a = (shared.get(name) or b"").replace(b"\r\n", b"\n")
            b = (other.get(name) or b"").replace(b"\r\n", b"\n")
            if (name in shared) != (name in other) or a != b:
                sys.exit(f"{name} differs between the linux and {p} packages")

    plugin = json.loads(shared["plugins/plugin.json"])
    for action in plugin.get("actions", []):
        if action.get("entrypoint", "").startswith("bin/"):
            action["entrypoint"] = DEFAULT_ENTRYPOINT
    shared["plugins/plugin.json"] = (json.dumps(plugin, indent=2) + "\n").encode()

    meta = json.loads(shared.pop("metadata.json"))
    version = dict(meta["versions"][0])
    for field in ("platforms", "download_url", "download_sha256",
                  "download_size", "install_size"):
        version.pop(field, None)
    version["version"] = args.version
    version["status"] = args.status

    files = dict(shared)
    for p in PLATFORMS:
        for name, data in per_platform[p][1].items():
            files[f"plugins/bin/{p}/{name}"] = data
    # KiCad's addon CI counts every file in the zip, metadata.json included,
    # and metadata.json holds install_size itself: settle on a fixed point.
    meta["versions"] = [version]
    content_size = sum(len(d) for d in files.values())
    install_size = content_size
    for _ in range(5):
        version["install_size"] = install_size
        metadata_bytes = (json.dumps(meta, indent=2) + "\n").encode()
        if content_size + len(metadata_bytes) == install_size:
            break
        install_size = content_size + len(metadata_bytes)

    args.out.mkdir(parents=True, exist_ok=True)
    zip_name = f"kidig-mcp-pcm-v{args.version}.zip"
    zip_path = args.out / zip_name
    with zipfile.ZipFile(zip_path, "w") as z:
        add(z, "metadata.json", metadata_bytes)
        for name in sorted(files):
            add(z, name, files[name], executable=name.startswith("plugins/bin/"))

    blob = zip_path.read_bytes()
    base = args.download_base or f"{REPO}/releases/download/v{args.version}"
    entry = dict(version)
    entry["download_url"] = f"{base.rstrip('/')}/{zip_name}"
    entry["download_sha256"] = hashlib.sha256(blob).hexdigest()
    entry["download_size"] = len(blob)
    entry_path = args.out / f"{zip_name}.pcm-version.json"
    entry_path.write_text(json.dumps(entry, indent=2) + "\n", encoding="utf-8")

    print(f"PCM package (all platforms): {zip_path}")
    for k in ("download_size", "install_size", "download_sha256", "download_url"):
        print(f"  {k + ':':17} {entry[k]}")
    print(f"Version entry: {entry_path}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
