# TUI 实现笔记

本文收录 TUI 内部的实现细节与正在进行/提议的重构：`TuiState` 特质拆分、
测试不稳定的根因、渲染核心一致性验收标准、Mermaid 渲染重构 ADR。
合并自原四篇文档。

## 一、`TuiState` 特质拆分

**现状**（本 revision 实测）：`pub trait TuiState`（`crates/jcode-tui/src/tui/mod.rs`）暴露
**117 个方法**；2 个实现者（`App`、测试用 `TestState`）；18 个文件、88 处使用 `dyn TuiState`，
其中 65 处是渲染函数签名里的 `app: &dyn TuiState`。它是 `App` 上帝对象在展示层的对应物。

### 为什么朴素拆分收益有限

1. `App` 无论如何都要实现整个表面——拆分不会减少它必须实现的东西，
   也不改变 crate 级编译耦合（该特质只是展示层数据访问）。收益是意图/可导航性，不是解耦。
2. `&dyn TuiState` **不可组合**：Rust 没有稳定的 `&dyn (A + B)`，需要多个领域方法的消费者
   必须接受超特质。实测 18 个使用 `dyn TuiState` 的文件里只有 **2** 个是多领域
   （`ui.rs`、`ui_viewport.rs`），其余各用单一领域——因此叶子模块的声明面确实能收窄，
   但由这 2 个中央渲染器驱动的头号上帝接口仍宽。

结论：值得做，但它**不是**编译耦合收益，应增量进行以避免在 29 个文件上高冲突。

### 目标形态

```
trait TuiState:
    TuiTranscriptState + TuiInputState + TuiScrollState + TuiStreamStatusState
    + TuiProviderState + TuiSessionServerState + TuiWorkspaceState
    + TuiDiffPaneState + TuiSidePanelState
    + TuiInlineState + TuiOverlayState + TuiCopySelectionState
    + TuiOnboardingState + TuiMiscState
{}
```

`App` 与 `TestState` 各子特质保留一个 `impl`（机械移动）；2 个中央渲染器留在超特质上；
叶子渲染模块收窄到所需子特质。

方法归类（按职责域）：会话记录/输入/滚动/流状态/Provider/会话-服务器/工作区/
差异窗格/侧栏/内联交互/覆盖层/复制选择/入职/杂项。

（原方案里还有一个 `TuiDiagramPaneState`，随图窗格 `ui_diagram_pane.rs` 一起在
feature-simplification 中被删除，不再需要。）

### 增量迁移顺序

1. 在特质定义中落地文档化节标题（已完成，纯注释，单文件）；
2. 提取一个单文件消费者的叶子子特质作为模式证明（如 `TuiCopySelectionState`），
   用 `cargo check -p jcode-tui` 验证；
3. 每次提交提取一个剩余叶子子特质，同一提交收窄对应叶子渲染模块；
4. 全程保持 `ui.rs` / `ui_viewport.rs` 在超特质上。

每步只移动数据访问器、保持行为且独立可编译，因此能与其他智能体的改动并行合并。

验证注意：`cargo check -p jcode-tui` 时 `TMPDIR` 必须指向真实磁盘（RAM-backed tmpfs 会让
ring/aws-lc-sys 构建脚本报 "Disk quota exceeded"）；结束时跑一次 `cargo test -p jcode-tui --lib`，
但要用 `--test-threads=1` 单独复核失败用例（见下节）。

## 二、`jcode-tui` 测试不稳定的根因

**现象**：`cargo test -p jcode-tui --lib` 每次运行失败 1–4 个测试，且失败集合每次不同；
`--test-threads=1` 时全绿；单独运行任一失败测试都能过。
这是**进程全局状态上的并行竞争**，不是逻辑错误。

