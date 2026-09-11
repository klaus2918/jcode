#!/usr/bin/env bash
# 重新触发一次首发：在同一个 commit 上删除并重建远端 tag。
#
# 用途：fork 的 Actions 刚被启用时，启用前推送的 tag 事件已被 GitHub 静默丢弃，
# 不会自动补跑（见 docs/发布流程.md §4.2）。
#
# 安全前提（脚本会逐条强制检查，任一不满足即拒绝执行）：
#   1. tag 确实存在，且本地与远端指向同一个 commit
#   2. 该 tag 没有任何 release（即无 release / 无资产 / 可安全重建）
#   3. 本地 release 元数据（Cargo.toml、changelog）已全部提交，工作区干净
#
# 用法：
#   scripts/retrigger-release.sh --dry-run v0.65.0   # 只检查前提，不动任何东西
#   scripts/retrigger-release.sh v0.65.0             # 实际执行删除+重建+推送
#   scripts/retrigger-release.sh --assume-no-release v0.65.0   # 网络查不到 API 时
set -euo pipefail

DRY_RUN=false
ASSUME_NO_RELEASE=false
while [[ "${1:-}" == --* ]]; do
  case "$1" in
    --dry-run) DRY_RUN=true ;;
    # 受限网络（如企业代理只放行部分流量）下 curl 不通时使用。
    # 使用前必须自己在 GitHub 上确认该 tag 没有 release，否则会违反
    # docs/发布流程.md §10.3（禁止重建已有 release 的同名 tag）。
    --assume-no-release) ASSUME_NO_RELEASE=true ;;
    *) echo "Error: unknown option: $1" >&2; exit 1 ;;
  esac
  shift
done

TAG="${1:?Usage: $0 [--dry-run] vX.Y.Z}"
REPO_SLUG="${GITHUB_REPOSITORY:-klaus2918/jcode}"

