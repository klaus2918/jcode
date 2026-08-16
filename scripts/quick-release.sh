#!/usr/bin/env bash
set -euo pipefail

# Release helper with three modes:
# - --prepare-fast: refresh target/selfdev before the release metadata commit.
# - --fast-local: package a local release-profile build, publish Linux
#   immediately, then let CI replace it with portable/signoff assets and add
#   every other platform.
# - --remote: push the tag immediately and let CI gate publication.
# - default: build Linux + macOS locally and stage them on the CI-owned draft.
#
# Usage:
#   scripts/quick-release.sh --prepare-fast v0.5.5 # warm selfdev before bump
#   scripts/quick-release.sh --fast-local v0.5.5  # package a release build, public now
#   scripts/quick-release.sh --remote v0.5.5      # tag now, CI-gated publication
#   scripts/quick-release.sh v0.5.5               # local Linux + macOS draft
#   scripts/quick-release.sh --dry-run v0.5.5     # standard local build only

MODE="standard"
DRY_RUN=false
while [[ "${1:-}" == --* ]]; do
    case "$1" in
        --dry-run) DRY_RUN=true ;;
        --fast|--fast-local)
            [[ "$MODE" == "standard" ]] || { echo "Error: release modes cannot be combined." >&2; exit 1; }
            MODE="fast-local"
            ;;
        --prepare-fast)
            [[ "$MODE" == "standard" ]] || { echo "Error: release modes cannot be combined." >&2; exit 1; }
            MODE="prepare-fast"
            ;;
        --remote|--ci-only)
            [[ "$MODE" == "standard" ]] || { echo "Error: release modes cannot be combined." >&2; exit 1; }
            MODE="remote"
            ;;
        --)
            shift
            break
            ;;
        *)
            echo "Error: Unknown option: $1" >&2
            echo "Usage: scripts/quick-release.sh [--prepare-fast | --fast-local | --remote | --dry-run] <version> [title]" >&2
            exit 1
            ;;
    esac
    shift
done

if $DRY_RUN && [[ "$MODE" == "remote" ]]; then
    echo "Error: --dry-run cannot be combined with --remote." >&2
    exit 1
fi

VERSION="${1:?Usage: scripts/quick-release.sh [--prepare-fast | --fast-local | --remote | --dry-run] <version> [title]}"
TITLE="${2:-$VERSION}"
VERSION_NUM="${VERSION#v}"

if [[ ! "$VERSION" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "Error: Version must be in format v0.5.4"
    exit 1
fi

cd "$(git rev-parse --show-toplevel)"

required_commands=(git)
case "$MODE" in
    remote) ;;
    prepare-fast)
        required_commands+=(cargo sha256sum)
        ;;
    fast-local)
        required_commands+=(file strip tar gzip sha256sum cargo)
        $DRY_RUN || required_commands+=(gh)
        ;;
    standard)
        required_commands+=(cargo docker file)
        $DRY_RUN || required_commands+=(gh)
        ;;
esac
for cmd in "${required_commands[@]}"; do
    command -v "$cmd" &>/dev/null || { echo "Error: $cmd not found."; exit 1; }
done

if [[ "$MODE" == "standard" ]]; then
    [[ -f "$HOME/.cargo/env" ]] && source "$HOME/.cargo/env"
    export PATH="$HOME/.osxcross/bin:$PATH"
    if ! command -v aarch64-apple-darwin23.5-clang &>/dev/null; then
        echo "Error: osxcross not found. Install at ~/.osxcross"
        exit 1
    fi
fi

working_tree_changes="$(git status --porcelain)"
if [[ -n "$working_tree_changes" ]]; then
    case "$MODE" in
        fast-local|prepare-fast)
            echo "Error: --$MODE requires a clean working tree so the binary matches committed source." >&2
            printf '%s\n' "$working_tree_changes" >&2
            exit 1
            ;;
        remote)
            echo "Warning: working-tree changes are not included; only committed HEAD will be tagged."
            ;;
        standard)
            echo "Warning: uncommitted changes are present."
            read -rp "Continue anyway? [y/N] " confirm
            [[ "$confirm" =~ ^[Yy]$ ]] || exit 1
            ;;
    esac
fi

if [[ "$MODE" == "fast-local" || "$MODE" == "prepare-fast" ]]; then
    if [[ "$(uname -s)" != "Linux" || "$(uname -m)" != "x86_64" ]]; then
        echo "Error: --fast-local currently publishes the Linux x86_64 asset and must run on Linux x86_64." >&2
        exit 1
    fi
fi

echo "=== Quick Release: $VERSION ($MODE) ==="
echo ""

DIST="$(mktemp -d)"
trap 'rm -rf "$DIST"' EXIT
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
    NOTES_FILE="$DIST/release_notes.md"
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

