#!/usr/bin/env bash
# Build the pinned VHS release with vhs-render-context.patch applied.
#
# VHS v0.12.0 renders its screenshots with an already-cancelled context, so
# ffmpeg is killed before it writes anything (charmbracelet/vhs#787). Until a
# release carries the fix, build the tagged commit plus that one patch.
set -euo pipefail

VHS_REPOSITORY=https://github.com/charmbracelet/vhs
VHS_VERSION=v0.12.0
VHS_COMMIT=db96d7374f7d7a3774f69a43f4fcc5c5a1fd74e3

destination=${1:?usage: build-vhs.sh <output-dir>}
here=$(cd "$(dirname "$0")" && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

git -C "$work" init --quiet
git -C "$work" fetch --quiet --depth 1 "$VHS_REPOSITORY" "$VHS_COMMIT"
git -C "$work" checkout --quiet "$VHS_COMMIT"
git -C "$work" apply "$here/vhs-render-context.patch"

executable=vhs
if [[ "${OS:-}" == Windows_NT ]]; then
  executable=vhs.exe
fi
mkdir -p "$destination"
(cd "$work" && go build -trimpath -ldflags "-s -w -X main.Version=$VHS_VERSION+superline" -o "$destination/$executable" .)
"$destination/$executable" --version
