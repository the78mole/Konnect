#!/usr/bin/env python3
"""Build KopperDigger's self-hosted KiCad PCM repository and kicad-addons metadata.

Input: packaging/metadata.json (package description) plus one
`*.pcm-version.json` per released version, as written by
build-pcm-universal.py and attached to each GitHub release.

Output directory layout (served as-is by GitHub Pages):

  repository.json        what users add in KiCad: PCM > Manage Repositories
  packages.json          package list with every released version
  resources.zip          <identifier>/icon.png for the PCM dialog
  index.html             short human-readable page with the repository URL
  kicad-addons/<identifier>/metadata.json
  kicad-addons/<identifier>/icon.png
                         ready to copy into a checkout of
                         gitlab.com/kicad/addons/metadata (packages/ dir)

Usage:
  python packaging/build-pcm-repo.py --versions DIR --base-url URL [--out site]
"""

import argparse
import hashlib
import html
import json
import re
import sys
import time
import zipfile
from pathlib import Path

HERE = Path(__file__).parent
ZIP_DATE = (2020, 1, 1, 0, 0, 0)


def version_key(entry: dict):
    """Sort key matching KiCad's PCM: epoch, then major.minor.patch."""
    nums = [int(n) for n in re.findall(r"\d+", entry["version"])[:3]]
    nums += [0] * (3 - len(nums))
    return (entry.get("version_epoch", 0), *nums)


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--metadata", type=Path, default=HERE / "metadata.json")
    ap.add_argument("--icon", type=Path, default=HERE / "resources" / "icon.png")
    ap.add_argument("--versions", type=Path, required=True,
                    help="directory holding *.pcm-version.json files")
    ap.add_argument("--base-url", required=True,
                    help="public URL the output directory is served from")
    ap.add_argument("--out", type=Path, default=Path("site"))
    args = ap.parse_args()

    meta = json.loads(args.metadata.read_text(encoding="utf-8"))
    ident = meta["identifier"]

    versions = {}
    for f in sorted(args.versions.glob("*.pcm-version.json")):
        entry = json.loads(f.read_text(encoding="utf-8"))
        for field in ("version", "download_url", "download_sha256",
                      "download_size", "install_size"):
            if field not in entry:
                sys.exit(f"{f}: missing {field}")
        # KiCad's PCM finds versions by their version string, so the list
        # must not hold one twice. A pre-release (v0.14.0-rc.1) is packaged
        # as 0.14.0 "testing" because the PCM accepts only digits; once the
        # stable 0.14.0 is out it replaces the test build.
        prev = versions.get(entry["version"])
        if prev is not None:
            rank = {"stable": 2, "testing": 1}
            if rank.get(prev.get("status"), 0) == rank.get(entry.get("status"), 0):
                sys.exit(f"{f}: version {entry['version']} is published twice "
                         f"with status {entry.get('status')}")
            if rank.get(prev.get("status"), 0) > rank.get(entry.get("status"), 0):
                continue
        versions[entry["version"]] = entry
    if not versions:
        sys.exit(f"no *.pcm-version.json in {args.versions}")

    meta["versions"] = sorted(versions.values(), key=version_key)
    base = args.base_url.rstrip("/")
    out = args.out
    out.mkdir(parents=True, exist_ok=True)

    # The repository entry carries no $schema; KiCad validates it on its own.
    pkg = {k: v for k, v in meta.items() if k != "$schema"}
    packages_path = out / "packages.json"
    packages_path.write_text(json.dumps({"packages": [pkg]}, indent=2) + "\n",
                             encoding="utf-8")

    resources_path = out / "resources.zip"
    with zipfile.ZipFile(resources_path, "w") as z:
        info = zipfile.ZipInfo(f"{ident}/icon.png", date_time=ZIP_DATE)
        info.compress_type = zipfile.ZIP_DEFLATED
        z.writestr(info, args.icon.read_bytes())

    now = int(time.time())
    utc = time.strftime("%Y-%m-%d %H:%M:%S", time.gmtime(now))

    def resource(path: Path) -> dict:
        return {
            "url": f"{base}/{path.name}",
            "sha256": sha256(path),
            "update_timestamp": now,
            "update_time_utc": utc,
        }

    repository = {
        "$schema": "https://go.kicad.org/pcm/schemas/v1#/definitions/Repository",
        "name": "KopperDigger",
        "packages": resource(packages_path),
        "resources": resource(resources_path),
        "maintainer": {
            "name": meta["author"]["name"],
            "contact": meta["author"].get("contact", {}),
        },
    }
    (out / "repository.json").write_text(json.dumps(repository, indent=2) + "\n",
                                         encoding="utf-8")

    addons = out / "kicad-addons" / ident
    addons.mkdir(parents=True, exist_ok=True)
    (addons / "metadata.json").write_text(json.dumps(meta, indent=2) + "\n",
                                          encoding="utf-8")
    (addons / "icon.png").write_bytes(args.icon.read_bytes())

    repo_url = html.escape(f"{base}/repository.json")
    latest = html.escape(meta["versions"][-1]["version"])
    (out / "index.html").write_text(f"""<!doctype html>
<html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>KopperDigger PCM repository</title>
<style>body{{font-family:system-ui,sans-serif;max-width:42rem;margin:2rem auto;padding:0 1rem;line-height:1.5}}
code{{background:#eee;padding:.1rem .3rem;border-radius:.2rem;word-break:break-all}}</style>
</head><body>
<h1>KopperDigger for KiCad</h1>
<p>Latest version: <strong>{latest}</strong></p>
<p>In KiCad, open <em>Plugin and Content Manager</em>, click <em>Manage...</em>
next to the repository list, add this URL, then pick <em>KopperDigger</em> from the list:</p>
<p><code>{repo_url}</code></p>
<p>Source, issues and documentation:
<a href="{html.escape(meta['resources']['homepage'])}">{html.escape(meta['resources']['homepage'])}</a></p>
</body></html>
""", encoding="utf-8")

    print(f"PCM repository: {base}/repository.json "
          f"({len(meta['versions'])} version(s), latest {latest})")
    print(f"kicad-addons submission files: {addons}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
