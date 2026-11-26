#!/usr/bin/env python3
"""Stamp a release version into the Cargo and Tauri manifests before a build.

Releases are versioned by their git tag (the Version workflow computes it from
the commit messages), not by a commit that edits Cargo.toml. The release
workflow runs this right before `cargo build --locked` so the binaries report
the tag's version. It edits, in place:

  Cargo.toml                         [workspace.package] version
  Cargo.lock                         every workspace crate (no `source`) at the old version
  crates/schematic-viewer/Cargo.toml [package] version
  crates/schematic-viewer/Cargo.lock its path crates at the old version
  crates/schematic-viewer/tauri.conf.json  "version"

Usage:
  python packaging/set-version.py 0.15.0        # or v0.15.0, 0.15.0-rc.1
"""

import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SEMVER = re.compile(r"^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$")


def set_manifest_version(path: Path, section: str, new: str) -> str:
    """Replace `version = "..."` inside [section]; return the old version."""
    text = path.read_text(encoding="utf-8")
    pattern = re.compile(
        rf'(^\[{re.escape(section)}\]\n(?:(?!\[).*\n)*?version = ")([^"]+)(")',
        re.MULTILINE)
    m = pattern.search(text)
    if not m:
        sys.exit(f"{path}: no version in [{section}]")
    path.write_text(text[:m.start(2)] + new + text[m.end(2):], encoding="utf-8")
    return m.group(2)


def set_lock_versions(path: Path, old: set, new: str) -> int:
    """Bump path (source-less) packages at an `old` version to `new`."""
    blocks = path.read_text(encoding="utf-8").split("\n[[package]]\n")
    changed = 0
    for i, block in enumerate(blocks):
        if i == 0 or "\nsource = " in block:
            continue
        m = re.search(r'\nversion = "([^"]+)"\n', block)
        if m and m.group(1) in old:
            blocks[i] = block[:m.start(1)] + new + block[m.end(1):]
            changed += 1
    path.write_text("\n[[package]]\n".join(blocks), encoding="utf-8")
    return changed


def main() -> int:
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    new = sys.argv[1].removeprefix("v")
    if not SEMVER.match(new):
        sys.exit(f"not a semantic version: {sys.argv[1]}")

    old = set_manifest_version(ROOT / "Cargo.toml", "workspace.package", new)
    n = set_lock_versions(ROOT / "Cargo.lock", {old}, new)

    viewer = ROOT / "crates" / "schematic-viewer"
    viewer_old = set_manifest_version(viewer / "Cargo.toml", "package", new)
    # its lock also holds the workspace crates it depends on by path
    m = set_lock_versions(viewer / "Cargo.lock", {old, viewer_old}, new)
    conf_path = viewer / "tauri.conf.json"
    conf = json.loads(conf_path.read_text(encoding="utf-8"))
    conf["version"] = new
    conf_path.write_text(json.dumps(conf, indent=2, ensure_ascii=False) + "\n",
                         encoding="utf-8")

    print(f"version {old} -> {new} (Cargo.lock: {n} crates, viewer: {m} crates)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
