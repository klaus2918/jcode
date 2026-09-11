#!/usr/bin/env bash
# 自动执行 docs/发布流程.md §8 的发布后核对清单。
#
# 做四件事：
#   1. 核对 release 元数据：已公开、资产集合恰好为 exe + tar.gz + SHA256SUMS
#   2. 下载全部资产，逐项校验 SHA256SUMS 的覆盖范围与哈希
#   3. 核对发行说明：是 changelog 渲染结果、标注 Windows 专用、无误标其他平台
#   4. 运行产物：--version 与 tag 一致、无 -dev 后缀；tar.gz 解包内容与独立 exe 一致
#
# 第 4 项只能在 Windows 上执行（产物是 Windows 二进制）；其他平台会跳过并明确提示，
# 但仍完成 1–3 项。
#
# 用法：
#   scripts/verify-release.sh                # 核对 latest
#   scripts/verify-release.sh v0.65.0        # 核对指定 tag
#   scripts/verify-release.sh --skip-download v0.65.0   # 跳过大文件（只看元数据与发行说明）
set -euo pipefail

SKIP_DOWNLOAD=0
if [[ "${1:-}" == "--skip-download" ]]; then
  SKIP_DOWNLOAD=1
  shift
fi

REPO_SLUG="${GITHUB_REPOSITORY:-klaus2918/jcode}"
TAG="${1:-}"

cd "$(git rev-parse --show-toplevel)"

# 有 git 凭据时带上 Authorization：匿名请求每 IP 每小时仅 60 次，核对很容易把它用尽。
# 加 timeout 保护：无凭据且需交互的环境中 credential fill 可能阻塞。
cred="$(mktemp)"
trap 'rm -f "$cred"' EXIT
printf 'protocol=https\nhost=github.com\n\n' | timeout 15 git credential fill > "$cred" 2>/dev/null || true
JCODE_API_TOKEN="$(sed -n 's/^password=//p' "$cred" | head -1)"
JCODE_REPO="$REPO_SLUG" JCODE_TAG="$TAG" JCODE_API_TOKEN="$JCODE_API_TOKEN" \
JCODE_SKIP_DOWNLOAD="$SKIP_DOWNLOAD" \
PYTHONUTF8=1 PYTHONIOENCODING=utf-8 python3 - <<'PY'
import hashlib
import json
import os
import pathlib
import platform
import subprocess
import sys
import tarfile
import tempfile
import urllib.error
import urllib.request

repo = os.environ["JCODE_REPO"]
tag = os.environ["JCODE_TAG"]
token = os.environ.get("JCODE_API_TOKEN", "")

headers = {"User-Agent": "jcode-verify-release", "Accept": "application/vnd.github+json"}
if token:
    headers["Authorization"] = f"Bearer {token}"

failures = []


def check(label, ok, detail=""):
    print(f"[{'PASS' if ok else 'FAIL'}] {label}" + (f"  ({detail})" if detail else ""))
    if not ok:
        failures.append(label)


def get(url):
    request = urllib.request.Request(url, headers=headers)
    with urllib.request.urlopen(request, timeout=120) as response:
        return response.read()


# --- release 元数据 ---
try:
    if tag:
        rel = json.loads(get(f"https://api.github.com/repos/{repo}/releases/tags/{tag}"))
    else:
        rel = json.loads(get(f"https://api.github.com/repos/{repo}/releases/latest"))
except urllib.error.HTTPError as exc:
    print(f"无法读取 release（HTTP {exc.code}）。若为 403，请确认本机有 github.com 的 git 凭据。")
    sys.exit(1)

tag = rel["tag_name"]
version = tag.lstrip("v")
print(f"=== 核对 {tag}（{rel['html_url']}）===")
check("release 已公开（非草稿）", rel["draft"] is False)
check("release 非 prerelease", rel["prerelease"] is False)

names = sorted(a["name"] for a in rel["assets"])
expected = ["SHA256SUMS", "jcode-windows-x86_64.exe", "jcode-windows-x86_64.tar.gz"]
check("资产集合恰好为 exe + tar.gz + SHA256SUMS", names == expected, str(names))

