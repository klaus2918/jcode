# TUI 与终端

本文覆盖 TUI 的颜色系统与终端能力适配。合并自原「TUI 颜色配置」与「终端能力」两篇。

## 一、颜色与调色板

jcode TUI 渲染的每种颜色都可配置，且配色质量可被客观度量（而不是靠肉眼）。
**内置默认调色板是手工调校并冻结的**：`crates/jcode-tui-style/src/palette.rs` 的
`every_role_keeps_its_hand_tuned_default` 与 `default_palette_matches_historical_values`
持有每个角色的期望值副本，改默认值会让测试失败。
默认值和谐分偏低**不是**改它的理由。

### 配置

```toml
[display.colors]
user = "#8ab4f8"
ai = "#81c784"
accent = "#ba8bff"
error = "#ff6464"
```

| 命令 | 效果 |
|---|---|
| `/colors`（或 `/colors list`） | 列出每个可配置角色及其当前值 |
| `/colors <role> <#rrggbb>` | 设置一个角色（写入配置，立即生效） |
| `/colors reset [role]` | 重置某个角色或全部 |

`/colors generate`、`/colors harmony`、`/colors export` 三个子命令已随
feature-simplification（S-3，2026-08-16）移除，TUI 不再暴露入口；打分与生成能力仍在
`jcode-tui-style` 库里供程序调用（`jcode_tui_style::analyze_harmony`、
`jcode_tui_style::harmony::generate_from_seed`），目前的使用者是该 crate 的测试与示例
`crates/jcode-tui-style/examples/light_bench.rs`。

### 替换点：渲染后的帧缓冲

TUI 有 22 个命名语义角色（`ALL_ROLES`），加上散布各处的 222 个不同 `rgb(...)` 字面量
（`palette_literals.rs` 实测值）。
逐个调用点改会永久脆弱，因此替换发生在颜色到达终端的唯一必经点——帧缓冲：

```
部件（rgb 字面量 / 角色访问器 / 命名颜色）
   → 渲染后的帧缓冲
   → adapt_buffer_for_theme   （亮/暗适配）
   → adapt_buffer_for_palette （用户颜色配置）
   → 终端
```

顺序有意义：亮/暗 pass 会翻转亮度，让为暗色终端设计的内置调色板在亮色终端也可用；
**用户配置的颜色最后运行且不翻转**，配置什么终端就收到什么。

三个后果：

- 角色访问器返回**默认**颜色（不是配置色），否则同一单元格会被重映射两次、色相/亮度偏移叠加。
- 处于某角色默认值感知半径内的临时字面量会**跟随该角色**重新表达，保留自身的亮度/彩度偏移
  （因此"警告色的略暗变体"重新着色后仍是略暗变体）；远离任何角色的字面量不变。
- 未配置调色板是字节相同的 no-op（有测试保护）。

### 覆盖完整性由测试保证

- `palette_literals.rs` 记录 TUI crates 渲染的每个不同 `rgb(...)` 字面量（222 个），
  测试 `most_tui_literals_are_reachable_from_some_role` 要求**全部**能从某个角色可达
  （无人认领 = 用户改不了的颜色），阈值就是 100% 而不是某个宽松比例。
- `no_single_role_dominates_the_literal_space` 要求 22 个角色每个至少认领一个真实字面量
  （不留死重角色），且没有角色认领超过一半（保证族半径仍能区分角色）。
- ratatui 命名颜色单独覆盖（它们不带可匹配的 RGB）：`every_named_color_used_by_the_tui_is_configurable`
  枚举实际用到的每个命名颜色并要求映射到角色；`reset_is_never_substituted` 固定 `Color::Reset`
  刻意从不替换（它是终端自身背景透出的方式）。
- 引入新色调部件时要重新生成 `palette_literals.rs`。

### 和谐度量

和谐度由 `jcode-tui-style` 的 `harmony::analyze(palette, background)` 在 Oklab 感知均匀
空间里算出 0–100 分（**TUI 已无 `/colors harmony` 入口**）。共 7 条准则：

| 标准 | 权重 | 关键 | 度量 |
|---|---:|:--:|---|
| 可读性 | 3.0 | 是 | APCA 式亮度对比，逐前景角色对真实终端背景（背景亮暗不同则结论不同） |
| 区分度 | 2.0 | 是 | `MUST_DISTINGUISH` 列出的角色对（success/error、user/ai 等）的最小 Oklab 距离 |
| 视觉多样性 | 2.0 | 是 | 屏幕主导区域是否是一团低饱和的“灰糊”（色度带不出信息） |
| 色相和谐 | 1.5 | 否 | 对公认配色方案（单色/类似/互补/三角/四角/分裂互补）的最佳拟合，45° 均值偏差记 0 分 |
| 彩度连贯 | 1.5 | 是 | 饱和度离散度与舒适阅读带（相对该调色板自身饱和度水平判定） |
| 相邻分离 | 1.5 | 否 | 渲染时真正相邻出现的角色对之间的感知距离（补充手列清单的区分度） |
| 色盲安全 | 1.0 | 否 | 在红绿色盲（deuteranopia / protanopia）模拟下重测区分度 |

