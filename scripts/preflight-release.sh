#!/usr/bin/env bash
# 发布前自检：确认当前 master 的发布链路自洽，可以安全打 tag。
#
# 只读操作：不创建 tag、不推送、不构建。建议在 scripts/quick-release.sh 之前跑。
#
# 用法：
#   scripts/preflight-release.sh          # 检查当前 HEAD
#   scripts/preflight-release.sh v0.65.1  # 额外检查该 tag 是否已存在
set -uo pipefail

TAG_ARG="${1:-}"
cd "$(git rev-parse --show-toplevel)"

# 参数校验：只看 vX.Y.Z 形式的 tag，其他值直接报错。
# （不静默忽略：否则打错参数时自检会“通过”，掩盖真实问题。）
if [ -n "$TAG_ARG" ] && ! printf '%s' "$TAG_ARG" | grep -Eq '^v[0-9]+\.[0-9]+\.[0-9]+$'; then
  echo "用法：$0 [vX.Y.Z]" >&2
  echo "错误：可选参数必须是形如 v0.65.1 的 tag（收到：'$TAG_ARG'）" >&2
  exit 2
fi

fail=0
chk() {
  if [ "$2" = "1" ]; then
    echo "[PASS] $1"
  else
    echo "[FAIL] $1"
    fail=$((fail + 1))
  fi
}

echo "=== 1. 版本与锁文件一致性 ==="
cargo_ver="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
lock_ver="$(sed -n '/^name = "jcode"$/{n;s/^version = "\(.*\)"/\1/p;q}' Cargo.lock)"
t=0; [ "$cargo_ver" = "$lock_ver" ] && t=1
chk "Cargo.toml($cargo_ver) == Cargo.lock($lock_ver)" "$t"

cargo metadata --locked --offline --format-version 1 >/dev/null 2>&1 && t=1 || t=0
chk "cargo metadata --locked 通过" "$t"

if [ -n "$TAG_ARG" ]; then
  t=0; [ "v$cargo_ver" = "$TAG_ARG" ] && t=1
  chk "tag($TAG_ARG) 与版本(v$cargo_ver) 匹配" "$t"
  t=0; [ -f "changelog/v$cargo_ver.json" ] && t=1
  chk "changelog/v$cargo_ver.json 存在（否则发行说明退化为提交清单）" "$t"
fi

echo
echo "=== 2. 发布工作流结构（Windows 专用 + 必达资产）==="
PYTHONUTF8=1 python3 - <<'PY'
import pathlib, sys
try:
    import yaml
except ImportError:
    print("[SKIP] pyyaml 不可用，跳过工作流结构检查")
    sys.exit(0)

path = pathlib.Path(".github/workflows/release.yml")
text = path.read_text(encoding="utf-8")
doc = yaml.safe_load(text)
jobs = doc["jobs"]
ok = True

expected = ["create-release", "build-windows", "publish-windows", "release"]
if list(jobs) != expected:
    print(f"[FAIL] 作业集合 {list(jobs)} != {expected}")
    ok = False
else:
    print(f"[PASS] 作业集合 = {expected}")

if jobs["release"]["needs"] != ["create-release", "publish-windows"]:
    print(f"[FAIL] release.needs = {jobs['release'].get('needs')}")
    ok = False
else:
    print("[PASS] release 只等 Windows 链路")

for needle, label in [
    ("artifacts/jcode-windows-x86_64/jcode-windows-x86_64.exe", "必达资产：exe"),
    ("artifacts/jcode-windows-x86_64/jcode-windows-x86_64.tar.gz", "必达资产：tar.gz"),
    ("verify_windows_install.ps1", "安装器门禁"),
    ("generate_checksums.sh", "校验和生成"),
    ("generate_release_notes.sh", "发行说明生成"),
    ("--draft=false --latest", "release 转正"),
]:
    if needle in text:
        print(f"[PASS] {label}")
    else:
        print(f"[FAIL] 缺少 {label}")
        ok = False

code = "\n".join(l for l in text.splitlines() if not l.lstrip().startswith("#"))
residue = [s for s in ("freebsd", "apple-darwin", "linux-gnu", "windows-aarch64", "DEPLOY_KEY") if s in code]
if residue:
    print(f"[FAIL] 残留引用：{residue}")
    ok = False
else:
    print("[PASS] 无其他平台 / DEPLOY_KEY 残留")

sys.exit(0 if ok else 1)
PY
[ $? -ne 0 ] && fail=$((fail + 1))

echo
echo "=== 3. 发布辅助脚本可用 ==="
for s in generate_release_notes.sh generate_checksums.sh quick-release.sh retrigger-release.sh verify-release.sh delete-draft-release.sh; do
  if [ -f "scripts/$s" ]; then
    if bash -n "scripts/$s" 2>/dev/null; then
      echo "[PASS] scripts/$s（存在且语法通过）"
    else
      echo "[FAIL] scripts/$s 语法错误"
      fail=$((fail + 1))
    fi
  else
    echo "[WARN] scripts/$s 不存在"
  fi
done

echo
echo "=== 4. 工作区与远端状态 ==="
dirty="$(git status --porcelain -- Cargo.toml Cargo.lock changelog/ 2>/dev/null || true)"
t=0; [ -z "$dirty" ] && t=1
chk "release 元数据工作区干净" "$t"
[ -n "$dirty" ] && printf '    %s\n' "$dirty"

git fetch origin -q 2>/dev/null || true
diff_count="$(git rev-list --left-right --count origin/master...HEAD 2>/dev/null || echo '?')"
t=0; [ "$diff_count" = "0	0" ] && t=1
chk "本地与 origin/master 同步（$diff_count）" "$t"

echo
echo "  现有 tag: $(git tag -l 'v*' --sort=-v:refname | head -5 | tr '\n' ' ')"

echo
if [ "$fail" -eq 0 ]; then
  echo "✅ 发布前自检通过，可以打 tag"
  exit 0
fi
echo "❌ 发布前自检失败：$fail 项"
exit 1
