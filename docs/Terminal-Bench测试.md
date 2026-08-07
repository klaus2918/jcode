# jcode 上的 Terminal-Bench 2.0

本文档描述目前通过 Harbor 在 Terminal-Bench 2.0 上运行 jcode 的最干净可用路径。

## 仓库里有什么

- `scripts/jcode_harbor_agent.py`
  - jcode 的 Harbor 自定义智能体适配器
- `scripts/run_terminal_bench_harbor.sh`
  - 把 Harbor 与适配器和 Linux 兼容 jcode 二进制接线的辅助脚本
- `scripts/run_terminal_bench_campaign.py`
  - 顺序战役运行器，以小批次保留在可拼接布局中
- `scripts/build_linux_compat.sh`
  - 针对较旧 glibc 基线为 TB 风格容器构建 Linux jcode 构件

## 为什么兼容二进制重要

许多 Terminal-Bench 任务容器使用比本地构建的主机二进制更旧的 glibc。Harbor 适配器应使用以下命令生产的 Linux 二进制：

```bash
scripts/build_linux_compat.sh /tmp/jcode-compat-dist
```

如果缺失，辅助脚本会自动为你构建它。

## 认证和模型假设

当前适配器设计用于：

- `~/.jcode/openai-auth.json` 中的 OpenAI OAuth 认证文件
- `gpt-5.4`
- 高推理努力
- 优先服务层级

这些默认值可以用环境变量覆盖。

## 顺序战役模式

如果你想一次只运行几个任务但保持连贯的构件集，使用战役运行器。

示例：

```bash
python scripts/run_terminal_bench_campaign.py \
  --campaign-dir ~/tb2-jcode-campaign \
  --task regex-log \
  --task largest-eigenval \
  --task cancel-async-tasks
```

它做什么：

- 以 `--n-concurrent 1` 顺序运行任务
- 在 `campaign-dir/harbor-jobs/` 下保留 Harbor 作业
- 写入固定的 `campaign.json`
- 关键设置漂移时拒绝混合运行
- 将每个任务的结果追加到 `results.jsonl`

这是当你想要逐步批量任务并在之后拼接时推荐的路径。

## 快速开始

假设 Terminal-Bench 已经位于 `/tmp/terminal-bench-2`：

```bash
scripts/run_terminal_bench_harbor.sh \
  --include-task-name regex-log \
  --n-tasks 1 \
  --n-concurrent 1 \
  --jobs-dir /tmp/jcode-tb2 \
  --job-name regex-log-pilot \
  --yes
```

或让 Harbor 直接指向远程数据集：

```bash
scripts/run_terminal_bench_harbor.sh \
  --dataset terminal-bench@2.0 \
  --include-task-name regex-log \
  --n-tasks 1 \
  --n-concurrent 1 \
  --jobs-dir /tmp/jcode-tb2 \
  --job-name regex-log-pilot \
  --yes
```

## 有用的环境变量

- `JCODE_HARBOR_BINARY`
  - 要上传到任务容器的 Linux 兼容 jcode 二进制路径
- `JCODE_HARBOR_BINARY_DIR`
  - 自动构建兼容二进制时使用的输出目录
- `JCODE_HARBOR_OPENAI_AUTH`
  - OpenAI OAuth 文件路径
- `JCODE_HARBOR_CA_BUNDLE`
  - 可选的主机 CA 捆绑包路径，上传到任务容器
- `JCODE_TB_MODEL`
  - Harbor 模型字符串，默认 `openai/gpt-5.4`
- `JCODE_TB_PATH`
  - 默认本地 Terminal-Bench 路径，默认 `/tmp/terminal-bench-2`
- `JCODE_OPENAI_REASONING_EFFORT`
  - 默认 `high`
- `JCODE_OPENAI_SERVICE_TIER`
  - 默认 `priority`

## 关于公平性和状态隔离的说明

适配器给每个试验一个容器内的全新 jcode 主目录，位于 `/tmp/jcode-home` 下，因此记忆和认证状态按试验容器隔离。

## 当前验证状态

该路径已用真实的 Harbor 任务运行验证，使用：

- `regex-log`
- `largest-eigenval`
- `cancel-async-tasks`

初始试点中三者都在容器内通过，验证器奖励 `1.0`。