**根因**：`create_test_app()`（以及兄弟构造函数 `create_named_provider_test_app`；定义分别在
`crates/jcode-tui/src/tui/app/tests/support_failover/part_01.rs`、`ui_header.rs`、
`remote_tests.rs`）会调用 `crate::tui::ui::clear_test_render_state_for_tests()`，清空
**进程全局**渲染状态（闪烁帧历史、布局快照、状态区快照、复制目标、滚动位置）。
渲染测试用 `render_state_test_lock()` 保护该状态，但 `create_test_app` **不加锁**就清空它，
因此它约 620 个调用点中任何一个都能在断言中途重置并发运行的渲染测试状态。

最常见的受害者是 `test_changelog_overlay_repeated_renders_are_stable`：记录到的闪烁事件会给
后续渲染追加一行"⚠ 检测到闪烁"通知，使每个布局敏感断言偏移一行。

**已试过且无效**：

- 在 `create_test_app` 内取 `render_state_test_lock`：正确但把 810 个调用点串行化，
  套件时间从约 12 秒涨到 10 分钟以上（已测量并回退）；
- 断言下限而非精确计数、或在测试顶部清空状态：5/5 仍失败（已回退）。

**建议方向**（真正的修复是停止跨测试共享状态，而不是给它加锁）：

1. **首选**：把渲染状态做成**线程局部**而非进程全局，使并行测试无法观察彼此的复位。
   生产只有一个渲染线程，运行时行为不变。
2. 退而求其次：让 `create_test_app` 完全跳过渲染状态清空（只有渲染测试依赖它，
   而它们已在锁下清空），但需审计哪些应用测试隐式依赖该清空。

## 三、渲染核心一致性验收标准

`jcode-render-core` 与 TUI 的切换采用差分测试
（`crates/jcode-tui-markdown/src/render_core_adapter_tests.rs`），比较
`render_markdown_via_core*` 与旧版 `render_markdown*`。

### 一致性级别

| 级别 | 定义 | 比较器 |
|---|---|---|
| L1 内容一致 | 空白折叠后的可见文本相同 | `flattened()` |
| L2 行结构一致 | 每行修剪后的非空可见文本相同（捕获换行分歧） | `nonblank_texts()` |
| L3 包裹布局一致 | 在生产宽度 {20, 40, 80} 下做 L2 比较 | 对包裹输出的 `nonblank_texts()` |
| L4 样式不变量 | 定向样式等价：数学前景跨度相同、粗体带 BOLD、行内代码带背景填充、标题按级别着色、显示数学加框 | 跨度级谓词 |

刻意排除（属记录在案的分歧，不是失败）：空行填充计数、装饰标记字形选择、
非不变量跨度上的精确 `Style` 相等。新的有意分歧必须登记在此。

### 阈值与统计

所有差分测试**零容忍**（`mismatches == 0`）；下面的统计只量化"一次通过证明了什么残余风险"。

| 层级 | 迭代 | 门 | 残余失配率 95% 上界 |
|---|---|---|---|
| CI 默认 | 5000（L1/L2）、3000×3 宽度（L3） | 触碰 `jcode-tui-markdown`、`jcode-render-core` 的 PR 必须通过 | 每文档 p < 6.0e-4（L1/L2）；每文档每宽度 p < 1.0e-3（L3） |
| 切换前深度运行 | `JCODE_MD_FUZZ_ITERS=100000` | 在提议切换的确切提交上通过一次 | 每文档 p < 3.0e-5 |
| 夜间浸泡（可选） | 25000 次、轮换 `JCODE_MD_FUZZ_SEED` | 失败用复现种子记 issue | 跨种子累积覆盖 |

上界依据"三规则"：N 个独立生成文档、0 失配时，失配概率的单侧 95% 上界为 `1 - 0.05^(1/N) ≈ 3/N`。

固定语料标准（非统计）：43 个语料用例全过 L1；每个 `parity_*` 用例过 L1；
每个 L4 不变量测试通过。向生成器加构造**必须**至少补一个固定语料用例。

