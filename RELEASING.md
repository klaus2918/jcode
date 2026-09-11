# 发布（本 fork）

本仓库 `klaus2918/jcode` 是 `1jehuang/jcode` 的 fork。发布链路已按本 fork 的条件改造过，
**完整规范以 [docs/发布流程.md](docs/发布流程.md) 为准**；本文只给差异速查与最短可操作路径。

## 一、与 upstream 的差异

| 维度 | upstream | 本 fork |
|---|---|---|
| 发布平台 | 7 个（Linux x86_64/aarch64、macOS aarch64/x86_64、Windows x86_64/aarch64、FreeBSD x86_64） | **仅 Windows x86_64** |
| 触发方式 | 推送 `v*` tag | 同上（`.github/workflows/release.yml`，且**没有** `workflow_dispatch`，只能靠 tag 触发） |
| 构建位置 | 本地 `scripts/quick-release.sh --fast-local`（Linux x86_64 主机）＋ CI 补其余平台 | 全部在 CI；本地只用 `--remote` 推 tag |
| 代码签名 | Azure Artifact Signing 为必达步骤 | 本仓库未配置签名账号，签名步骤自动跳过；二进制未签名，首次运行会有 SmartScreen 警告 |
| 包管理器 | 结束时更新 Homebrew 与 AUR（`scripts/update_packages.sh`） | 相关步骤已从 workflow 移除（它们依赖 Linux/macOS 资产）；脚本仍在仓库里，但 CI 不再调用 |
| 跨平台工具链 | osxcross 交叉编译 macOS | 不使用（无 macOS 产物） |
| 在线自更新 | 支持在线自更新 | 已移除，只支持 `jcode update --local <包>` 与 git 源码重建 |

## 二、最短发布路径

```bash
# 0. 工作区干净且与远端同步
git status --short
git pull --ff-only origin master

# 1. 只提交版本元数据（Cargo.toml / Cargo.lock / changelog/ 三类文件）
git add Cargo.toml Cargo.lock changelog/
git commit -m "release: vX.Y.Z"
git push origin master

# 2. 打 tag 并推送，触发 CI 发布（Windows 下用 Git Bash 执行）
bash scripts/quick-release.sh --remote vX.Y.Z
```

- 版本唯一真源是 `Cargo.toml` 的 `version`，tag 必须与其一致。
- 必须同时准备 `changelog/vX.Y.Z.json` 与 `changelog/index.json` 里的对应条目，
  否则发行说明会退化成一份提交清单。
- CI 先建 draft release，只有 Windows 双资产（`.exe` + `.tar.gz`）齐全才转为 public。

## 三、发布后核对

- [ ] release 已由 draft 变为 public 并标记 latest；
- [ ] 资产恰好为 `jcode-windows-x86_64.exe`、`jcode-windows-x86_64.tar.gz` 与 `SHA256SUMS`；
- [ ] 下载 `.exe` 执行 `--version`，输出与 tag 一致。

## 四、不再适用的 upstream 做法

以下内容来自 upstream 的发布流程，在本仓库**不要照做**：

- `scripts/quick-release.sh --fast-local` / `--prepare-fast`：要求 Linux x86_64 主机，
  与本 fork“构建只在 CI”的取向冲突（原因见 [docs/发布流程.md](docs/发布流程.md) §6）。
- osxcross 交叉编译（`~/.osxcross`、`aarch64-apple-darwin`、`~/.cargo/config.toml` 的
  Darwin linker 配置）：本 fork 不含 macOS 产物。
- Homebrew / AUR 发布：workflow 已移除对应步骤。
- upstream 的在线安装脚本：`scripts/install.sh` 与 `scripts/install.ps1` 仍指向 upstream 仓库与
  其元数据服务，本 fork 的产物请用 `jcode update --local <包>`（或
  `scripts/update_local_install.ps1`）安装。

## 五、仓库内相关文件

| 文件 | 作用 |
|---|---|
| `.github/workflows/release.yml` | 唯一的正式发布流水线（Windows x86_64） |
| `scripts/quick-release.sh` | 本地入口；本 fork 只用 `--remote` |
| `scripts/retrigger-release.sh` | tag 事件被丢弃后重新触发（先查前提再删建 tag） |
| `scripts/delete-draft-release.sh` | 清理异常中止留下的草稿 release |
| `scripts/generate_release_notes.sh` | 生成发行说明（CI 与本地共用） |
| `scripts/generate_checksums.sh` | 生成 `SHA256SUMS` |
| `changelog/` | 版本发行说明数据与 schema（见 `changelog/README.md`） |
| `scripts/update_packages.sh` | Homebrew / AUR 更新脚本；本 fork 的 CI 已不再调用 |

上游版本的 `RELEASING.md`（英文、约 198 行，含 osxcross 与包管理器章节）已随本文件重写删除；
需要查阅时用 git 历史。
