#!/usr/bin/env bash
# Download the .pcm-version.json of every published release into
# <dir>/<tag>/ (one directory per tag: a pre-release and the stable release
# that follows it share a file name, because the PCM version is digits only).
#
#   packaging/collect-pcm-versions.sh DIR [--stable-only]
#
# Needs GH_TOKEN and GITHUB_REPOSITORY. Lists all releases (paginated, no
# count limit). Releases without the asset (older than the PCM packaging)
# are skipped; a release that has the asset but cannot be downloaded fails
# the run, so a catalog is never published with a version silently missing.
set -euo pipefail

dir="${1:?usage: collect-pcm-versions.sh DIR [--stable-only]}"
stable_only="${2:-}"
mkdir -p "$dir"

filter='.[] | select(.draft | not)'
[ "$stable_only" = "--stable-only" ] && filter="$filter | select(.prerelease | not)"

gh api --paginate "repos/$GITHUB_REPOSITORY/releases?per_page=100" \
    --jq "$filter | . as \$r | .assets[] | select(.name | endswith(\".pcm-version.json\")) | [\$r.tag_name, .name] | @tsv" |
while IFS=$'\t' read -r tag asset; do
    gh release download "$tag" --repo "$GITHUB_REPOSITORY" \
        --pattern "$asset" --dir "$dir/$tag" --clobber
done

count=$(find "$dir" -name '*.pcm-version.json' | wc -l)
echo "Collected $count version entr$( [ "$count" = 1 ] && echo y || echo ies ) into $dir"
[ "$count" -gt 0 ] || { echo "::error::no release carries a .pcm-version.json"; exit 1; }
