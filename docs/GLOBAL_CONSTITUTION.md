# 全局宪法配置说明（AGENTS.md）

本文说明 jcode 全局约束（"全局宪法"）的设计、读取位置与配置步骤，用于指导如何把统一的全局规则生效到所有项目、所有会话。

## 1. 设计概述

jcode 通过 **AGENTS.md** 机制注入全局指令。系统提示词在会话启动时构建，会自动读取并拼接若干指令文件：

- **项目级**：`<工作目录>/AGENTS.md`（标签 `Project Instructions (AGENTS.md)`）
- **全局级**：全局 AGENTS.md（标签 `Global Instructions (~/AGENTS.md)`）

全局级文件作用于所有项目、所有会话，正是"统一的全局约束"的落点。

## 2. 全局 AGENTS.md 读取位置

读取逻辑位于 `crates/jcode-base/src/prompt.rs` 的 `load_agents_md_files_from_dir`，通过 `user_home_path("AGENTS.md")` 解析路径。路径规则如下（二选一，互斥）：

```
设置了 JCODE_HOME  →  $JCODE_HOME/external/AGENTS.md
未设置 JCODE_HOME  →  <系统主目录>/AGENTS.md
```

其中 `JCODE_HOME` 是 jcode 的**根数据目录**（见 `crates/jcode-storage/src/lib.rs` 的 `jcode_dir()`）：

```
设置了 JCODE_HOME  →  $JCODE_HOME
未设置 JCODE_HOME  →  <主目录>/.jcode
```

**关键点**：两条路径是互斥的，不是叠加。如果你设置了 `JCODE_HOME`，全局宪法必须放在 `$JCODE_HOME/external/AGENTS.md`，否则不生效。

### 各平台默认路径（未设置 JCODE_HOME）

| 平台 | 全局 AGENTS.md |
|---|---|
| Windows | `C:\Users\<用户名>\AGENTS.md` |
| Linux / macOS | `~/AGENTS.md` |

## 3. 推荐配置方案

### 方案 1：使用 JCODE_HOME（推荐，根目录集中管理）

把 `JCODE_HOME` 指向 jcode 根目录，宪法放其 `external` 子目录。

```powershell
# 1. 设置用户级环境变量（永久，Windows）
[Environment]::SetEnvironmentVariable(
    'JCODE_HOME', "C:\Users\<用户名>\.jcode", 'User')

# 2. 新建 external 目录
New-Item -ItemType Directory -Force `
    -Path "$env:USERPROFILE\.jcode\external"

# 3. 放入宪法
Copy-Item -Force .\AGENTS.md `
    "$env:USERPROFILE\.jcode\external\AGENTS.md"
```

**注意**：若 `C:\Users\<用户名>\.jcode` 已有 jcode 数据（skills、配置等），`JCODE_HOME` 必须指向该目录本身，否则现有数据不会被读取。

### 方案 2：不设置 JCODE_HOME（最简单）

直接把宪法放到系统主目录：

```powershell
# Windows
Copy-Item -Force .\AGENTS.md "$env:USERPROFILE\AGENTS.md"
```

## 4. 生效时机与验证

- **AGENTS.md 在会话启动时读取**。修改后需**新开会话 / 重启 jcode**，当前运行中的会话不会重新加载。
- 验证：新会话中执行 `/info`，系统提示词统计中应显示
  `global ~/AGENTS.md: <字符数>` 已加载。

## 5. 相关配置层（进阶）

除 AGENTS.md 外，还有一套更底层的 **Prompt Overlay**，直接叠加进系统提示词：

- 项目级：`<工作目录>/.jcode/prompt-overlay.md`
- 全局级：`$JCODE_HOME/prompt-overlay.md`（未设 JCODE_HOME 时为 `~/.jcode/prompt-overlay.md`）

| 层 | 文件 | 作用域 |
|---|---|---|
| 项目规则 | `<工作目录>/AGENTS.md` | 仅当前项目 |
| **全局宪法** | `$JCODE_HOME/external/AGENTS.md` | 所有项目、所有会话 |
| 全局 overlay | `$JCODE_HOME/prompt-overlay.md` | 所有项目，直接叠加 |

## 6. 注意事项

- **路径互斥**：引入 `JCODE_HOME` 后，原 `<主目录>/AGENTS.md` 不再生效，需要迁移文件。
- **数据目录一致**：`JCODE_HOME` 同时决定 skills、日志、状态等位置，改动前确认旧数据无需迁移或已就位。
- **不要放置失效规则**：宪法引用的 skill 需先确认已安装，否则该规则只是空转。
