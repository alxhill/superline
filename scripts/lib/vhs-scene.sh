# Renders superline under VHS: the branch-local release build in fish, inside
# an isolated home, with backends answered from a fixtures file. Sourced by
# scripts/site-screenshots/generate.sh and scripts/demo/record.sh.
#
#   source scripts/lib/vhs-scene.sh
#   scene_prepare
#   FIXTURES=fixtures.json scene_screenshot out.png "$scratch" 90 8 config.json code <<'EOF'
#   Type "git status"
#   Enter
#   EOF
#
# Each scene runs in <scratch>/home, with the working directory given relative
# to it. Optional variables:
#   FIXTURES  a SUPERLINE_FIXTURES file for the session
#   PRE       a fish command run off camera in the working directory
#   HIDDEN    tape commands run off camera just before a recording starts
#
# Needs the patched VHS from tests/terminal/build-vhs.sh (pass it with
# SUPERLINE_E2E_VHS, or it is built into target/vhs-bin on first use), plus
# fish, git, uv, ttyd, ffmpeg, the MesloLGS Nerd Font and Symbols Nerd Font
# Mono 3.4 or newer.
# shellcheck shell=bash

repo=${repo:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}
vhs=${SUPERLINE_E2E_VHS:-$repo/target/vhs-bin/vhs}
scene_theme="$repo/scripts/site-screenshots/theme.json"

# Fixed git identity and dates so commit hashes are stable between runs.
export GIT_AUTHOR_NAME=superline GIT_AUTHOR_EMAIL=demo@example.com
export GIT_COMMITTER_NAME=superline GIT_COMMITTER_EMAIL=demo@example.com
export GIT_AUTHOR_DATE="2026-01-01T12:00:00Z" GIT_COMMITTER_DATE="2026-01-01T12:00:00Z"
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1
# Scenes read the kubeconfig from their fake home.
unset KUBECONFIG

# Builds VHS if needed and the release build of superline.
scene_prepare() {
  if [[ ! -x "$vhs" ]]; then
    "$repo/tests/terminal/build-vhs.sh" "$repo/target/vhs-bin"
  fi
  cargo build --quiet --release --bin superline --manifest-path "$repo/Cargo.toml"
}

git_repo() {
  local dir=$1
  mkdir -p "$dir"
  git -C "$dir" init --quiet --initial-branch=main
  git -C "$dir" remote add origin https://github.com/alxhill/superline.git
  echo "# demo" >"$dir/README.md"
  git -C "$dir" add README.md
  git -C "$dir" commit --quiet -m "initial commit"
}

# Commits on `main`, pushed to a local bare remote so the branch has an
# upstream, then diverged by `ahead` local and `behind` remote commits.
git_repo_with_upstream() {
  local dir=$1 ahead=$2 behind=$3 branch=${4:-main}
  local remote="$dir.remote.git"
  git_repo "$dir"
  git -C "$dir" switch --quiet -c "$branch" 2>/dev/null || true
  git init --quiet --bare "$remote"
  git -C "$dir" remote set-url origin "$remote"
  git -C "$dir" push --quiet -u origin "$branch"
  if ((behind > 0)); then
    local other="$dir.other"
    git clone --quiet --branch "$branch" "$remote" "$other"
    for i in $(seq "$behind"); do
      git -C "$other" commit --quiet --allow-empty -m "upstream $i"
    done
    git -C "$other" push --quiet
    git -C "$dir" fetch --quiet
    rm -rf "$other"
  fi
  for ((i = 1; i <= ahead; i++)); do
    git -C "$dir" commit --quiet --allow-empty -m "local $i"
  done
}

