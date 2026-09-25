#!/usr/bin/env bash
# Regenerate the website screenshots in site/img with VHS.
#
# Every scene runs the branch-local release build of superline in fish, inside
# an isolated home with a scene-specific config and fixture directory. `gh` is
# replaced by a stub so PR segments are deterministic, and AI usage is served
# from a pre-seeded cache so no provider CLI is contacted.
#
#   scripts/site-screenshots/generate.sh [scene...]
#   COMPONENTS="git pr" scripts/site-screenshots/generate.sh components
#
# Needs the patched VHS from tests/terminal/build-vhs.sh (pass it
# with SUPERLINE_E2E_VHS, or it is built into target/vhs-bin on first run),
# plus fish, git, jq, uv, ttyd, ffmpeg, the MesloLGS Nerd Font and Symbols
# Nerd Font Mono 3.4 or newer (for the Claude and Codex glyphs).
set -euo pipefail

repo=$(cd "$(dirname "$0")/../.." && pwd)
here="$repo/scripts/site-screenshots"
out="$repo/site/img"
vhs=${SUPERLINE_E2E_VHS:-$repo/target/vhs-bin/vhs}

if [[ ! -x "$vhs" ]]; then
  "$repo/tests/terminal/build-vhs.sh" "$repo/target/vhs-bin"
fi
cargo build --quiet --release --bin superline --manifest-path "$repo/Cargo.toml"
mkdir -p "$out"

# Shorter than the macOS $TMPDIR so tape lines stay readable in logs. Resolved
# (/tmp is a symlink on macOS) so resolve_symlinks still sees the fake home.
work=$(cd "$(mktemp -d /tmp/superline-site.XXXXXX)" && pwd -P)
# The read_only fixture leaves a directory without write permission.
trap 'chmod -R u+w "$work"; rm -rf "$work"' EXIT

# Fixed git identity and dates so commit hashes are stable between runs.
export GIT_AUTHOR_NAME=superline GIT_AUTHOR_EMAIL=demo@example.com
export GIT_COMMITTER_NAME=superline GIT_COMMITTER_EMAIL=demo@example.com
export GIT_AUTHOR_DATE="2026-01-01T12:00:00Z" GIT_COMMITTER_DATE="2026-01-01T12:00:00Z"
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1
# Scenes read the kubeconfig from their fake home.
unset KUBECONFIG

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
  for i in $(seq "$ahead"); do
    git -C "$dir" commit --quiet --allow-empty -m "local $i"
  done
}

seed_usage() {
  local home=$1 now_s now_ms
  now_s=$(date +%s)
  now_ms=$((now_s * 1000))
  mkdir -p "$home/.cache/superline"
  cat >"$home/.cache/superline/usage-claude.json" <<EOF
{"fetched_at":$now_s,"value":{"session":38.0,"weekly":61.0,"fable":22.0,"credits":{"used":12.5,"limit":50.0,"unit":"dollars"},"session_resets_at":$((now_s + 8000))}}
EOF
  cat >"$home/.cache/superline/usage-codex.json" <<EOF
{"fetched_at":$now_s,"value":{"session":84.0,"weekly":47.0}}
EOF
  echo "$now_ms" >"$home/.cache/superline/usage-claude.refresh"
  echo "$now_ms" >"$home/.cache/superline/usage-codex.refresh"
}

# Renders one scene to site/img/<name>.png. Arguments: name, columns, rows,
# config path, workdir (relative to the fake home), then the tape body on
# stdin. PRE is an optional hidden fish command run in the workdir first.
capture() {
  local name=$1 cols=$2 rows=$3 config=$4 dir=$5
  local home="$work/$name/home"
  local tape="$work/$name/scene.tape"
  local body
  body=$(cat)

  mkdir -p "$home/.config/superline" "$home/$dir" "$(dirname "$out/$name.png")"
  cp "$config" "$home/.config/superline/config.json"
  "$repo/target/release/superline" init fish >"$home/superline-init.fish"

  cat >"$tape" <<EOF
Set Shell fish
Set Columns $cols
Set Rows $rows
Set FontSize 32
Set FontFamily "MesloLGS Nerd Font, Symbols Nerd Font Mono"
Set Padding 28
Set CursorBlink false
Set WaitTimeout 30s
Set TypingSpeed 40ms
Set Theme $(cat "$here/theme.json")

Hide
Wait+Line />/
Type@2ms "set -gx HOME '$home'; set -gx XDG_CONFIG_HOME '$home/.config'; set -gx XDG_CACHE_HOME '$home/.cache'; set -gx PATH '$here/bin' '$repo/target/release' \$PATH; source '$home/superline-init.fish'; cd '$home/$dir'; ${PRE:-true}"
Enter
Sleep 1.5s
# A separate command so the setup's run time is not shown as a duration.
Type "clear"
Enter
Sleep 1s
Show
$body
Sleep 1s
Screenshot "$out/$name.png"
Sleep 1s
EOF
  echo "capturing $name"
  if ! (cd "$work/$name" && LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8 "$vhs" "$tape" >"$work/$name/vhs.log" 2>&1); then
    tail -30 "$work/$name/vhs.log" >&2
    return 1
  fi
  [[ -s "$out/$name.png" ]] || { echo "no screenshot for $name" >&2; return 1; }
  uv run --quiet --with pillow python "$here/trim.py" "$out/$name.png"
}

source "$here/scenes.sh"

scenes=("$@")
if ((${#scenes[@]} == 0)); then
  scenes=("${ALL_SCENES[@]}")
fi
for scene in "${scenes[@]}"; do
  "scene_$scene"
done


uv run --quiet --with pillow python "$here/render_examples.py"
