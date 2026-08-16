#!/usr/bin/env bash
set -euo pipefail

# Generate SHA256SUMS covering every release asset in a directory.
#
# Mirrors the CI convention (release.yml "Generate checksums" step): one line
# per asset, "<sha256>  <filename>" (two spaces), sorted by filename, covering
# only the release asset types (*.tar.gz, *.exe, *.zip). The manifest itself
# is never listed.
#
# Usage:
#   scripts/generate_checksums.sh <dir> [output-file] [--recursive]
#
# With no output-file the manifest is written to <dir>/SHA256SUMS.
# --recursive scans subdirectories (the CI artifacts layout, matching the
# release.yml "Generate checksums" step); the default scans only <dir>'s
# top level (the local dist layout).

if [[ "$#" -lt 1 || "$#" -gt 3 ]]; then
    echo "Usage: $0 <dir> [output-file] [--recursive]" >&2
    exit 1
fi

src_dir="$1"
out_file="${2:-$src_dir/SHA256SUMS}"
recursive=false
if [[ "$#" -eq 3 ]]; then
    [[ "$3" == "--recursive" ]] || { echo "Error: unknown option: $3" >&2; exit 1; }
    recursive=true
fi

[[ -d "$src_dir" ]] || { echo "Error: not a directory: $src_dir" >&2; exit 1; }

if [[ "$recursive" == true ]]; then
    mapfile -t assets < <(
        find "$src_dir" -type f \
            \( -name '*.tar.gz' -o -name '*.exe' -o -name '*.zip' \) \
            -printf '%P\n' | LC_ALL=C sort
    )
else
    mapfile -t assets < <(
        find "$src_dir" -maxdepth 1 -type f \
            \( -name '*.tar.gz' -o -name '*.exe' -o -name '*.zip' \) \
            -printf '%P\n' | LC_ALL=C sort
    )
fi

if (( ${#assets[@]} == 0 )); then
    echo "Error: no release assets (*.tar.gz, *.exe, *.zip) found in $src_dir" >&2
    exit 1
fi

: > "$out_file"
for asset in "${assets[@]}"; do
    name="$(basename "$asset")"
    hash="$(sha256sum "$src_dir/$asset" | awk '{print $1}')"
    printf '%s  %s\n' "$hash" "$name" >> "$out_file"
done

echo "SHA256SUMS: ${#assets[@]} asset(s) -> $out_file"