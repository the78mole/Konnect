#!/usr/bin/env bash
# Decide whether the current commit gets a release tag. Used by
# .github/workflows/version.yml; writes `tag=<vX.Y.Z or empty>` to
# $GITHUB_OUTPUT (stdout when unset) and explains the decision.
#
# Environment:
#   MANUAL     version given by hand (workflow_dispatch); released as is
#   COMPUTED   next version from paulhatch/semantic-version
#   IS_TAGGED  "true" when the commit already carries a version tag
set -euo pipefail

out=${GITHUB_OUTPUT:-/dev/stdout}
summary=${GITHUB_STEP_SUMMARY:-/dev/stderr}
semver='^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$'
stable='^v[0-9]+\.[0-9]+\.[0-9]+$'
# Conventional Commits that release: feat/fix, with optional scope and `!`,
# at the start of the subject or of a squash-merge body line ("* fix: ...").
releasing='^(\* )?(feat|fix)(\([^)]*\))?!?: |^BREAKING CHANGE: '

skip() { echo "No release: $1" | tee -a "$summary" >&2; echo "tag=" >> "$out"; exit 0; }
release() {
  if git rev-parse -q --verify "refs/tags/$1" >/dev/null; then
    echo "::error::tag $1 already exists"; exit 1
  fi
  echo "Release $1: $2" | tee -a "$summary" >&2
  echo "tag=$1" >> "$out"
}

if [ -n "${MANUAL:-}" ]; then
  version=${MANUAL#v}
  [[ $version =~ $semver ]] || { echo "::error::not a version: $MANUAL"; exit 1; }
  release "v$version" "requested by hand"
  exit 0
fi

[ "${IS_TAGGED:-false}" != "true" ] || skip "this commit is already tagged"

prev=$(git tag --list 'v*' --merged HEAD --sort=-v:refname | grep -E "$stable" | head -n 1 || true)
[ -n "$prev" ] || skip "no stable vX.Y.Z tag yet. Release the first version by hand (Actions > Version > Run workflow); later versions count from it."

if ! git log --format='%s%n%b' "$prev..HEAD" | grep -Eq "$releasing"; then
  skip "no feat or fix commit since $prev"
fi

[[ ${COMPUTED:-} =~ $semver ]] || { echo "::error::semantic-version gave no version (${COMPUTED:-})"; exit 1; }
# The computed version must move forward from the last stable tag.
if [ "$(printf '%s\n%s\n' "${prev#v}" "$COMPUTED" | sort -V | tail -n 1)" != "$COMPUTED" ] \
   || [ "${prev#v}" = "$COMPUTED" ]; then
  echo "::error::computed version $COMPUTED is not newer than $prev"; exit 1
fi
release "v$COMPUTED" "feat/fix commits since $prev"
