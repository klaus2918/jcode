#!/usr/bin/env bash
# Shared GitHub release API helpers for the release tooling.
#
# Before this file existed, verify-release.sh, retrigger-release.sh and
# delete-draft-release.sh each carried their own copy of the token lookup, and
# the latter two plus verify-release.sh each had their own HTTP implementation
# (curl / python3 / PowerShell variants). Source this file instead:
#
#   # shellcheck source=scripts/lib/release_api.sh
#   . "$(dirname "$0")/lib/release_api.sh"
#   JCODE_API_TOKEN="$(jcode_release_token)"
#
# Token precedence: JCODE_API_TOKEN -> GH_TOKEN -> GITHUB_TOKEN -> git credential.
# Anonymous GitHub API requests are capped at 60/hour per IP, which release
# verification exhausts easily (observed as HTTP 403), so authenticate whenever a
# token is available.

# Echo the GitHub token (possibly empty). Diagnostics never go to stdout.
jcode_release_token() {
  local token="${JCODE_API_TOKEN:-${GH_TOKEN:-${GITHUB_TOKEN:-}}}"
  if [ -z "$token" ] && command -v git >/dev/null 2>&1; then
    local cred
    cred="$(mktemp)"
    # `timeout` guards the interactive-prompt path where it exists.
    if command -v timeout >/dev/null 2>&1; then
      printf 'protocol=https\nhost=github.com\n\n' | timeout 15 git credential fill > "$cred" 2>/dev/null || true
    else
      printf 'protocol=https\nhost=github.com\n\n' | git credential fill > "$cred" 2>/dev/null || true
    fi
    token="$(sed -n 's/^password=//p' "$cred" | head -1)"
    rm -f "$cred"
  fi
  printf '%s' "$token"
}

# GET a GitHub API URL. Prints the body on success; returns non-zero when every
# available transport fails (mirrors the "curl is blocked on this host, python3
# works" reality observed on the dev machine).
jcode_release_fetch() {
  local url="$1"
  local token="${JCODE_API_TOKEN:-${GH_TOKEN:-${GITHUB_TOKEN:-}}}"
  local out=""
  local auth_args=()
  if [ -n "$token" ]; then
    auth_args=(-H "Authorization: Bearer $token")
  fi

  if command -v curl >/dev/null 2>&1; then
    out="$(curl -fsS --max-time 20 "${auth_args[@]}" \
      -H 'User-Agent: jcode-release' -H 'Accept: application/vnd.github+json' \
      "$url" 2>/dev/null || true)"
    if [ -n "$out" ]; then
      printf '%s' "$out"
      return 0
    fi
  fi

  if command -v python3 >/dev/null 2>&1; then
    out="$(JCODE_API_TOKEN="$token" python3 -c '
import os, sys, urllib.request
headers = {"User-Agent": "jcode-release", "Accept": "application/vnd.github+json"}
token = os.environ.get("JCODE_API_TOKEN", "")
if token:
    headers["Authorization"] = "Bearer " + token
req = urllib.request.Request(sys.argv[1], headers=headers)
try:
    with urllib.request.urlopen(req, timeout=25) as r:
        sys.stdout.write(r.read().decode())
except Exception:
    pass
' "$url" 2>/dev/null || true)"
    if [ -n "$out" ]; then
      printf '%s' "$out"
      return 0
    fi
  fi

  if command -v powershell >/dev/null 2>&1; then
    out="$(JCODE_API_TOKEN="$token" powershell -NoProfile -Command "
      try {
        \$h = @{ 'User-Agent' = 'jcode-release'; Accept = 'application/vnd.github+json' }
        if (\$env:JCODE_API_TOKEN) { \$h['Authorization'] = 'Bearer ' + \$env:JCODE_API_TOKEN }
        Invoke-RestMethod -Uri '$url' -Headers \$h -TimeoutSec 25 -UseBasicParsing | ConvertTo-Json -Depth 6 -Compress
      } catch { }
    " 2>/dev/null || true)"
    if [ -n "$out" ]; then
      printf '%s' "$out"
      return 0
    fi
  fi

  return 1
}
