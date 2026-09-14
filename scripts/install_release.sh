#!/usr/bin/env bash
# Install the current release binary into the immutable version store,
# update the stable + current channel symlinks, and point the launcher at current.
#
# Paths after install:
# - ~/.jcode/builds/versions/<hash>/jcode (immutable)
# - ~/.jcode/builds/stable/jcode -> .../versions/<hash>/jcode
# - ~/.jcode/builds/current/jcode -> .../versions/<hash>/jcode
# - ~/.local/bin/jcode -> ~/.jcode/builds/current/jcode (launcher)
set -euo pipefail

repo_root="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"

profile="${JCODE_RELEASE_PROFILE:-release-lto}"
if [[ "${1:-}" == "--fast" ]]; then
  profile="release"
  shift
fi

if [[ "$#" -gt 0 ]]; then
  echo "Usage: $0 [--fast]" >&2
  exit 1
fi

case "$profile" in
  release-lto)
    echo "Building with LTO (this takes a few minutes)..."
    ;;
  release)
    echo "Building fast release profile (no LTO)..."
    ;;
  *)
    echo "Unsupported profile: $profile (expected: release or release-lto)" >&2
    exit 1
    ;;
esac

cargo build --profile "$profile" --manifest-path "$repo_root/Cargo.toml"
bin="$repo_root/target/$profile/jcode"

if [[ ! -x "$bin" ]]; then
  echo "Release binary not found: $bin" >&2
  exit 1
fi

# Version key for the immutable store. This path keys by git short hash (with a
# -dirty suffix for uncommitted trees); scripts/install.ps1 (the upstream
# downloaded-installer path) keys by version number instead. The two schemes
# cannot collide, and only this script's keys feed prune_old_versions below, so
# the difference is documented rather than unified: rewriting install.ps1's keys
# would orphan existing installations.
hash=""
if command -v git >/dev/null 2>&1; then
  if git -C "$repo_root" rev-parse --git-dir >/dev/null 2>&1; then
    hash="$(git -C "$repo_root" rev-parse --short HEAD 2>/dev/null || true)"
    if [[ -n "${hash}" ]] && [[ -n "$(git -C "$repo_root" status --porcelain 2>/dev/null || true)" ]]; then
      hash="${hash}-dirty"
    fi
  fi
fi

if [[ -z "$hash" ]]; then
  hash="$(date +%Y%m%d%H%M%S)"
fi

# Install versioned binary into <JCODE_HOME or ~/.jcode>/builds/versions/<hash>/.
#
# JCODE_HOME is the layout AGENTS.md documents and the one the user-level
# environment actually uses on Windows (measured 2026-09-13:
# JCODE_HOME=C:\Users\<user>\.jcode with builds/ under it, and no
# %LOCALAPPDATA%\jcode\builds at all). This script used to hardcode
# $HOME/.jcode/builds and ignore JCODE_HOME, so a host with a custom JCODE_HOME
# ended up with two divergent install trees.
builds_dir="${JCODE_HOME:-$HOME/.jcode}/builds"
version_dir="$builds_dir/versions/$hash"
mkdir -p "$version_dir"
install -m 755 "$bin" "$version_dir/jcode"

# `current` is the single authoritative launcher target.
current_dir="$builds_dir/current"
mkdir -p "$current_dir"
ln -sfn "$version_dir/jcode" "$current_dir/jcode"
printf '%s\n' "$hash" > "$builds_dir/current-version"

# `stable` is a compatibility alias: it points at current rather than at the
# version dir, so there is exactly one authoritative target to keep in sync.
# Kept for one release cycle so launchers that still resolve builds/stable/jcode
# keep working (rollback path in
# .op/changes/packaging-simplification/plan.md).
stable_dir="$builds_dir/stable"
mkdir -p "$stable_dir"
ln -sfn "$current_dir/jcode" "$stable_dir/jcode"
printf '%s\n' "$hash" > "$builds_dir/stable-version"

# Update launcher path to current channel
install_dir="${JCODE_INSTALL_DIR:-$HOME/.local/bin}"
mkdir -p "$install_dir"
ln -sfn "$current_dir/jcode" "$install_dir/jcode"

echo "Installed: $version_dir/jcode"
echo "Updated current symlink: $current_dir/jcode -> $version_dir/jcode"
echo "Updated stable alias: $stable_dir/jcode -> $current_dir/jcode"
echo "Updated launcher symlink: $install_dir/jcode -> $current_dir/jcode"

# Retain policy for the immutable version store. Before this, every install simply
# appended to versions/<hash> and nothing ever pruned it (the dev machine carried 3
# versions / 0.34 GB with no policy). Keep the newest JCODE_VERSIONS_KEEP versions
# (default 3), always keep the in-use version, and delete nothing else.
#
# In-use detection deliberately prefers the current-version / stable-version marker
# files this script writes. On MSYS `ln -s` can fall back to copying the file, and
# `readlink -f` then returns the link path itself instead of the version dir, so a
# symlink-only check silently misses the live version (caught by
# .op/changes/packaging-simplification/work/t4-prune-versions-test.sh).
prune_old_versions() {
  local versions_dir="$builds_dir/versions"
  local keep="${JCODE_VERSIONS_KEEP:-3}"
  [[ -d "$versions_dir" ]] || return 0

  local active=() marker hash link target
  for marker in "$builds_dir/current-version" "$builds_dir/stable-version"; do
    if [[ -f "$marker" ]]; then
      hash="$(tr -d '[:space:]' < "$marker" 2>/dev/null || true)"
      if [[ -n "$hash" ]]; then
        active+=("$hash")
      fi
    fi
  done
  for link in "$current_dir/jcode" "$stable_dir/jcode"; do
    target="$(readlink -f "$link" 2>/dev/null || true)"
    case "$target" in
      */versions/*) active+=("$(basename "$(dirname "$target")")") ;;
    esac
  done
  active+=("$(basename "$version_dir")")

  local index=0 dir name candidate skip
  while IFS= read -r dir; do
    [[ -n "$dir" ]] || continue
    name="$(basename "$dir")"
    index=$((index + 1))
    if (( index <= keep )); then
      continue
    fi
    skip="false"
    for candidate in ${active[@]+"${active[@]}"}; do
      if [[ "$candidate" == "$name" ]]; then
        skip="true"
      fi
    done
    if [[ "$skip" == "true" ]]; then
      echo "Keeping in-use version: $dir"
      continue
    fi
    if rm -rf -- "$dir"; then
      echo "Pruned old version: $dir"
    else
      echo "Warning: could not prune $dir (permissions?)" >&2
    fi
  done < <(ls -1dt "$versions_dir"/*/ 2>/dev/null | sed 's:/$::')
}

prune_old_versions


# Gracefully reload any running background server onto the binary we just
# installed (issue #291). `server reload` only reloads when the running daemon
# is genuinely older, hands live headless/swarm sessions to the new process, and
# is a no-op when no server is running, so it is safe to call unconditionally.
if [ "${JCODE_SKIP_SERVER_RELOAD:-}" != "1" ]; then
  if "$install_dir/jcode" server reload </dev/null >/dev/null 2>&1; then
    echo "Reloaded the running jcode server onto $hash (if one was active)."
  fi
fi

if ! echo "$PATH" | tr ':' '\n' | grep -qx "$install_dir"; then
  echo ""
  echo "Tip: add $install_dir to PATH if needed."
fi

# Ensure the launcher dir is on PATH for bash, zsh and fish in future shells.
# shellcheck source=scripts/lib/configure_path.sh
. "$(dirname "$0")/lib/configure_path.sh"
jcode_configure_path "$install_dir"