调色板里带色相的角色不足 2 个时，色相和谐直接记满分 100（退化场景不作惩罚）。

两条关键设计（`harmony.rs` 里可逐行核对）：

- **只有关键标准能拉低总分**：`总分 = 0.75 × 加权平均 + 0.25 × 最差关键标准`。
  可读性、区分度、视觉多样性、彩度连贯是关键；色相和谐、相邻分离、色盲安全只通过加权平均影响
  总分。不可读文本是缺陷；非常规色相是风格选择（Solarized 刻意打破色相教科书规则仍极受欢迎）。
- **标准内聚合是 `0.4 * 均值 + 0.6 * 最差`**：一个坏角色不能躲在二十个好角色后面。

**校准**：用被数千人选择的真实调色板钉住排序，断言写在 `harmony.rs` 的测试里——
`respected_community_palettes_all_score_well` 要求 Solarized Dark、Gruvbox Dark、Dracula、
Nord 在暗色背景下全部 ≥60 分；`hostile_palettes_score_far_below_good_ones` 要求
"全灰"与"霓虹混乱"两个对抗样本至少比最差的好调色板低 10 分；
`an_unreadable_palette_is_ranked_below_a_merely_garish_one` 要求不可读的排在刺眼的之后。
测试钉住的是**排序与间距**，不钉具体分数（准则调整后分数会变，排序不该变）。

### 生成调色板

`generate_from_seed(seed, background)` 从一个种子派生整套调色板（库 API，无 TUI 入口）：

- 角色按**分裂互补**布局放在种子的色相环上；
- 彩度拉入舒适阅读带（霓虹种子也能产生可用结果）；
- 亮度瞄准*当前*终端背景（为暗色调校的调色板在亮色上通常错）；
- `success` / `warning` / `error` 保留传统色相（用户对"红=错误"的依赖远超对新颖的追求）；
- 必须区分的对在**亮度与色相上都分离**：红绿色觉缺陷下色相分离基本坍缩到蓝-黄轴，
  只有亮度在所有类型下都存活，因此绿/琥珀/红被放在三个不同亮度级别；
- **修复 pass** 用调色板全局最弱对给候选移动打分（约束耦合——success/warning/error 构成三角形，
  贪心成对修复会循环），且候选被约束保持对比度、彩度与传统色相。

诚实的限制：在可读亮度带与"红=错误"色相预算内，琥珀警告与红色错误在红色盲下
无法被推到目标区分度以上。测试 `generated_palettes_score_well_from_any_seed` 要求 7 个种子
（jcode 蓝、纯红、纯绿、近黑、近白、纯灰、品红）在亮色与暗色背景上**都**≥74 分；
`generated_palettes_beat_hand_made_classics` 还要求生成结果不低于四个手调经典中的最高分。

### 添加角色

1. 在 `crates/jcode-tui-style/src/palette.rs` 的 `Role` 加变体，列进 `ALL_ROLES`，
   给出 `key()` 与等于现调用点硬编码值的 `default_rgb()`；
2. 若是背景色，在 `is_background()` 中说明（背景用不同可读性标准）；
3. 若必须与其他角色可区分，把该对加入 `harmony.rs` 的 `MUST_DISTINGUISH`
   （不要加入好调色板里本就相似的组合，如 `dim`/`tool`）；
4. 在 `theme.rs` 加访问器并在调用点使用。

`ALL_ROLES` 驱动 `/colors` 列表与补全，同时也是字面量覆盖测试与和谐分析的遍历来源，
因此新角色会自动被这些检查覆盖。

## 二、终端能力与适配

### 能力矩阵（编制于 2026-03-02，反映当时各终端稳定版）