# Sets up <scratch>/home with the config and prints the hidden fish command
# that enters it. Arguments: scratch, config, workdir, then any extra fish
# commands to run before PRE.
scene_home() {
  local scratch=$1 config=$2 dir=$3 extra=${4:-true}
  local home="$scratch/home" fixtures=""
  mkdir -p "$home/.config/superline" "$home/$dir"
  cp "$config" "$home/.config/superline/config.json"
  "$repo/target/release/superline" init fish >"$home/superline-init.fish"
  if [[ -n ${FIXTURES:-} ]]; then
    cp "$FIXTURES" "$scratch/fixtures.json"
    # UTC, so a pinned clock reads the same on every machine.
    fixtures="set -gx SUPERLINE_FIXTURES '$scratch/fixtures.json'; set -gx TZ UTC; "
  fi
  printf '%s' "set -gx HOME '$home'; set -gx XDG_CONFIG_HOME '$home/.config'; set -gx XDG_CACHE_HOME '$home/.cache'; ${fixtures}${extra}; set -gx PATH '$repo/target/release' \$PATH; source '$home/superline-init.fish'; cd '$home/$dir'; ${PRE:-true}"
}

scene_run() {
  local scratch=$1 label=$2
  echo "rendering $label"
  if ! (cd "$scratch" && LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8 "$vhs" scene.tape >vhs.log 2>&1); then
    tail -30 "$scratch/vhs.log" >&2
    return 1
  fi
}

# Renders a screenshot to <out>.png, trimmed to its content. Arguments: out,
# scratch, columns, rows, config, workdir (relative to the home), then the
# tape body on stdin.
scene_screenshot() {
  local out=$1 scratch=$2 cols=$3 rows=$4 config=$5 dir=$6
  local body setup
  body=$(cat)
  mkdir -p "$scratch" "$(dirname "$out")"
  setup=$(scene_home "$scratch" "$config" "$dir")

  cat >"$scratch/scene.tape" <<EOF
Set Shell fish
Set Columns $cols
Set Rows $rows
Set FontSize 32
Set FontFamily "MesloLGS Nerd Font, Symbols Nerd Font Mono"
Set Padding 28
Set CursorBlink false
Set WaitTimeout 30s
Set TypingSpeed 40ms
Set Theme $(cat "$scene_theme")

Hide
Wait+Line />/
Type@2ms "$setup"
Enter
Sleep 1.5s
# A separate command so the setup's run time is not shown as a duration.
Type "clear"
Enter
Sleep 1s
Show
$body
Sleep 1s
Screenshot "$out"
Sleep 1s
EOF
  rm -f "$out"
  scene_run "$scratch" "$out" || return 1
  [[ -s "$out" ]] || { echo "no screenshot for $out" >&2; return 1; }
  uv run --quiet --with pillow python "$repo/scripts/site-screenshots/trim.py" "$out"
}

# Records the session to <out>, a .gif, .mp4 or .webm. Takes the same
# arguments as `scene_screenshot`.
scene_recording() {
  local out=$1 scratch=$2 cols=$3 rows=$4 config=$5 dir=$6
  local body setup ext=${1##*.}
  body=$(cat)
  mkdir -p "$scratch" "$(dirname "$out")"
  setup=$(scene_home "$scratch" "$config" "$dir" "set -gx SHELL (command -s fish)")

  cat >"$scratch/scene.tape" <<EOF
Output "$scratch/recording.$ext"
Set Shell fish
Set Columns $cols
Set Rows $rows
Set FontSize 14
Set FontFamily "MesloLGS Nerd Font, Symbols Nerd Font Mono"
Set Padding 16
Set CursorBlink false
Set WaitTimeout 30s
Set TypingSpeed 70ms
Set Theme $(cat "$scene_theme")

Hide
Wait+Line />/
Type@2ms "$setup"
Enter
Sleep 1.5s
Type "clear"
Enter
Sleep 1s
${HIDDEN:-}
Show
$body
Sleep 3s
EOF
  rm -f "$scratch/recording.$ext"
  scene_run "$scratch" "$out" || return 1
  [[ -s "$scratch/recording.$ext" ]] || { echo "no recording for $out" >&2; return 1; }
  cp "$scratch/recording.$ext" "$out"
}
