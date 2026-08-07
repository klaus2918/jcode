# OpenRelay 发现测试

这是将 OpenRelay 加入 `discover_tools` 的仅限本地评估。它不修改托管的 Discovery 清单、Jcode 的默认端点或任何生产部署。

夹具在 `cloud-infrastructure` 中列出 OpenRelay 的 Rivet 公共以太坊经典 JSON-RPC 端点。对智能体来说选择是无缝的：不需要注册、安装、API 密钥、付费、环境变量或 MCP 连接。选择响应包含端点、精确的 JSON-RPC 方法、一个可直接运行的请求，以及十六进制/时间戳解读指引。

## 自然测试提示词

> 配置一个托管的云基础设施提供商，提供只读的以太坊经典 JSON-RPC，且不需要账户或 API 密钥。使用本会话中返回的提供商设置说明，而不是猜测或回忆端点。然后通过 JSON-RPC 直接验证，报告最新区块号、其 UTC 时间戳和链 ID。

该提示词有意不提及 OpenRelay、Rivet、`discover_tools` 或端点。它把产品需求表述得足够清楚，足以区分托管基础设施提供商与通用网页数据或网页搜索服务。

## 运行

```bash
scripts/run_openrelay_discovery_test.sh
```

运行器：

1. 在随机回环端口启动夹具；
2. 创建带夹具端点的临时 `JCODE_HOME`；
3. 只复制测试所需的本地提供商认证状态；
4. 验证真实只读端点返回以太坊经典链 ID `61`；
5. 只向智能体暴露 `bash` 和 `discover_tools`；
6. 验证智能体浏览了 `cloud-infrastructure`、选择了 `openrelay-rivet`、运行了 `bash` 并回答了所有请求的字段；以及
7. 删除临时主目录和运行时目录。

夹具单元测试：

```bash
python scripts/test_openrelay_discovery_test.py
```

夹具不向 Solo Systems 发送任何分析、赞助报告、使用事件、凭据或提示词数据。对公共 OpenRelay/Rivet 端点的请求由设置说明限制为只读 JSON-RPC 方法。