切换门（全部必需）：切换提交 CI 绿；10 万次深度运行对 L1/L2/L3 绿；
用第二个种子重复深度运行绿；生成器覆盖清单无未勾选构造。

### 失败报告契约

- **可复现**：每次失败报告迭代索引 `i`，每轮 RNG 种子为 `seed = base_seed + i * 0x100000001B3`，
  因此可用 `JCODE_MD_FUZZ_SEED=<base_seed> JCODE_MD_FUZZ_ITERS=<i+1>` 复现；
  `base_seed` 默认是每套件固定常量，可用 `JCODE_MD_FUZZ_SEED` 覆盖。
- **有界转储**：中止前收集最多 5 个失败用例并一次性报告（输入、核心输出、旧版输出）——
  绝不在第一个用例上失败，分类分歧需要多个样本。
- **全输入回显**：原始 markdown 逐字打印，便于把失败用例直接提升进固定语料。

报告结果时必须给出：`iters`/`base_seed`/套件名与 L1–L4 通过情况、每个通过套件的 95% 上界、
开发中发现的失败分类（解析器分歧 / 适配器样式 / 包裹分歧 / 生成器产物）与新增语料用例。

### 生成器覆盖清单

已覆盖：标题 1–3 级、段落、硬/软换行、粗斜删除线、行内代码、链接、行内与显示数学、
货币美元消歧、有序/无序/嵌套/任务/定义列表、块引用（嵌套/多行）、主题分隔、
围栏代码块（带/不带语言）、表格 1–3 列、脚注、CJK/emoji。

未覆盖：setext 标题（仅固定语料）、图像/自动链接/HTML 片段（仅固定语料）、
引用风格链接与缩进代码块（未覆盖）。未勾选项必须补齐或在切换 PR 中显式豁免并附语料用例。

### 运行

```sh
cargo test -p jcode-tui-markdown --lib render_core_adapter            # CI 等价
JCODE_MD_FUZZ_ITERS=100000 cargo test -p jcode-tui-markdown --lib render_core_adapter::tests::fuzz
JCODE_MD_FUZZ_ITERS=100000 JCODE_MD_FUZZ_SEED=20260713 \
  cargo test -p jcode-tui-markdown --lib render_core_adapter::tests::fuzz   # 替代种子确认
```

## 四、Mermaid 渲染重构（ADR）

日期 2026-05-08；状态：**提议**（部分已落地）。

### 问题

渲染、缓存、UI 放置、活动图注册、延迟工作、调试统计与终端图像协议状态通过全局状态和副作用
耦合在一起：`jcode-tui-mermaid/src/lib.rs` 仍是状态枢纽；Markdown 渲染直接决定 Mermaid 行为
（含流式/延迟/仅侧栏的注册规则）；活动图作为**渲染调用的副作用**注册，仅准备 markdown 就会
改变固定窗格状态；`with_preferred_aspect_ratio` 用线程局部状态，缓存键与渲染尺寸依赖环境上下文；
同一张图会在聊天行内占位、侧栏图像、固定窗格、流式预览、调试探针等多种上下文中渲染；
延迟渲染自带去重/纪元/全局队列并顺带做活动注册；图像协议渲染、PNG 生成、图像状态缓存与
视口渲染混在同一公共面。

### 尺寸 API 方向

渲染器已有 `mmdr-size-api` 路径（由特性 + `JCODE_MMDR_SIZE_API_AVAILABLE=1` 保护），
应成为主路径：由渲染器询问 Mermaid/布局测量的 SVG/Canvas 尺寸，而不是用源文本复杂度估计
最终 PNG 尺寸；`calculate_render_size` 降级为"目标提示"而非尺寸事实来源；
回退 SVG 重定向路径仅作兼容保留；调试统计报告 `render_size_backend` 并在期望尺寸 API
却不可用时显式失败；缓存键包含归一化的目标/配置输入，构件存储测量得到的输出尺寸。
这能减少纵横比重定向、模糊放大、占位符高度不匹配与窗格调整大小振荡带来的 bug。