if [[ ! "$TAG" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "Error: tag must look like v0.65.0 (got: $TAG)" >&2
  exit 1
fi

cd "$(git rev-parse --show-toplevel)"

fail() { echo "REFUSED: $*" >&2; exit 1; }

# 抓取 GitHub API。依次尝试 curl / python3 / PowerShell：
# 受限网络下 curl 可能被拦截（本机实测 curl 返回 000），但 python 或
# PowerShell 可能仍通，因此不能只依赖 curl。
fetch_api() {
  local url="$1"

  if command -v curl >/dev/null 2>&1; then
    out="$(curl -fsS --max-time 20 "$url" 2>/dev/null || true)"
    [[ -n "$out" ]] && { printf '%s' "$out"; return 0; }
  fi

  if command -v python3 >/dev/null 2>&1; then
    out="$(python3 -c '
import sys, urllib.request
try:
    with urllib.request.urlopen(sys.argv[1], timeout=20) as r:
        sys.stdout.write(r.read().decode())
except Exception:
    pass
' "$url" 2>/dev/null || true)"
    [[ -n "$out" ]] && { printf '%s' "$out"; return 0; }
  fi

  if command -v powershell >/dev/null 2>&1; then
    out="$(powershell -NoProfile -Command "
      try {
        \$r = Invoke-RestMethod -Uri '$url' -TimeoutSec 20 -UseBasicParsing
        \$r | ConvertTo-Json -Depth 6 -Compress
      } catch { }
    " 2>/dev/null || true)"
    [[ -n "$out" ]] && { printf '%s' "$out"; return 0; }
  fi

  return 1
}

echo "=== 前置条件检查：$TAG ==="

# 1. tag 存在性与一致性
git rev-parse -q --verify "refs/tags/$TAG" >/dev/null \
  || fail "本地不存在 tag $TAG"
LOCAL_COMMIT="$(git rev-list -n 1 "$TAG")"

remote_line="$(git ls-remote --tags origin "refs/tags/$TAG^{}" "refs/tags/$TAG" 2>/dev/null || true)"
[[ -n "$remote_line" ]] || fail "远端不存在 tag $TAG"

REMOTE_COMMIT="$(printf '%s\n' "$remote_line" | awk '$2 ~ /\^\{\}$/ { print $1; found=1 } END { if (!found) print first } NR==1 { first=$1 }')"
echo "  本地 tag commit: $LOCAL_COMMIT"
echo "  远端 tag commit: $REMOTE_COMMIT"
[[ "$LOCAL_COMMIT" == "$REMOTE_COMMIT" ]] \
  || fail "本地与远端 tag 指向不同 commit，需人工判断，脚本不处理"

# 2. 该 tag 还没有任何 release（否则绝不重建同名 tag）
#
# 默认必须查得到 API 结果才放行。查询失败时**拒绝执行**而不是放行：
# 在无法确认「没有 release」的情况下重建 tag，可能破坏已发布资产的不可变性。
if [[ "$ASSUME_NO_RELEASE" == "true" ]]; then
  echo "  [警告] 跳过了 release 查询（--assume-no-release）"
  echo "        你必须在 https://github.com/$REPO_SLUG/releases 亲自确认 $TAG 无 release"
else
  if ! releases="$(fetch_api "https://api.github.com/repos/$REPO_SLUG/releases?per_page=100")"; then
    fail "无法查询 release 列表（curl/python3/PowerShell 全部不可达，或 API 限流）。\
若你已在网页上确认 $TAG 无 release，可加 --assume-no-release 重跑"
  fi
  if printf '%s' "$releases" | grep -Fq "\"tag_name\": \"$TAG\"" \
     || printf '%s' "$releases" | grep -Fq "\"tag_name\":\"$TAG\""; then
    fail "已存在 $TAG 的 release，禁止重建同名 tag（改用更高版本，见 docs/发布流程.md §10.3）"
  fi
  echo "  已确认：$TAG 无任何 release（无资产、无消费者）"
fi

# 3. release 元数据已全部提交
dirty="$(git status --porcelain -- Cargo.toml Cargo.lock changelog/ 2>/dev/null || true)"
[[ -z "$dirty" ]] || fail "release 元数据存在未提交改动：$dirty"
echo "  release 元数据工作区干净"

# 4. 该 commit 已是远端 master 的祖先（避免 tag 指向未推送的提交）
git merge-base --is-ancestor "$LOCAL_COMMIT" origin/master \
  || fail "tag 指向的 commit 未出现在 origin/master 上"
echo "  tag 指向的 commit 已在 origin/master 上"

# 5. tag 名与 tag 指向的提交里的 Cargo.toml 版本必须一致。
#
# 否则会出现「tag 是 v0.65.0，但二进制报 0.64.x」的发布事故：
# 发布流程要求在打 tag 前先单独提交版本元数据（§5.1），这一步很容易漏。
manifest_version="$(git show "$LOCAL_COMMIT:Cargo.toml" 2>/dev/null \
  | sed -n 's/^version[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' | head -1)"
[[ -n "$manifest_version" ]] || fail "无法从 $LOCAL_COMMIT 的 Cargo.toml 读取版本"
if [[ "v$manifest_version" != "$TAG" ]]; then
  fail "tag 为 $TAG，但该提交的 Cargo.toml 版本为 $manifest_version。\
两者不一致，发布产物会报错误版本；先提交版本元数据再重打 tag（见 docs/发布流程.md §5.1）"
fi
echo "  Cargo.toml 版本与 tag 一致（$manifest_version）"

# 6. changelog 条目存在，否则发行说明会退化成全量提交清单（§5.1）
changelog_path="changelog/v${manifest_version}.json"
git cat-file -e "$LOCAL_COMMIT:$changelog_path" 2>/dev/null \
  || fail "$LOCAL_COMMIT 缺少 $changelog_path；发行说明会退化成提交清单，请先补上"
echo "  changelog 条目存在（$changelog_path）"

echo
if $DRY_RUN; then
  echo "=== dry-run：全部前置条件通过，未做任何修改 ==="
  echo "实际执行时将运行："
  echo "  git push origin :refs/tags/$TAG"
  echo "  git tag -f -a $TAG -m \"$TAG\" $LOCAL_COMMIT"
  echo "  git push origin $TAG"
  exit 0
fi

echo "=== 删除并重建 tag 以重新触发流水线 ==="
echo "▸ 删除远端 tag..."
git push origin ":refs/tags/$TAG"
echo "▸ 在同一 commit 上重建 tag..."
git tag -f -a "$TAG" -m "$TAG" "$LOCAL_COMMIT"
echo "▸ 推送 tag..."
git push origin "$TAG"

echo
echo "=== 已重新触发 ==="
echo "  ✅ $TAG -> $LOCAL_COMMIT 已重新推送"
echo "  ⏳ 请在 Actions 页确认 Release 流水线已启动"
