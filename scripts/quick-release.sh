#!/usr/bin/env bash
set -euo pipefail

# Release helper for this fork.
#
# Publishing target: Windows x86_64 only (see .github/workflows/release.yml),
# built by CI on tag push. This script therefore has exactly one path: verify the
# version, tag HEAD, push the tag, and make sure the draft release exists with
# generated notes. CI does the build, signing, checksums and publication.
#
# History: this script used to carry four mutually exclusive modes -- a default
# "standard" local Linux (Docker manylinux) + macOS (osxcross) build,
# --fast-local (publish a local Linux build immediately), --prepare-fast (warm a
# selfdev binary), and --remote. Measured 2026-09-13 on the dev host: docker was
# not installed and ~/.osxcross and ~/.cache/jcode-linux-compat did not exist, so
# every local build path was unrunnable; CI does not publish those platforms
# either. They were removed rather than maintained.
#
# Usage:
#   scripts/quick-release.sh <version> [title]     # tag, push, ensure draft
#   scripts/quick-release.sh --dry-run <version>   # show the plan, change nothing

DRY_RUN=false
while [[ "${1:-}" == --* ]]; do
    case "$1" in
        --dry-run)
            DRY_RUN=true
            ;;
        -h|--help)
            sed -n '4,22p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *)
            echo "Error: Unknown option: $1" >&2
            echo "Usage: scripts/quick-release.sh [--dry-run] <version> [title]" >&2
            exit 1
            ;;
    esac
    shift
done

VERSION="${1:?Usage: scripts/quick-release.sh [--dry-run] <version> [title]}"
TITLE="${2:-$VERSION}"

if [[ ! "$VERSION" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "Error: Version must be in format v0.5.4"
    exit 1
fi

cd "$(git rev-parse --show-toplevel)"

required_commands=(git)
$DRY_RUN || required_commands+=(gh)
for cmd in "${required_commands[@]}"; do
    command -v "$cmd" &>/dev/null || { echo "Error: $cmd not found."; exit 1; }
done

# CI builds the tagged commit, so uncommitted work is simply not part of the
# release. Warn instead of prompting: there is no local build to invalidate.
if [[ -n "$(git status --porcelain)" ]]; then
    echo "Warning: only committed HEAD is tagged; working-tree changes are ignored." >&2
fi

NOTES_DIR="$(mktemp -d)"
trap 'rm -rf "$NOTES_DIR"' EXIT
NOTES_FILE="$NOTES_DIR/release_notes.md"
OVERALL_START=$(date +%s)
TAG_PUSHED=false

elapsed() {
    echo $(( $(date +%s) - OVERALL_START ))
}

tag_and_push() {
    echo "▸ Tagging $VERSION..."
    local head_commit remote_tags remote_commit local_commit
    head_commit="$(git rev-parse HEAD)"

    remote_tags="$(git ls-remote --tags origin "refs/tags/$VERSION" "refs/tags/$VERSION^{}" 2>/dev/null || true)"
    if [[ -n "$remote_tags" ]]; then
        remote_commit="$(printf '%s\n' "$remote_tags" | awk '$2 ~ /\^\{\}$/ { print $1; found=1 } END { if (!found) print first } NR == 1 { first=$1 }')"
        if [[ "$remote_commit" != "$head_commit" ]]; then
            echo "Error: Remote tag $VERSION already points to a different commit." >&2
            exit 1
        fi
        echo "  Remote tag already exists at HEAD"
        return
    fi

    if git tag -l "$VERSION" | grep -qx "$VERSION"; then
        local_commit="$(git rev-list -n 1 "$VERSION")"
        if [[ "$local_commit" != "$head_commit" ]]; then
            echo "Error: Local tag $VERSION already points to a different commit." >&2
            exit 1
        fi
        echo "  Local tag already exists at HEAD"
    else
        git tag "$VERSION" -m "$TITLE"
    fi
    git push origin "$VERSION"
    TAG_PUSHED=true
    echo "  Tag pushed"
}

generate_notes() {
    if ! scripts/generate_release_notes.sh "$VERSION" > "$NOTES_FILE" || [[ ! -s "$NOTES_FILE" ]]; then
        echo "  Warning: release notes generation failed, using the release title"
        printf '%s\n' "$TITLE" > "$NOTES_FILE"
    fi
}

ensure_release_draft() {
    generate_notes
    if ! gh release view "$VERSION" >/dev/null 2>&1; then
        if ! gh release create "$VERSION" \
            --draft \
            --title "$TITLE" \
            --notes-file "$NOTES_FILE"; then
            sleep 2
            gh release view "$VERSION" >/dev/null
        fi
    fi
    gh release edit "$VERSION" --title "$TITLE" --notes-file "$NOTES_FILE"
}

echo "=== Quick Release: $VERSION ($TITLE) ==="
echo ""

if $DRY_RUN; then
    echo "▸ Dry run plan (nothing is changed):"
    echo "  1. tag $VERSION at $(git rev-parse --short HEAD)"
    echo "  2. push the tag to origin"
    echo "  3. create/update the draft release with generated notes"
    echo "  4. CI builds, signs, checksums and publishes jcode-windows-x86_64"
    echo ""
    echo "No local build is performed: this fork publishes Windows x86_64 from CI only."
    exit 0
fi

tag_and_push
echo ""
echo "=== Remote release triggered in $(elapsed)s ==="
if $TAG_PUSHED; then
    echo "  ✅ Tag pushed; release CI started"
else
    echo "  ✅ Tag already existed at HEAD; release CI was already triggered"
fi

echo ""
echo "▸ Ensuring the draft release and its notes..."
ensure_release_draft
echo "  ✅ Draft release $VERSION is ready"
echo "  ⏳ CI will build, sign, checksum and publish the Windows x86_64 asset"
echo ""
echo "No local build was run. Publication remains gated on the release workflow."
