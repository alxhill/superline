#!/usr/bin/env bash
# Screenshot or record superline for a PR, with widgets fed from fixtures so it
# looks the same on any machine. Runs the branch-local release build in fish
# inside an isolated home, exactly as the website screenshots do.
#
#   scripts/demo/record.sh --config config.json --fixtures fixtures.json \
#     --tape demo.tape [--setup 'source setup.sh'] [--dir code/demo] \
#     [--size 120x10] [-o target/demo/demo.png]
#
#   --config    superline config for the session
#   --fixtures  SUPERLINE_FIXTURES file (see docs/fixtures.md)
#   --tape      VHS commands to run on camera (Type, Enter, Sleep, ...)
#   --setup     bash run first in the working directory, with the helpers in
#               scripts/lib/vhs-scene.sh (git_repo, git_repo_with_upstream)
#   --dir       working directory, relative to the isolated home
#   --size      terminal columns x rows
#   -o          .png for a trimmed screenshot, .gif/.mp4/.webm for a
#               recording; defaults to target/demo/<tape or example name>.png
#
# Example: scripts/demo/record.sh --example pr-usage [-o target/demo/pr-usage.gif]
# runs scripts/demo/examples/pr-usage (config.json, fixtures.json, demo.tape
# and setup.sh).
set -euo pipefail

repo=$(cd "$(dirname "$0")/../.." && pwd)
name="" config="" fixtures="" tape="" setup="" dir=code/demo size="" output=""

absolute() { echo "$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"; }
usage() { sed -n '2,/^set -euo/p' "$0" | sed '$d; s/^# \{0,1\}//' >&2; exit "${1:-2}"; }

while (($#)); do
  case $1 in
    --config) config=$2; shift 2 ;;
    --fixtures) fixtures=$2; shift 2 ;;
    --tape) tape=$2; shift 2 ;;
    --setup) setup=$2; shift 2 ;;
    --dir) dir=$2; shift 2 ;;
    --size) size=$2; shift 2 ;;
    -o | --output) output=$2; shift 2 ;;
    --example)
      example="$repo/scripts/demo/examples/$2"
      [[ -d $example ]] || { echo "no example $2" >&2; exit 2; }
      config="$example/config.json" fixtures="$example/fixtures.json" tape="$example/demo.tape"
      [[ -f $example/setup.sh ]] && setup="source '$example/setup.sh'"
      name=$2
      shift 2
      ;;
    -h | --help) usage 0 ;;
    *) echo "unknown argument $1" >&2; usage ;;
  esac
done
[[ -n $config && -n $tape ]] || usage
config=$(absolute "$config")
tape=$(absolute "$tape")
[[ -n $fixtures ]] && fixtures=$(absolute "$fixtures")
name=${name:-$(basename "${tape%.*}")}
output=${output:-$repo/target/demo/$name.png}
mkdir -p "$(dirname "$output")"
output=$(absolute "$output")

source "$repo/scripts/lib/vhs-scene.sh"
scene_prepare

# Short and resolved, as in generate.sh, so the prompt's path is tidy.
scratch=$(cd "$(mktemp -d /tmp/superline-demo.XXXXXX)" && pwd -P)
trap 'chmod -R u+w "$scratch"; rm -rf "$scratch"' EXIT
mkdir -p "$scratch/home/$dir"
if [[ -n $setup ]]; then
  (cd "$scratch/home/$dir" && HOME="$scratch/home" eval "$setup")
fi

export FIXTURES=$fixtures
case $output in
  *.png)
    size=${size:-120x10}
    scene_screenshot "$output" "$scratch" "${size%x*}" "${size#*x}" "$config" "$dir" <"$tape"
    ;;
  *.gif | *.mp4 | *.webm)
    size=${size:-120x16}
    scene_recording "$output" "$scratch" "${size%x*}" "${size#*x}" "$config" "$dir" <"$tape"
    ;;
  *) echo "output must be .png, .gif, .mp4 or .webm" >&2; exit 2 ;;
esac
echo "wrote $output"
