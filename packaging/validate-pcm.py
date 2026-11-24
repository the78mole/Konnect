#!/usr/bin/env python3
"""Validate KopperDigger's PCM packaging against KiCAD's addon schema.

Two modes, combinable:

  python packaging/validate-pcm.py --metadata packaging/metadata.json
      Validate a metadata.json file against the vendored packages.v1 schema.

  python packaging/validate-pcm.py --zip dist/kidig-mcp-pcm-v0.1.2.zip
      Assert the PCM zip structure (metadata at root, plugin launcher,
      per-OS entrypoint binary, icons) and validate the embedded metadata.

Exits non-zero on any failure. This is the gate that would have caught the
v0.1.1 install failures (#4/#8: missing author.contact) and the schema-invalid
license/sha fields found while building it.

Requires: pip install jsonschema
"""

import argparse
import json
import sys
import zipfile
from pathlib import Path

import jsonschema

SCHEMA_PATH = Path(__file__).parent / "schema" / "packages.v1.schema.json"

REQUIRED_ZIP_ENTRIES = [
    "metadata.json",
    "plugins/__init__.py",
    "plugins/plugin.json",
    "plugins/settings_dialog.py",
    "plugins/native_bridge.py",
    "plugins/resources/icon.png",
    "resources/icon.png",
]

# Executable formats, by the magic bytes each platform's loader requires. A
# package bundles a native binary, so its declared platform is checkable rather
# than a promise: a Windows PE in a package declaring "macos" is unrunnable.
EXECUTABLE_MAGIC = {
    "windows": [b"MZ"],
    "linux": [b"\x7fELF"],
    "macos": [
        b"\xcf\xfa\xed\xfe",  # Mach-O 64-bit, little-endian
        b"\xce\xfa\xed\xfe",  # Mach-O 32-bit, little-endian
        b"\xca\xfe\xba\xbe",  # universal ("fat") binary
        b"\xbe\xba\xfe\xca",  # universal, byte-swapped
    ],
}


def identify_executable(blob: bytes) -> str | None:
    """Name the platform whose loader can run `blob`, or None if unrecognized."""
    for platform, magics in EXECUTABLE_MAGIC.items():
        if any(blob.startswith(m) for m in magics):
            return platform
    return None


def load_schema():
    return json.loads(SCHEMA_PATH.read_text(encoding="utf-8"))


def validate_metadata(meta: dict, label: str) -> list[str]:
    errors = []
    try:
        jsonschema.validate(meta, load_schema())
    except jsonschema.ValidationError as e:
        errors.append(f"{label}: schema violation at {e.json_path}: {e.message}")
    return errors


def validate_zip(zip_path: Path) -> list[str]:
    errors = []
    z = zipfile.ZipFile(zip_path)
    names = set(z.namelist())

    for entry in REQUIRED_ZIP_ENTRIES:
        if entry not in names:
            errors.append(f"{zip_path.name}: missing required entry {entry}")

    # Exactly one server binary must be present, and plugin.json's entrypoint
    # must point at it (PR #7's per-OS stamping contract).
    binaries = [n for n in names
                if n.startswith("plugins/bin/") and n.rsplit("/", 1)[-1].startswith("kidig-mcp")]
    if not binaries:
        errors.append(f"{zip_path.name}: no plugins/bin/kidig-mcp* binary found")

    if "plugins/plugin.json" in names:
        plugin = json.loads(z.read("plugins/plugin.json"))
        for action in plugin.get("actions", []):
            ep = action.get("entrypoint", "")
            if ep.startswith("bin/") and f"plugins/{ep}" not in names:
                errors.append(
                    f"{zip_path.name}: plugin.json entrypoint '{ep}' "
                    f"not present in the zip"
                )

    if "metadata.json" in names:
        meta = json.loads(z.read("metadata.json"))
        errors += validate_metadata(meta, f"{zip_path.name}:metadata.json")
        # Empty-string download fields pass nothing; they must be real or absent.
        for v in meta.get("versions", []):
            for field in ("download_sha256", "download_url"):
                if v.get(field) == "":
                    errors.append(
                        f"{zip_path.name}: {field} is an empty string — "
                        f"omit the field or provide a real value"
                    )

        errors += validate_platforms(z, meta, binaries, zip_path.name)

    return errors


def validate_platforms(z, meta: dict, binaries: list[str], label: str) -> list[str]:
    """Every version must declare exactly the platforms its binaries run on.

    Omitting `platforms` means "all platforms" to KiCAD's PCM. That is only
    true of the all-platform package, which carries bin/<platform>/ for each
    of windows, macos and linux. A single-platform package must declare its
    one platform — that is how a Windows-only package came to be offered to
    macOS and Linux users.
    """
    errors = []
    for v in meta.get("versions", []):
        declared = v.get("platforms")
        if not declared:
            errors += validate_all_platforms(z, binaries, label, v.get("version"))
            continue
        if len(declared) != 1:
            errors.append(
                f"{label}: version {v.get('version')} declares platforms "
                f"{declared}; a package bundles one platform's binaries"
            )
            continue

        # The declaration must match what is actually in the zip.
        for name in binaries:
            actual = identify_executable(z.read(name)[:4])
            if actual is None:
                errors.append(f"{label}: {name} is not a recognized executable")
            elif actual != declared[0]:
                errors.append(
                    f"{label}: declares platforms {declared} but {name} is a "
                    f"{actual} executable"
                )
    return errors


def validate_all_platforms(z, binaries: list[str], label: str, version) -> list[str]:
    """An all-platform package: each OS's binaries in bin/<platform>/, nothing loose."""
    errors = []
    bin_files = [n for n in z.namelist()
                 if n.startswith("plugins/bin/") and not n.endswith("/")]
    for platform in EXECUTABLE_MAGIC:
        exe = "kidig-mcp.exe" if platform == "windows" else "kidig-mcp"
        if f"plugins/bin/{platform}/{exe}" not in binaries:
            errors.append(
                f"{label}: version {version} declares no 'platforms' (= all), "
                f"but plugins/bin/{platform}/{exe} is missing"
            )
    for name in bin_files:
        parts = name.split("/")
        if len(parts) != 4 or parts[2] not in EXECUTABLE_MAGIC:
            errors.append(
                f"{label}: {name} is outside bin/<platform>/ in an "
                f"all-platform package"
            )
            continue
        actual = identify_executable(z.read(name)[:4])
        if actual != parts[2]:
            errors.append(
                f"{label}: {name} is a {actual or 'unrecognized'} file, "
                f"not a {parts[2]} executable"
            )
    return errors


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--metadata", type=Path, help="metadata.json to validate")
    ap.add_argument("--zip", type=Path, help="PCM zip to validate")
    args = ap.parse_args()

    if not args.metadata and not args.zip:
        ap.error("provide --metadata and/or --zip")

    errors: list[str] = []
    if args.metadata:
        meta = json.loads(args.metadata.read_text(encoding="utf-8"))
        errors += validate_metadata(meta, str(args.metadata))
    if args.zip:
        errors += validate_zip(args.zip)

    if errors:
        for e in errors:
            print(f"FAIL: {e}", file=sys.stderr)
        return 1

    checked = " and ".join(
        str(p) for p in (args.metadata, args.zip) if p is not None
    )
    print(f"OK: {checked} passed PCM validation")
    return 0


if __name__ == "__main__":
    sys.exit(main())
