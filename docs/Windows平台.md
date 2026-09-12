# Windows 支持

Jcode 把 Windows 作为一等平台支持。Windows 实现使用原生命名管道、Windows 进程管理、PowerShell 安装和平台特定的启动集成。

> **注（2026-09-12，本 fork）**：
>
> - **不要用在线安装脚本安装本 fork 的产物。** `scripts/install.sh` / `install.ps1` 仍指向 upstream 的仓库与 `jcode.sh` 元数据服务；本 fork 的发布产物请用 `jcode update --local <包>` 或 `.\install.ps1 -ArtifactExePath ...` 本地安装（见下方"使用本地安装包安装 / 更新"）。发布规范见 [发布流程](发布流程.md)。
> - **发布产物未做 Authenticode 签名**（本仓库未配置 Azure Artifact Signing），因此首次运行会出现 SmartScreen 警告；CI 会把这个事实写进当次运行的 step summary。
> - **启动热键（全局快捷键）支持已彻底移除**：`setup-hotkey` 子命令、`-ConfigureHotkey`
>   参数均不存在，安装器也不再创建任何开机启动快捷方式（卸载器仍会清理旧版本留下的
>   快捷方式与 `%JCODE_HOME%\hotkey` 目录）。
>
> - **CI 只构建与验证 Windows**：上游的 ubuntu + macOS 构建矩阵与 FreeBSD 冒烟已删除
>   （2026-09-12），非 Windows 平台不再进入本 fork 的 CI。
>
> 下文的在线安装段落保留用于说明安装器行为，对本 fork 请按"本地安装包"一节操作。

## 支持状态

| 领域 | 状态 |
|---|---|
| Windows 11 x64 | **本 fork 唯一发布平台**，已在 CI 构建、签名（可选）与安装验证。CI 本身也已收敛为 Windows-only（2026-09-12 删除 ubuntu/macOS 构建作业与 FreeBSD 冒烟），仅保留在 ubuntu 上做跨平台静态检查与 Windows 目标的交叉编译 |
| Windows 11 ARM64 | 仅 `windows-smoke.yml` 编译并跑 `--version` 冒烟（`--no-default-features`），**不发布资产** |
| PowerShell 安装器 | 已在 Windows CI 测试 |
| 原生 IPC 与进程生命周期 | 由定向和端到端 Windows 测试覆盖 |
| `jcode update` | `--local <包>` 离线安装（版本号取自包内二进制）；不带 `--local` 时从 git 源码 `pull + cargo build --release` 重建。**没有在线自更新** |
| 发布资产 | 仅 `jcode-windows-x86_64.exe` 与 `jcode-windows-x86_64.tar.gz`，外加 `SHA256SUMS`（见 [发布流程](发布流程.md)） |
| Authenticode 签名 | **本 fork 未启用**：无 Azure Artifact Signing 配置，产物未签名（见文首注） |

安装器要求 PowerShell 5.1 或更高版本。x64 构建是 Intel 和 AMD Windows 电脑的默认选择。
安装器脚本仍保留 ARM64 分支（`scripts/install.ps1`），但**本 fork 不发布 ARM64 资产**，
因此在 ARM64 Windows 上走在线/资产安装会以可操作的错误结束；请改用 x64 资产
（在 Windows 11 ARM64 上通过 x64 仿真运行），或按 `-BuildFromSource` 自行源码构建。

## 安装

打开 PowerShell 并运行：

```powershell
irm https://jcode.sh/install.ps1 | iex
```

安装器：

1. 检测 x64 或 ARM64。
2. 从官方 GitHub 发布下载匹配资产。
3. 对照该发布的 `SHA256SUMS` 文件校验。
4. 在 `%LOCALAPPDATA%\jcode` 下安装不可变、稳定和启动器副本。
5. 把 `%LOCALAPPDATA%\jcode\bin` 添加到用户 `PATH`。

Alacritty 安装是可选的，不再自动安装。要显式请求：

```powershell
$script = [scriptblock]::Create((irm https://jcode.sh/install.ps1))
& $script -ConfigureAlacritty
```