| 终端 | 真彩色 | Kitty 键盘 | 括号粘贴 | 鼠标捕获 | 备注（要点） |
|---|:--:|:--:|:--:|:--:|---|
| macOS Terminal.app | 否（无真彩，RGB 被钳到 256） | 否 | 是 | 是 | emoji 常渲染 1 格宽；`TERM=xterm-256color` |
| iTerm2 | 是 | 是（3.5+） | 是 | 是 | 专有行内图像协议；偶有 TERM_PROGRAM 误报 |
| Ghostty | 是 | 是 | 是 | 是 | GPU 渲染，遗留怪癖极少 |
| Handterm | 是 | 部分 | 是 | 是 | 实验性 Wayland 原生 GPU 终端，平滑像素滚动是终端原生 |
| Kitty | 是 | 是（发起者） | 是 | 是 | 严格合规；`TERM=xterm-kitty`，ssh 需传 terminfo |
| Alacritty | 是 | 是（0.13+） | 是 | 是 | 无标签/分屏；默认鼠标滚动不传给应用；无连字 |
| WezTerm | 是 | 是 | 是 | 是 | 功能最全；Lua 配置可能拖慢启动 |
| Warp | 是 | 部分 | 拦截粘贴 | 有限 | 块架构拦截大量转义序列，TUI 可能渲染异常 |
| Windows Terminal | 是 | 否 | 是 | 是 | ConPTY 可能丢快速转义；调整大小可能溢出 1 格 |
| VS Code 终端 | 是 | 是（xterm.js 5.x+） | 是 | 是 | 比原生终端略慢；Canvas 可能留陈旧单元格 |
| GNOME 终端（VTE） | 是 | 否 | 是 | 是 | 重写 `COLORTERM=truecolor`；无连字 |
| Konsole | 是 | 部分 | 是 | 是 | 调整大小重排可能瞬时损坏；旧版 SGR 背景溢出 |
| tmux | 需显式配置 | 否（剥离 kitty 序列） | 透传 | 透传 | **渲染问题头号来源**：自带模拟层、剥离未知转义、宽度表可能与外层不一致 |
| screen | 否 | 否 | 较新版本 | 基本 | **最受限**：无真彩、Unicode 支持极少、过滤激进 |

图例：真彩色 = `\e[38;2;R;G;Bm`；Kitty 键盘协议 = `CSI > flags u`；
括号粘贴 = `\e[?2004h`；鼠标捕获 = SGR 1006（`\e[?1006h`）。

### 典型渲染问题与根因

1. **背景色溢出 / "白块"**：清除前未先设置背景色时，`\e[K`（清到行尾）继承终端默认背景——
   深色主题白块的头号原因。tmux 需正确的 `tmux-256color` terminfo（BCE 声明）；
   ConPTY 会错误合并快速 SGR+清除序列；xterm.js Canvas 在缓冲切换时可能留幽灵单元格。
2. **Emoji / 双宽字符错位**：应用与终端对字符列宽判断不一致，该行后续单元格全部偏移。
   Terminal.app 最严重（emoji 常按 1 格渲染）；tmux 有自己的宽度实现；screen 宽度表严重过时；
   Alacritty 的 PUA（Nerd Font）字形默认 1 格。
3. **调整大小后陈旧内容**：`SIGWINCH` 后的部分重绘与终端重排冲突。Konsole 会重排软换行；
   tmux 重排后再中继 SIGWINCH（存在竞争）；xterm.js 的 resize 处理可能滞后。
4. **备用屏幕切换伪影**：Warp 不用传统备用屏幕；嵌套 tmux 可能丢失缓冲区追踪；
   旧版 Terminal.app 恢复后光标可能保持不可见。
5. **光标可见性**：`\e[?25l`/`\e[?25h` 不总是配对（应用被杀死时尤其）。
   kitty/WezTerm/iTerm2 会在提示符处自动恢复，GNOME 终端与 Terminal.app 可能不会。
6. **SGR 重置范围**：旧 VTE 不重置下划线颜色；Konsole 曾不重置上划线；screen 的 `\e[0m`
   不能可靠重置 256 色。
7. **Kitty 键盘协议未复位**：应用退出/崩溃时未发 `CSI < u`，会把终端留在增强键盘模式。
   Alacritty 与 WezTerm 不自动复位，需用户 `reset`。
8. **tmux 透传限制**：剥离未知序列（破坏 kitty 键盘/图形、iTerm2 行内图像、部分扩展 SGR，
   如卷曲下划线需 tmux 3.4+）；DCS 透传增加延迟；`TERM` 不匹配会让能力协商静默失败；
   OSC 52 剪贴板需 `set -g set-clipboard on`。

### 对 TUI 开发（含本仓库实现）的要点

1. **清除前总是先设置背景色**：任何 `\e[K`/`\e[J`/`\e[2J` 之前先设 SGR 背景，
   绝不假设终端默认背景与主题一致。
2. **用 `COLORTERM` 检测真彩色**（`truecolor` / `24bit`），而不是解析 terminfo。
3. **防御性处理 emoji 宽度**：用 Unicode 15.1+ 宽度表，并接受部分终端不一致；
   网格对齐布局中可考虑避免 emoji 或用显式空格填充。
4. **`SIGWINCH` 时全量重绘**，不要增量修补。
5. **退出时总是恢复终端状态**（清理处理器覆盖 SIGTERM/SIGINT）：恢复光标、离开备用屏幕、
   关闭鼠标捕获、关闭括号粘贴、重置 kitty 键盘协议、发 SGR 重置。
6. **在 tmux 下显式测试**（许多问题只在复用器下出现）。
7. **对 Terminal.app 与 screen 优雅降级**：检测后回退到 256 色与 ASCII 安全元素。

jcode 相关的具体处理：启动时请求 kitty 键盘协议
（`enable_keyboard_enhancement`，见 [交互机制](交互机制.md) 的 Shift+Enter 一节），
`/terminal-setup` 会实测终端支持并给出可执行修复。
