#!/bin/sh
# Publish doc/wiki/ to the GitHub wiki (the repository's .wiki.git).
# GitHub creates that repository only once the wiki's first page has been
# saved on the website; after that this script keeps it equal to doc/wiki/.
#   tools/wiki_sync.sh [REMOTE]
set -e
root=$(cd "$(dirname "$0")/.." && pwd)
remote=${1:-$(git -C "$root" remote get-url origin | sed 's/\.git$//').wiki.git}
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
git clone -q "$remote" "$tmp"
find "$tmp" -maxdepth 1 -name '*.md' -delete
cp "$root"/doc/wiki/*.md "$tmp"/
cd "$tmp"
git add -A
if git diff --cached --quiet; then
    echo "wiki already up to date"
    exit 0
fi
git commit -q -m "Sync with doc/wiki at $(git -C "$root" rev-parse --short HEAD)"
git push -q
echo "wiki updated"