if [[ "$MODE" == "prepare-fast" ]]; then
    echo "▸ Refreshing the warm selfdev Linux build before the version bump..."
    JCODE_REMOTE_CARGO=0 scripts/dev_cargo.sh build --profile selfdev -p jcode --bin jcode
    source_bin="target/selfdev/jcode"
    [[ -x "$source_bin" ]] || { echo "Error: selfdev binary not found: $source_bin" >&2; exit 1; }
    prepared_marker="target/selfdev/fast-release-prepared"
    {
        printf 'version=%s\n' "$VERSION_NUM"
        printf 'commit=%s\n' "$(git rev-parse HEAD)"
        printf 'binary_sha256=%s\n' "$(sha256sum "$source_bin" | cut -d' ' -f1)"
    } > "$prepared_marker"
    echo ""
    echo "=== Fast release prepared in $(elapsed)s ==="
    echo "  ✅ Warm selfdev binary recorded for $VERSION at $(git rev-parse --short HEAD)"
    echo "  Next: commit only Cargo.toml, Cargo.lock, and changelog release metadata, then run --fast-local."
    exit 0
fi

if [[ "$MODE" == "remote" ]]; then
    tag_and_push
    echo ""
    echo "=== Remote release triggered in $(elapsed)s ==="
    if $TAG_PUSHED; then
        echo "  ✅ Tag pushed; release CI started"
    else
        echo "  ✅ Tag already existed at HEAD; release CI was already triggered"
    fi
    echo "  ⏳ CI will build, sign, checksum, and publish every platform"
    echo ""
    echo "No local build was run. Publication remains gated on the release workflow."
    exit 0
fi

if [[ "$MODE" == "fast-local" ]]; then
    echo "▸ Validating the release Linux build..."
    build_start=$(date +%s)

    unexpected_release_files="$(git diff-tree --no-commit-id --name-only -r HEAD | grep -Ev '^(Cargo\.toml|Cargo\.lock|changelog/)' || true)"
    [[ -z "$unexpected_release_files" ]] || {
        echo "Error: the release metadata commit contains code or unsupported files:" >&2
        printf '%s\n' "$unexpected_release_files" >&2
        exit 1
    }

    # A genuine release build embeds the JCODE_RELEASE_BUILD marker: the
    # version string is "vX.Y.Z (<hash>)" without the "-dev" suffix
    # (jcode-build-meta/build.rs only drops the suffix when JCODE_RELEASE_BUILD
    # was set at compile time), and the hash matches the release commit being
    # published. Anything else (missing binary, dev/selfdev build, stale hash)
    # is rejected and rebuilt.
    fast_release_binary_ok() {
        local bin="$1" version_output head_hash
        version_output="$("$bin" --version 2>/dev/null || true)"
        [[ -n "$version_output" && "$version_output" != *"-dev"* ]] || return 1
        head_hash="$(git rev-parse --short HEAD)"
        [[ "$version_output" == *"($head_hash)"* ]] || return 1
        return 0
    }

    # Prefer the release-lto artifact (the stable distribution profile:
    # opt-level=1 + thin LTO), then the plain release profile (opt-level=1).
    # Rebuild with the release marker when neither is a valid release build.
    source_bin=""
    for candidate in target/release-lto/jcode target/release/jcode; do
        if [[ -x "$candidate" ]] && fast_release_binary_ok "$candidate"; then
            source_bin="$candidate"
            break
        fi
    done
    if [[ -z "$source_bin" ]]; then
        echo "▸ Building the release binary (JCODE_RELEASE_BUILD=1, --profile release)..."
        JCODE_RELEASE_BUILD=1 JCODE_BUILD_SEMVER="$VERSION_NUM" JCODE_REMOTE_CARGO=0 \
            scripts/dev_cargo.sh build --profile release -p jcode --bin jcode
        source_bin="target/release/jcode"
        fast_release_binary_ok "$source_bin" || {
            echo "Error: rebuilt binary failed release validation." >&2
            "$source_bin" --version >&2 || true
            exit 1
        }
    fi
    echo "  ✅ Release binary: $source_bin"

    cp "$source_bin" "$DIST/jcode-linux-x86_64.bin"
    strip --strip-unneeded "$DIST/jcode-linux-x86_64.bin"
    chmod +x "$DIST/jcode-linux-x86_64.bin"
    cat > "$DIST/jcode-linux-x86_64" <<WRAPPER
