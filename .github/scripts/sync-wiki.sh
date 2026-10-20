#!/usr/bin/env bash
# Mirrors the hand-written pages in <src dir> (default: wiki/) into the GitHub wiki
# and regenerates the Releases page from the repository's published releases.
#
# Needs GITHUB_REPOSITORY, GITHUB_TOKEN (contents: write) and GH_TOKEN (for `gh`).
set -euo pipefail

src="${1:-wiki}"
repo="${GITHUB_REPOSITORY:?GITHUB_REPOSITORY is not set}"
token="${GITHUB_TOKEN:?GITHUB_TOKEN is not set}"
here="$(cd "$(dirname "$0")" && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

if ! git clone --quiet --depth 1 "https://x-access-token:${token}@github.com/${repo}.wiki.git" "$work" 2>/dev/null; then
  echo "::error::The wiki repository does not exist yet. Open https://github.com/${repo}/wiki, click 'Create the first page' and save it once, then re-run this job."
  exit 1
fi

# Replace everything except the git metadata, so pages removed from wiki/ disappear too.
find "$work" -mindepth 1 -maxdepth 1 ! -name .git -exec rm -rf {} +
cp -R "$src"/. "$work"/
bash "$here/wiki-releases.sh" > "$work/Releases.md"

cd "$work"
git add -A
if git diff --cached --quiet; then
  echo "Wiki is already up to date."
  exit 0
fi
git -c user.name="github-actions[bot]" \
    -c user.email="41898282+github-actions[bot]@users.noreply.github.com" \
    commit --quiet -m "Update wiki from ${repo}@${GITHUB_SHA:0:7}"
git push --quiet origin HEAD
echo "Wiki updated: https://github.com/${repo}/wiki"
