#!/usr/bin/env python3
"""Merge KopperDigger's generated metadata into the one KiCad already has.

KiCad's addon CI rejects a merge request that changes the download fields
of a version it already lists, and the KiCad team may edit entries (for
example `kicad_version_max` on an old version). So the official file is the
base: versions it lists stay exactly as they are, versions it lacks are
appended from ours, and the package description fields are taken from ours.

Usage:
  python packaging/merge-kicad-addons-metadata.py \\
      --ours site/kicad-addons/<id>/metadata.json \\
      [--theirs checkout/packages/<id>/metadata.json] --out FILE

Prints the versions that are new compared to --theirs, one per line, and
exits 3 when there is nothing new (so a workflow can stop early).
"""

import argparse
import json
import sys
from pathlib import Path

DESCRIPTIVE = ("$schema", "name", "description", "description_full",
               "identifier", "type", "author", "maintainer", "license",
               "resources", "tags")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--ours", type=Path, required=True)
    ap.add_argument("--theirs", type=Path)
    ap.add_argument("--out", type=Path, required=True)
    args = ap.parse_args()

    ours = json.loads(args.ours.read_text(encoding="utf-8"))
    if args.theirs and args.theirs.exists():
        theirs = json.loads(args.theirs.read_text(encoding="utf-8"))
    else:
        theirs = {"versions": []}

    if theirs.get("identifier", ours["identifier"]) != ours["identifier"]:
        sys.exit("identifier mismatch between our and KiCad's metadata")

    merged = {k: ours[k] for k in DESCRIPTIVE if k in ours}
    known = {v["version"] for v in theirs["versions"]}
    new = [v for v in ours["versions"] if v["version"] not in known]
    merged["versions"] = list(theirs["versions"]) + new
    for k, v in theirs.items():  # keep fields KiCad added that we don't know
        if k not in merged and k != "versions":
            merged[k] = v

    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(merged, indent=4, ensure_ascii=False) + "\n",
                        encoding="utf-8")
    for v in new:
        print(v["version"])
    return 0 if new else 3


if __name__ == "__main__":
    sys.exit(main())
