#!/usr/bin/env bash
# 删除一个**草稿** release（重建 tag 前清理用）。
#
# 安全约束：
#   - 只删 draft；已公开的 release 一律拒绝（那种情况必须发新版本，见 §10.3）
#   - 只删指定 tag，不做批量操作
#   - 删除前打印该 release 的资产清单，便于留痕
#
# 用法：
#   bash scripts/delete-draft-release.sh --dry-run v0.65.0
#   bash scripts/delete-draft-release.sh v0.65.0
set -euo pipefail

DRY_RUN=false
if [[ "${1:-}" == "--dry-run" ]]; then
  DRY_RUN=true
  shift
fi

TAG="${1:?Usage: $0 [--dry-run] vX.Y.Z}"
REPO_SLUG="${GITHUB_REPOSITORY:-klaus2918/jcode}"

cd "$(git rev-parse --show-toplevel)"

fail() { echo "REFUSED: $*" >&2; exit 1; }

tmp="$(mktemp)"
trap 'rm -f "$tmp"' EXIT
# 取凭据；加 timeout 保护：无凭据且需交互时 credential fill 可能阻塞。
printf 'protocol=https\nhost=github.com\n\n' | timeout 15 git credential fill > "$tmp" 2>/dev/null || true
TOKEN="$(sed -n 's/^password=//p' "$tmp" | head -1)"
[[ -n "$TOKEN" ]] || fail "git 中没有 github.com 的凭据，无法调用 API"

TAG="$TAG" REPO_SLUG="$REPO_SLUG" DRY_RUN="$DRY_RUN" GH_TOKEN_FOR_API="$TOKEN" \
PYTHONUTF8=1 PYTHONIOENCODING=utf-8 python3 - <<'PY'
import json
import os
import urllib.error
import urllib.request

tag = os.environ["TAG"]
repo = os.environ["REPO_SLUG"]
dry = os.environ["DRY_RUN"] == "true"
token = os.environ["GH_TOKEN_FOR_API"]

base = f"https://api.github.com/repos/{repo}"
headers = {
    "Authorization": f"Bearer {token}",
    "Accept": "application/vnd.github+json",
    "User-Agent": "jcode-draft-cleanup",
    "X-GitHub-Api-Version": "2022-11-28",
}


def call(method, url):
    req = urllib.request.Request(url, headers=headers, method=method)
    with urllib.request.urlopen(req, timeout=30) as resp:
        body = resp.read().decode()
        return json.loads(body) if body.strip() else None


releases = call("GET", f"{base}/releases?per_page=100")
target = next((r for r in releases if r["tag_name"] == tag), None)
if target is None:
    print(f"没有 {tag} 的 release，无需清理")
    raise SystemExit(0)

if not target["draft"]:
    print(f"REFUSED: {tag} 的 release 不是草稿（public），禁止删除。按 §10.3 发新版本。")
    raise SystemExit(1)

assets = [a["name"] for a in target.get("assets", [])]
print(f"目标：{tag}  draft=True  id={target['id']}")
print(f"资产（{len(assets)}）：{assets}")

if dry:
    print("dry-run：未删除任何内容")
    raise SystemExit(0)

call("DELETE", f"{base}/releases/{target['id']}")
print(f"已删除 {tag} 的草稿 release")
PY
