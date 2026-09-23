# Scene definitions for generate.sh. Each `scene_<name>` builds its fixture
# under "$work/<name>/home" and calls `capture` with the tape body on stdin.
# shellcheck shell=bash

ALL_SCENES=(hero git pr languages status usage themes separators widgets)

configs="$here/configs"

# Writes superline's own default config (as a fresh install sees it) with the
# update check turned off.
default_config() {
  local scratch="$work/default-config"
  mkdir -p "$scratch"
  HOME="$scratch" XDG_CONFIG_HOME="$scratch/.config" XDG_CACHE_HOME="$scratch/.cache" \
    "$repo/target/release/superline" show fish -s 0 -c 100 >/dev/null 2>&1 || true
  jq '.update = {"disable": true}' "$scratch/.config/superline/config.json" >"$work/default.json"
  echo "$work/default.json"
}

rust_project() {
  local dir=$1
  mkdir -p "$dir/src"
  printf '[package]\nname = "superline"\nversion = "0.19.1"\n' >"$dir/Cargo.toml"
  printf '[toolchain]\nchannel = "1.90.0"\n' >"$dir/rust-toolchain.toml"
  echo 'fn main() {}' >"$dir/src/main.rs"
}

scene_hero() {
  local home="$work/hero/home" dir="$work/hero/home/code/superline"
  git_repo_with_upstream "$dir" 0 0 main
  rust_project "$dir"
  git -C "$dir" add -A && git -C "$dir" commit --quiet -m "cargo project"
  git -C "$dir" push --quiet
  git -C "$dir" switch --quiet -c feat/usage-sparklines
  git -C "$dir" push --quiet -u origin feat/usage-sparklines
  git -C "$dir" commit --quiet --allow-empty -m "draw sparklines"
  echo "// sparkline" >>"$dir/src/main.rs"
  echo "## Usage" >>"$dir/README.md"
  git -C "$dir" add README.md
  touch "$dir/src/sparkline.rs"
  seed_usage "$home"
  PRE="superline show fish -s 0 -c 100 >/dev/null; sleep 1" \
    capture hero 132 8 "$(default_config)" code/superline <<'EOF'
Type "git status --short"
Enter
Sleep 1s
EOF
}

scene_git() {
  local dir="$work/git/home/code/superline"
  git_repo_with_upstream "$dir" 2 1 main
  printf 'a\n' >"$dir/staged.rs" && git -C "$dir" add staged.rs
  echo "changed" >>"$dir/README.md"
  touch "$dir/notes.md" "$dir/todo.md"
  capture git 90 9 "$configs/git.json" code/superline <<'EOF'
Type "git stash -q -u"
Enter
Sleep 1s
Type "git checkout -q --detach"
Enter
Sleep 1s
EOF
}

scene_pr() {
  local dir="$work/pr/home/code/superline" branch
  local branches=(feat/draft-themes feat/login fix/flaky-test feat/nushell)
  git_repo "$dir"
  for branch in "${branches[@]}"; do
    git -C "$dir" branch "$branch"
  done
  git -C "$dir" switch --quiet feat/draft-themes
  # PR lookups land in the cache in the background, so render each branch once
  # up front and let the refreshes finish before recording.
  local warm="for b in ${branches[*]}; git switch -q \$b; superline show fish -s 0 -c 90 >/dev/null; end; sleep 2; git switch -q feat/draft-themes"
  PRE="$warm" capture pr 90 12 "$configs/pr.json" code/superline <<'TAPE'
Type "git switch -q feat/login"
Enter
Sleep 1s
Type "git switch -q fix/flaky-test"
Enter
Sleep 1s
Type "git switch -q feat/nushell"
Enter
Sleep 1s
TAPE
}

scene_languages() {
  local code="$work/languages/home/code"
  mkdir -p "$code/api/.venv/bin" "$code/web" "$code/service"
  printf '[project]\nname = "api"\n' >"$code/api/pyproject.toml"
  printf 'home = /usr/bin\nversion_info = 3.13.7\n' >"$code/api/.venv/pyvenv.cfg"
  printf 'set -gx VIRTUAL_ENV (pwd)/.venv\n' >"$code/api/.venv/bin/activate.fish"
  echo "22.19.0" >"$code/web/.nvmrc"
  echo "java=21.0.8-tem" >"$code/service/.sdkmanrc"
  rust_project "$code/engine"
  printf '[tools]\nrust = "1.90.0"\n' >"$code/engine/mise.toml"
  capture languages 90 12 "$configs/languages.json" code/api <<'TAPE'
Type "source .venv/bin/activate.fish"
Enter
Sleep 1s
Type "set -e VIRTUAL_ENV; cd ../web"
Enter
Sleep 1s
Type "cd ../service"
Enter
Sleep 1s
Type "cd ../engine"
Enter
Sleep 1s
TAPE
}

scene_status() {
  mkdir -p "$work/status/home/code/superline"
  capture status 90 12 "$configs/status.json" code/superline <<'TAPE'
Type "sleep 2.4"
Enter
Sleep 3.5s
Type "sleep 30 &"
Enter
Sleep 1.5s
Type "cat missing.txt"
Enter
Sleep 1.5s
TAPE
}

scene_usage() {
  local home="$work/usage/home"
  mkdir -p "$home/code"
  seed_usage "$home"
  capture usage 90 12 "$configs/usage.json" code <<'TAPE'
TAPE
}

scene_themes() {
  local theme dir
  for theme in rainbow simple custom; do
    dir="$work/theme-$theme/home/code/superline"
    git_repo_with_upstream "$dir" 1 0 main
    rust_project "$dir"
    echo "changed" >>"$dir/README.md"
    local config="$work/theme-$theme.json"
    if [[ $theme == custom ]]; then
      jq '.theme = "theme.json"' "$configs/themes.json" >"$config"
      mkdir -p "$work/theme-$theme/home/.config/superline"
      cp "$configs/ocean-theme.json" "$work/theme-$theme/home/.config/superline/theme.json"
    else
      jq --arg theme "$theme" '.theme = $theme' "$configs/themes.json" >"$config"
    fi
    capture "theme-$theme" 90 6 "$config" code/superline <<'TAPE'
TAPE
  done
}

scene_separators() {
  mkdir -p "$work/separators/home/code/superline"
  capture separators 90 12 "$configs/separators.json" code/superline <<'TAPE'
TAPE
}

scene_widgets() {
  mkdir -p "$work/widgets/home/code/superline"
  capture widgets 90 8 "$configs/widgets.json" code/superline <<'TAPE'
Type "sleep 60 &; sleep 60 &"
Enter
Sleep 1.5s
TAPE
}