#!/usr/bin/env sh
set -eu
export JCODE_RUNTIME_RELEASE_SEMVER="$VERSION_NUM"
export JCODE_RUNTIME_RELEASE_GIT_HASH="$(git rev-parse --short HEAD)"
export JCODE_RUNTIME_RELEASE_GIT_DATE="$(git log -1 --format=%ci)"
export JCODE_RUNTIME_RELEASE_GIT_TAG="$VERSION"
self_dir=\$(CDPATH= cd -- "\$(dirname -- "\$0")" && pwd)
exec "\$self_dir/jcode-linux-x86_64.bin" "\$@"
WRAPPER
    chmod +x "$DIST/jcode-linux-x86_64"
    file "$DIST/jcode-linux-x86_64.bin" | grep -q 'ELF 64-bit' || { echo "Error: bad Linux binary" >&2; exit 1; }
    version_output="$("$DIST/jcode-linux-x86_64" --version)"
    printf '%s\n' "$version_output" | grep -Fq "v$VERSION_NUM" || {
        echo "Error: fast binary reports the wrong version: $version_output" >&2
        exit 1
    }
    (cd "$DIST" && tar -cf - jcode-linux-x86_64 jcode-linux-x86_64.bin | gzip -1 > jcode-linux-x86_64.tar.gz)
    echo "  ✅ Linux artifact ready ($(( $(date +%s) - build_start ))s validation/package, $(du -h "$DIST/jcode-linux-x86_64.tar.gz" | cut -f1))"

    if $DRY_RUN; then
        scripts/generate_checksums.sh "$DIST"
        echo ""
        echo "Fast dry run complete in $(elapsed)s. Artifacts in: $DIST"
        trap - EXIT
        exit 0
    fi

    tag_and_push
    echo "▸ Publishing immediate Linux release..."
    ensure_release_draft
    gh release upload "$VERSION" \
        "$DIST/jcode-linux-x86_64.tar.gz" \
        --clobber

    # SHA256SUMS must describe exactly the assets published in that release
    # (RELEASING.md), so pull every asset currently on the release back down --
    # including the archive just uploaded and any asset CI already attached to
    # the draft -- and checksum that full set. gh CLI behavior is not
    # verifiable on this (Windows) host; if the download fails, fall back to
    # checksumming the local packaging directory.
    echo "▸ Generating SHA256SUMS covering all release assets..."
    assets_dir="$DIST/release-assets"
    mkdir -p "$assets_dir"
    if (cd "$assets_dir" && gh release download "$VERSION") 2>/dev/null; then
        scripts/generate_checksums.sh "$assets_dir" "$DIST/SHA256SUMS"
    else
        echo "  Warning: could not download release assets; SHA256SUMS covers local artifacts only" >&2
        scripts/generate_checksums.sh "$DIST"
    fi
    gh release upload "$VERSION" \
        "$DIST/SHA256SUMS" \
        --clobber
    gh release edit "$VERSION" --draft=false --latest

    echo ""
    echo "=== Fast release published in $(elapsed)s ==="
    echo "  ✅ Linux x86_64: public now from the local release build"
    echo "  ⏳ CI: replacing Linux with the portable build and adding macOS, Windows, FreeBSD, signatures, and final checksums"
    echo ""
    echo "The immediate Linux asset targets this build host's runtime baseline until CI replaces it."
    exit 0
fi

# Standard local distribution build: Linux + macOS in parallel.
echo "▸ Building Linux x86_64 + macOS aarch64 in parallel..."
(
    JCODE_RELEASE_BUILD=1 JCODE_BUILD_SEMVER="$VERSION_NUM" scripts/build_linux_compat.sh "$DIST" >/dev/null
    echo "  ✅ Linux done ($(elapsed)s)"
) &
LINUX_PID=$!
(
    JCODE_RELEASE_BUILD=1 JCODE_BUILD_SEMVER="$VERSION_NUM" \
        CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-1}" \
        cargo build --release --target aarch64-apple-darwin --bin jcode 2>/dev/null
    cp target/aarch64-apple-darwin/release/jcode "$DIST/jcode-macos-aarch64"
    chmod +x "$DIST/jcode-macos-aarch64"
    (cd "$DIST" && tar czf jcode-macos-aarch64.tar.gz jcode-macos-aarch64)
    echo "  ✅ macOS done ($(elapsed)s)"
) &
MACOS_PID=$!
wait $LINUX_PID || { echo "Error: Linux build failed"; exit 1; }
wait $MACOS_PID || { echo "Error: macOS build failed"; exit 1; }

echo ""
echo "Build time: $(elapsed)s"
ls -lh "$DIST"/*.tar.gz
file "$DIST/jcode-linux-x86_64.bin" | grep -q 'ELF 64-bit' || { echo "Error: bad Linux binary"; exit 1; }
head -1 "$DIST/jcode-linux-x86_64" | grep -q '^#!/' || { echo "Error: bad Linux wrapper"; exit 1; }
file "$DIST/jcode-macos-aarch64" | grep -q 'Mach-O 64-bit' || { echo "Error: bad macOS binary"; exit 1; }

if $DRY_RUN; then
    echo ""
    echo "Dry run complete. Binaries in: $DIST"
    trap - EXIT
    exit 0
fi

echo ""
tag_and_push
echo "▸ Staging GitHub draft release..."
ensure_release_draft
gh release upload "$VERSION" \
    "$DIST/jcode-linux-x86_64.tar.gz" \
    "$DIST/jcode-macos-aarch64.tar.gz" \
    --clobber

echo ""
echo "=== Staged $VERSION in $(elapsed)s ==="
echo "  ✅ Linux + macOS: attached to draft"
echo "  ⏳ CI: building optional platforms, checksums, and publishing"
echo ""
echo "The release becomes visible when at least one platform asset and its checksum are ready."