### 目标设计（显式分阶段流水线，阶段间只传纯数据）

```
Markdown/事件源 → 图提取 → DiagramRegistry 更新 → RenderScheduler
                → RenderCache → 渲染器（AST/布局/SVG/PNG）
                → 放置规划器 → 终端图像呈现器
                ↘ 固定/侧栏选择器
```

1. **图提取**：Markdown 只把围栏 Mermaid 块提取为不可变描述符
   （`DiagramId` / `source_hash` / `source` / `origin` / `ordinal`），不直接改活动图、
   不同步渲染（除非调用方显式要求阻塞回退）。
2. **显式渲染请求**：用 `RenderRequest`（`diagram_id`、`source_hash`、`source`、`target`、
   `profile`、`priority`、`mode`）替换环境式配置与布尔参数；
   `RenderMode` = `CacheOnly` / `EnqueueIfMissing` / `Blocking`；
   缓存键只由 `source_hash + 归一化 RenderProfile` 构成，**绝不来自线程局部上下文**。
3. **注册表拥有活动状态**：引入由 TUI 应用/会话状态拥有的 `DiagramRegistry`
   （而不是全局 Mermaid crate 向量），跟踪可见图、以代 id 跟踪流式预览、发布固定窗格有序列表、
   每次准备原子清除/更新；渲染返回 `RenderArtifact`，**绝不把注册活动图作为副作用**。
4. **调度器拥有异步/延迟**：返回 `Ready(RenderArtifact)` / `Pending{request_id}` /
   `Failed(error)` / `ProtocolUnavailable`；按完整缓存键去重；工作者不改活动注册表；
   完成只发布 `MermaidRenderCompleted` + 构件元数据；纪元失效限定在请求代。
5. **放置与渲染分离**：markdown/侧栏准备根据 `RenderStatus` 插入占位符
   （行内图像占位、仅侧栏标记、失败错误块、待处理占位）；图像小部件只消费
   `RenderArtifact` + `PlacementPlan`，不认识 Mermaid 源与调度。
6. **模块边界**：`model.rs` / `extract.rs` / `cache.rs` / `renderer.rs` / `scheduler.rs` /
   `registry.rs` / `placement.rs` / `presenter.rs` / `debug.rs`。

### 迁移顺序与验证

顺序：① 加显式模型类型与缓存键归一化测试 → ② 加新调度器 API 并保留旧包装器 →
③ 把 `render_mermaid_sized_internal` 变成纯 `renderer::render_to_png(request) -> RenderArtifact`
→ ④ 把活动图写入移出渲染函数，改由 markdown 准备/应用注册表更新 →
⑤ 用显式 `RenderProfile` 替换 `with_preferred_aspect_ratio` 调用点 →
⑥ 拆开演示器/图像状态与 PNG 渲染 → ⑦ 删除旧布尔包装 API 与线程局部渲染配置。

验证：缓存键归一化与文件名解析单测；注册表更新顺序/流式预览替换/原子清除单测；
调度器去重、缓存命中/未命中、完成事件、无活动状态改变；markdown 渲染产生确定性占位符且无全局副作用；
现有滚动与固定窗格测试仍过；调试探针能用显式配置渲染并报告确切缓存键。

**近期高 ROI 动作**：先引入显式请求/状态类型，把旧公共函数变成薄兼容包装器，
从而一次迁移一个调用点，同时减少布尔/线程局部行为带来的新 bug。

## 五、相关缓存上限

Mermaid 侧的内存与磁盘上限（渲染缓存 512、图像状态 24、解码源 16、活动图 128、
磁盘缓存 50 MiB / 3 天）见 [记忆系统](记忆系统.md) 的"回归预算与护栏"一节。
