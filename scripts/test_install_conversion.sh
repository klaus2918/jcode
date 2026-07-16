#!/usr/bin/env bash
set -euo pipefail

repo_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/bin" "$tmp/home" "$tmp/install"

cat > "$tmp/bin/uname" <<'EOF'
#!/usr/bin/env bash
case "${1:-}" in
  -s) printf 'Linux\n' ;;
  -m) printf 'x86_64\n' ;;
  *) printf 'Linux\n' ;;
esac
EOF

cat > "$tmp/bin/curl" <<'EOF'
#!/usr/bin/env bash
output=""
payload=""
url=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    -o) output="$2"; shift 2 ;;
    --data) payload="$2"; shift 2 ;;
    http*) url="$1"; shift ;;
    *) shift ;;
  esac
done
case "$url" in
  *telemetry.jcode.sh*) printf '%s\n' "$payload" >> "$INSTALL_TELEMETRY_LOG" ;;
  *api.github.com*)
    [ "${FAIL_RELEASE:-0}" != "1" ] || exit 22
    printf '{"tag_name":"v1.2.3"}\n'
    ;;
  *github.com*/releases/download/*)
    [ -n "$output" ] || exit 2
    printf 'fake archive' > "$output"
    ;;
  *) exit 2 ;;
esac
EOF

cat > "$tmp/bin/tar" <<'EOF'
#!/usr/bin/env bash
dest=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    -C) dest="$2"; shift 2 ;;
    *) shift ;;
  esac
done
cat > "$dest/jcode-linux-x86_64" <<'BIN'
#!/usr/bin/env bash
if [ "${1:-}" = "--version" ]; then printf 'jcode 1.2.3\n'; fi
BIN
chmod +x "$dest/jcode-linux-x86_64"
EOF
chmod +x "$tmp/bin/uname" "$tmp/bin/curl" "$tmp/bin/tar"

conversion_id="11111111-2222-4333-8444-555555555555"
telemetry_log="$tmp/telemetry.jsonl"
PATH="$tmp/bin:$PATH" \
HOME="$tmp/home" \
JCODE_HOME="$tmp/home/.jcode" \
JCODE_INSTALL_DIR="$tmp/install" \
JCODE_INSTALL_CONVERSION_ID="$conversion_id" \
JCODE_SKIP_SERVER_RELOAD=1 \
INSTALL_TELEMETRY_LOG="$telemetry_log" \
bash "$repo_dir/scripts/install.sh" >/dev/null

test "$(cat "$tmp/home/.jcode/install_conversion_id")" = "$conversion_id"
grep -q '"stage":"installer_start".*"outcome":"success"' "$telemetry_log"
grep -q '"stage":"installer_finish".*"outcome":"success"' "$telemetry_log"

failure_log="$tmp/failure.jsonl"
if PATH="$tmp/bin:$PATH" \
  HOME="$tmp/home-failure" \
  JCODE_HOME="$tmp/home-failure/.jcode" \
  JCODE_INSTALL_DIR="$tmp/install-failure" \
  JCODE_INSTALL_CONVERSION_ID="$conversion_id" \
  JCODE_SKIP_SERVER_RELOAD=1 \
  INSTALL_TELEMETRY_LOG="$failure_log" \
  FAIL_RELEASE=1 \
  bash "$repo_dir/scripts/install.sh" >/dev/null 2>&1; then
  echo "expected release lookup failure" >&2
  exit 1
fi
grep -q '"stage":"installer_finish".*"outcome":"failure".*"failure_stage":"release_lookup"' "$failure_log"

privacy_log="$tmp/privacy.jsonl"
PATH="$tmp/bin:$PATH" \
HOME="$tmp/home-private" \
JCODE_HOME="$tmp/home-private/.jcode" \
JCODE_INSTALL_DIR="$tmp/install-private" \
JCODE_INSTALL_CONVERSION_ID="$conversion_id" \
JCODE_SKIP_SERVER_RELOAD=1 \
JCODE_NO_TELEMETRY=1 \
INSTALL_TELEMETRY_LOG="$privacy_log" \
bash "$repo_dir/scripts/install.sh" >/dev/null
test ! -e "$privacy_log"
test ! -e "$tmp/home-private/.jcode/install_conversion_id"

echo "installer conversion telemetry tests passed"