Jcode 也可以在启动后交互式提供该选项。

> 本 fork 说明：全局启动热键不可用（见文首注），因此安装器没有对应开关。

### 使用本地安装包安装 / 更新（离线）

不访问官网或 GitHub，直接用本地提供的安装包安装 / 更新 jcode：

- **更新已装好的 jcode**：

  ```powershell
  jcode update --local C:\dist\jcode-windows-x86_64-<hash>.tar.gz
  jcode update --local C:\dist\jcode-windows-x86_64-<hash>.exe
  ```

  `.tar.gz` 会解压提取顶层文件，裸 `.exe` 直接安装；版本号从包内二进制自动探测，
  安装到 `%LOCALAPPDATA%\jcode\builds\versions\<version>\`，更新 stable 与 current
  两个通道（启动器切到 current 通道），并写 stable/current 版本标记。
  全程离线：不做发布查询、不下载、不取校验和。

  **通道语义**（Windows 三种安装路径各不相同，不要混为一谈）：

  - `install.ps1`（全新安装）：只维护 stable 通道，启动器 `%LOCALAPPDATA%\jcode\bin\jcode.exe`
    指向 stable 副本，不创建 current 通道。
  - `jcode update --local`（更新已装好的 jcode）：额外维护 current 通道，并把启动器
    切到 current（active）通道。
  - `scripts\update_local_install.ps1`（本地自研构建，单 exe 布局）：部署到
    `%JCODE_HOME%\bin\jcode.exe`（启动器）+ `%JCODE_HOME%\builds\current-release-lto\jcode.exe`
    （build slot，同一文件的副本），并持久化 `JCODE_HOME` 用户环境变量。

- **全新安装**：

  ```powershell
  .\install.ps1 -ArtifactExePath C:\dist\jcode-windows-x86_64-<hash>.exe
  .\install.ps1 -ArtifactTgzPath C:\dist\jcode-windows-x86_64-<hash>.tar.gz
  ```

本地安装包常带 git hash 后缀（如 `jcode-windows-x86_64-2dc3213a6.exe`），
上面两条路径与官方资产的识别和安装方式完全一致。

如果某个发布不包含匹配的预构建 Windows 资产，安装器会用可操作的错误消息失败，而不是静默开始长时间构建。要显式允许源码构建：

```powershell
$script = [scriptblock]::Create((irm https://jcode.sh/install.ps1))
& $script -BuildFromSource
```

源码构建需要 Git、Rust 和带 **使用 C++ 的桌面开发** 工作负载的 Visual Studio 2022 Build Tools。

### 安装路径

- 启动器：`%LOCALAPPDATA%\jcode\bin\jcode.exe`
- 稳定二进制：`%LOCALAPPDATA%\jcode\builds\stable\jcode.exe`
- 当前通道（active，`jcode update --local` 维护，启动器指向它）：`%LOCALAPPDATA%\jcode\builds\current\jcode.exe`
- 版本化二进制：`%LOCALAPPDATA%\jcode\builds\versions\<version>\jcode.exe`
- 用户数据与配置：`%USERPROFILE%\.jcode`

### 验证安装

```powershell
jcode --version
Get-Command jcode
Get-FileHash (Get-Command jcode).Source -Algorithm SHA256
```

把哈希与 release（本 fork 为 [klaus2918/jcode releases](https://github.com/klaus2918/jcode/releases)，上游为 [1jehuang/jcode releases](https://github.com/1jehuang/jcode/releases)）上的 `SHA256SUMS` 对比。
注意 `scripts/install.sh` / `install.ps1` 仍指向 upstream 仓库，本 fork 的产物请用本地包安装（见文首注）。

启用 Authenticode 签名后，以下命令必须报告 `Valid`：

```powershell
Get-AuthenticodeSignature (Get-Command jcode).Source | Format-List Status,StatusMessage,SignerCertificate
```

## 安装后验证清单

安装/更新完成后，**重启终端**（必要时注销重登），然后逐项验证。全部通过即安装就绪：

1. **`which jcode` 只解析到唯一落点**

   ```powershell
   Get-Command jcode | Select-Object -ExpandProperty Source
   ```

   只能输出一个路径（PowerShell 5.1 没有 `which`，用上面的命令；cmd 里用 `where jcode`）。
   官方安装应为 `%LOCALAPPDATA%\jcode\bin\jcode.exe`；本地自研构建更新
   （`update_local_install.ps1`）应为 `%JCODE_HOME%\bin\jcode.exe`。若输出多个落点，
   说明 PATH 里有残留 jcode 条目，用 `jcode update` 或卸载重装收敛到单一条目。

2. **版本号正确**

   ```powershell
   jcode --version
   ```

   输出应为刚安装/更新的版本号。

3. **cc-switch 冒烟（ping 实测 pong）**

   配置了 cc-switch 本地代理时，实测往返链路：

   ```powershell
   jcode --provider-profile cc-switch run "Reply with exactly: pong"
   ```

   应收到模型回复 `pong`（或含 `pong`）；代理不可达时该命令直接报连接失败。
   也可以在 cc-switch 代理面板确认请求已记录。此验证无需真实 API key
   （`auth = "none"`，key 由 cc-switch 代理注入）。

4. **JCODE_HOME 持久化检查（如设置了 JCODE_HOME）**

   通过 `update_local_install.ps1` 安装过的话，该脚本会把 JCODE_HOME 持久化为
   用户环境变量：

   ```powershell
   [Environment]::GetEnvironmentVariable('JCODE_HOME', 'User')
   ```

   应输出你的 JCODE_HOME 目录；重启后的新终端里 `$env:JCODE_HOME` 应与此一致，
   且第 1 步的 `jcode` 落点就在该目录的 `bin\` 下。

## Microsoft Defender 与 SmartScreen

两种不同的 Windows 警告常被混淆：

- **Microsoft Defender SmartScreen** 在下载的应用未签名或尚未积累足够发布者声誉时显示"Windows 已保护你的电脑"之类的消息。使用受信任、带时间戳的证书进行 Authenticode 签名是主要修复手段。新的发布者身份仍会随时间积累声誉。
- **Microsoft Defender 杀毒软件** 报告命名威胁或可疑行为。签名有助于确立来源，但启发式误报也必须连同精确的签名文件和 SHA-256 哈希提交给 Microsoft。

不要告诉用户禁用 Defender、添加排除项或绕过命名的恶意软件检测。首先验证发布 URL、校验和与 Authenticode 签名。如果正确签名的官方构建仍被检测，通过 [Microsoft 安全情报文件提交门户](https://www.microsoft.com/wdsi/filesubmission) 作为软件开发人员误报提交。

### 已就位的启发式触发减少措施

Windows 设置被刻意设计为避免不必要的行为可疑：

- 发布下载对照 `SHA256SUMS` 校验。
- 可选的终端设置要求明确同意。
- 安装器不创建任何开机启动项；旧的隐藏 VBScript 启动跳板已被移除。
- 全局热键支持已彻底移除（不再安装监听器，也不再有对应的安装开关）。
- 发布二进制在 GitHub 托管的 Windows 运行器上构建，并在发布前测试。

## 启用 Authenticode 签名

发布工作流支持带 GitHub OIDC 的 [Azure Artifact Signing](https://azure.microsoft.com/products/artifact-signing)。这把证书私钥保留在 Microsoft 托管签名服务中，而不是导出到 GitHub secret。

这是一次性的所有者设置，可能需要 Azure 计费和组织或身份验证：

1. 创建 Artifact Signing 账户和一个公共信任证书配置文件。
2. 创建带 `1jehuang/jcode` GitHub Actions 联合凭据的 Microsoft Entra 应用或托管身份。
3. 在该证书配置上授予它 **Artifact Signing Certificate Profile Signer** 角色。
4. 添加这些 GitHub Actions secrets：
   - `AZURE_CLIENT_ID`
   - `AZURE_TENANT_ID`
   - `AZURE_SUBSCRIPTION_ID`
5. 添加这些 GitHub Actions variables：
   - `WINDOWS_SIGNING_ENDPOINT`，例如 `https://eus.codesigning.azure.net/`
   - `WINDOWS_SIGNING_ACCOUNT`
   - `WINDOWS_SIGNING_CERTIFICATE_PROFILE`
6. 推送测试标签，确认 `Sign and publish Windows assets` 任务签署两个可执行文件，且 `Get-AuthenticodeSignature` 返回 `Valid`。
7. 让 `WINDOWS_SIGNING_REQUIRED` 保持未设置或设为 `true`。Windows 资产默认要求签名，因此配置缺失或签名中断会使该发布的 Windows 被省略，而其他每个成功的平台仍可发布。设为 `false` 是明确的紧急覆盖，不适合正式 Windows 发布。

工作流在打包、校验和生成和发布上传之前应用 SHA-256 Authenticode 签名和 RFC 3161 时间戳。x64 和 ARM64 可执行文件都在受支持的 x64 Windows 签名运行器上签名。

在签名强制生效且公开发布具有有效签名之前，不要把 Defender 和 SmartScreen 的推广描述为已完成。

## 发布验收清单

对于每个改变 Windows 行为的发布：

- [ ] Windows x64 CI 构建和定向测试通过。
- [ ] Windows 生命周期端到端测试通过。
- [ ] x64 安装器验证通过（`windows-smoke.yml` 的 ARM64 冒烟可选，且不发布资产）。
- [ ] 若已配置 Azure Artifact Signing：`.exe` 带有效、带时间戳的 Authenticode 签名；未配置时确认
      CI step summary 已写明未签名（签名在本 fork 是可选项，不是必达项）。
- [ ] `SHA256SUMS` 覆盖当次发布的两个 Windows 资产（`.exe` 与 `.tar.gz`）。
- [ ] 干净的 Windows 11 机器能成功安装、启动、更新和卸载。
- [ ] Defender 杀毒软件对签名发布不报告命名检测。
- [ ] SmartScreen 识别预期发布者。任何低声誉警告与恶意软件检测分开跟踪。
- [ ] 网站 Windows 按钮指向已发布资产，不包含预览或进行中措辞。
- [ ] 发布说明提及实质性的 Windows 修复或限制。

## 持续集成

Windows 由以下覆盖：

- `.github/workflows/ci.yml`：发布构建、测试编译、定向平台测试、运行时冒烟测试、生命周期端到端测试、安装器验证和 PowerShell 语法检查。
- `.github/workflows/windows-smoke.yml`：可手动触发的 x64 和 ARM64 冒烟验证。
- `.github/workflows/release.yml`：**仅 Windows x86_64** 一条链路（构建 → 可选签名 → 校验和 → 发布），
  没有 ARM64 作业；只发布 Windows x86_64 资产，缺资产则发布停留在 draft（详见 [发布流程](发布流程.md)）。

## 架构说明

Unix 域 socket 在 `crates/jcode-base/src/transport/windows.rs` 中被 Windows 命名管道替代。平台特定的文件系统、进程、更新和替换行为用 `#[cfg(windows)]` 在编译时选择，因此 Windows 支持不会给 Unix 构建增加运行时分支。

启动热键（全局快捷键）集成已随功能减法移除；如需要，请自行在系统层配置，jcode 不再安装或引导。

## 报告 Windows 问题

在 GitHub issue 中包含以下内容：

- Windows 版本、发行版和架构
- `jcode --version` 的 Jcode 版本
- 安装方式
- 终端和 PowerShell 版本（`$PSVersionTable.PSVersion`）
- 确切的 Defender 或 SmartScreen 消息
- 如果显示过，Defender 威胁名称
- `Get-FileHash` 的 SHA-256
- `Get-AuthenticodeSignature` 的 Authenticode 状态

不要上传私有配置、凭据、会话记录或 `.jcode` 认证文件。
