#!/usr/bin/env bash
set -euo pipefail

# Verify a SHA256SUMS manifest against the files in its directory.
#
# Usage:
#   scripts/verify_dist.sh <dir>
#
# Reads <dir>/SHA256SUMS (one "<sha256>  <filename>" per line, the sha256sum
# convention also produced by scripts/generate_checksums.sh) and checks every
# entry: the file must exist in <dir> and its sha256 must match the recorded
# hash. Missing files are reported as MISSING, hash mismatches as MISMATCH,
# malformed lines as MALFORMED; any problem exits 1, a fully verified
# manifest prints OK and exits 0.

if [[ "$#" -ne 1 ]]; then
    echo "Usage: $0 <dir>" >&2
    exit 1
fi

dir="$1"
sums_file="$dir/SHA256SUMS"

[[ -d "$dir" ]] || { echo "Error: not a directory: $dir" >&2; exit 1; }
[[ -f "$sums_file" ]] || { echo "Error: missing manifest: $sums_file" >&2; exit 1; }

problems=0
entries=0
while IFS= read -r line || [[ -n "$line" ]]; do
    line="${line%$'\r'}"
    [[ -n "$line" ]] || continue
    [[ "$line" != \#* ]] || continue
    entries=$((entries + 1))

    hash="${line%%  *}"
    name="${line#*  }"
    if [[ ! "$hash" =~ ^[0-9a-fA-F]{64}$ ]]; then
        echo "MALFORMED: $line" >&2
        problems=$((problems + 1))
        continue
    fi
    if [[ -z "$name" || "$name" == "$line" ]]; then
        echo "MALFORMED: $line" >&2
        problems=$((problems + 1))
        continue
    fi
    if [[ ! -f "$dir/$name" ]]; then
        echo "MISSING: $name" >&2
        problems=$((problems + 1))
        continue
    fi

    actual="$(sha256sum "$dir/$name" | awk '{print tolower($1)}')"
    if [[ "$actual" != "${hash,,}" ]]; then
        echo "MISMATCH: $name (expected $hash, got $actual)" >&2
        problems=$((problems + 1))
        continue
    fi
    echo "OK: $name"
done < "$sums_file"

if (( problems > 0 )); then
    echo "verify_dist: $problems problem(s) in $sums_file" >&2
    exit 1
fi
echo "verify_dist: all $entries entries verified OK ($sums_file)"