body = rel.get("body") or ""
check("发行说明是 changelog 渲染（含 Highlights/Improvements）", "### Highlights" in body and "### Improvements" in body)
check("发行说明标注 Windows x86_64 可用", "Windows x86_64: available" in body)
for other in ("Linux x86_64: available", "macOS Apple Silicon: available",
              "FreeBSD x86_64: available", "Windows ARM64: available"):
    check(f"发行说明无误标「{other.split(':')[0]} 可用」", other not in body)

assets = {a["name"]: a for a in rel["assets"]}
if set(names) != set(expected):
    print("\n资产集合不符，后续检查无法继续")
    print(f"FAILURES: {len(failures)}")
    sys.exit(1)

# --- 下载并校验哈希（--skip-download 时跳过）---
if os.environ.get("JCODE_SKIP_DOWNLOAD") == "1":
    print("\n[SKIP] --skip-download：跳过资产下载、哈希、运行与解包检查")
    print("       （元数据与发行说明的检查已完成）")
    print(f"\nFAILURES: {len(failures)}")
    sys.exit(1 if failures else 0)

work = pathlib.Path(tempfile.mkdtemp(prefix="jcode-verify-"))
print(f"\n下载目录：{work}")
print("  说明：exe 约 86MB、tar.gz 约 30MB，在慢链路上可能需要数分钟；每件下载完会立即输出一行。")
print("  只要看到下一行的「已下载」就不会是卡死。若只想看元数据与说明，用 --skip-download。")
downloaded = {}
for name in expected:
    size = assets[name].get("size") or 0
    print(f"  下载中 {name}（{size / 1048576:.1f} MB）...", flush=True)
    data = get(assets[name]["browser_download_url"])
    path = work / name
    path.write_bytes(data)
    downloaded[name] = path
    print(f"  已下载 {name}：{len(data)} bytes", flush=True)

sums = {}
for line in downloaded["SHA256SUMS"].read_text(encoding="utf-8").splitlines():
    if line.strip():
        digest, fname = line.split("  ", 1)
        sums[fname.strip()] = digest.strip()
check("SHA256SUMS 恰好覆盖两个产物",
      sorted(sums) == ["jcode-windows-x86_64.exe", "jcode-windows-x86_64.tar.gz"], str(sorted(sums)))

for name, digest in sums.items():
    actual = hashlib.sha256(downloaded[name].read_bytes()).hexdigest()
    check(f"哈希匹配：{name}", actual == digest, "" if actual == digest else f"{digest[:16]}… != {actual[:16]}…")

# --- 运行产物（仅 Windows） ---
is_windows = os.name == "nt" or "windows" in platform.system().lower() or bool(os.environ.get("MSYSTEM"))
if not is_windows:
    print("\n[SKIP] 当前不是 Windows，跳过 --version 与解包运行检查（产物为 Windows 二进制）")
else:
    exe = downloaded["jcode-windows-x86_64.exe"]
    result = subprocess.run([str(exe), "--version"], capture_output=True, text=True, timeout=180)
    first = ((result.stdout + result.stderr).strip().splitlines() or [""])[0]
    print(f"\n  exe --version => {first!r} (exit={result.returncode})")
    check("exe 可运行（退出码 0）", result.returncode == 0)
    check("exe 版本号与 tag 一致", version in first, first)
    check("exe 版本号无 -dev 后缀", "-dev" not in first, first)

    extract = work / "extracted"
    extract.mkdir()
    with tarfile.open(downloaded["jcode-windows-x86_64.tar.gz"]) as tf:
        members = tf.getnames()
        tf.extractall(extract)
    check("tar.gz 内含 exe", any(m.endswith("jcode-windows-x86_64.exe") for m in members), str(members))

    inner = next((extract / m for m in members if m.endswith("jcode-windows-x86_64.exe")), None)
    if inner and inner.is_file():
        result2 = subprocess.run([str(inner), "--version"], capture_output=True, text=True, timeout=180)
        first2 = ((result2.stdout + result2.stderr).strip().splitlines() or [""])[0]
        print(f"  解包后 --version => {first2!r} (exit={result2.returncode})")
        check("解包后二进制可运行", result2.returncode == 0)
        check("解包后版本号与 tag 一致", version in first2, first2)
        check("解包内容与独立 exe 哈希一致",
              hashlib.sha256(inner.read_bytes()).hexdigest() == hashlib.sha256(exe.read_bytes()).hexdigest())

print()
print(f"FAILURES: {len(failures)}")
for item in failures:
    print("  -", item)
sys.exit(1 if failures else 0)
PY
