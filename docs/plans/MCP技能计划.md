# 计划：动态技能与 MCP 支持

## 目标
1. 无需重启热重载技能
2. MCP（Model Context Protocol）服务器支持
3. 运行时动态工具注册
4. 智能体可以自行添加/配置 MCP 服务器

## 当前状态
- 技能：启动时从 `~/.claude/skills/` 和 `./.claude/skills/` 加载
- 工具：硬编码在 `Registry::new()` 中
- 无 MCP 支持

---

## 实现计划

### 阶段 1：热重载技能

**对 `src/skill.rs` 的修改：**
- 给 `SkillRegistry` 添加 `reload(&mut self)` 方法
- 技能可以无需重启而重载

**新工具 `reload_skills`：**
- 智能体可以触发 `reload_skills` 以拾取新技能

### 阶段 2：动态工具注册表

**对 `src/tool/mod.rs` 的修改：**
```rust
impl Registry {
    /// 在运行时注册新工具
    pub async fn register(&self, tool: Arc<dyn Tool>);

    /// 按名称注销工具
    pub async fn unregister(&self, name: &str);

    /// 列出所有已注册工具
    pub async fn list(&self) -> Vec<String>;
}
```

### 阶段 3：MCP 客户端

**新模块 `src/mcp/mod.rs`：**
- MCP 协议类型（JSON-RPC 2.0）
- 面向 stdio 服务器的 MCP 客户端
- MCP 工具包装器（把 MCP 工具转换为我们的 Tool trait）

**配置文件 `~/.claude/mcp.json`：**
```json
{
  "servers": {
    "filesystem": {
      "command": "npx",
      "args": ["-y", "@anthropic/mcp-server-filesystem", "/path"],
      "env": {}
    },
    "github": {
      "command": "npx",
      "args": ["-y", "@anthropic/mcp-server-github"],
      "env": {"GITHUB_TOKEN": "..."}
    }
  }
}
```

**MCP 管理器：**
- 启动时加载配置
- 连接已配置服务器
- 把 MCP 工具转换为 jcode Tool trait
- 处理服务器生命周期（启动、停止、重启）

### 阶段 4：智能体自配置

**新工具：**
- `mcp_list` - 列出已连接的 MCP 服务器
- `mcp_connect` - 启动新 MCP 服务器
- `mcp_disconnect` - 停止 MCP 服务器
- `mcp_reload` - 重载所有 MCP 服务器

**流程：**
1. 智能体调用 `mcp_connect {"name": "playwright", "command": "npx", "args": ["-y", "@anthropic/mcp-server-playwright"]}`
2. jcode 派生进程，做 MCP 握手
3. 服务器工具被添加到注册表
4. 智能体可以立即使用新工具

---

## MCP 协议摘要

MCP 通过 stdio 使用 JSON-RPC 2.0：

**初始化：**
```json
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"jcode","version":"0.1.0"}}}
```

**列出工具：**
```json
{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}
```

**调用工具：**
```json
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"read_file","arguments":{"path":"/tmp/test.txt"}}}
```

---

## 要创建/修改的文件

1. `src/mcp/mod.rs` - MCP 模块
2. `src/mcp/protocol.rs` - JSON-RPC 类型
3. `src/mcp/client.rs` - MCP 客户端
4. `src/mcp/manager.rs` - 多服务器管理器
5. `src/mcp/tool.rs` - MCP 工具包装器
6. `src/tool/mod.rs` - 添加动态注册
7. `src/tool/mcp_tools.rs` - mcp_connect、mcp_list 等
8. `src/skill.rs` - 添加 reload()
9. `src/tool/reload_skills.rs` - reload_skills 工具

## 实现顺序
1. 动态工具注册表（前置条件）
2. 技能热重载（快速胜利）
3. MCP 协议类型
4. MCP 客户端（单服务器）
5. MCP 管理器（多服务器）
6. MCP 工具用于智能体自配置
