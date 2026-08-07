# Shift+Enter 与多行输入

## 问题

终端对 Enter 只发送一个字节：`0x0d`。VT100 时代的编码无处记录 Shift 是否被按住，因此 `Enter`、`Shift+Enter` 和 `Ctrl+Enter` 都以同一个字节到达。无论应用怎么写，都无法区分它们。

## jcode 如何处理

现代解决方案是 **kitty 键盘协议**。应用请求终端进行消歧，终端随后对 Shift+Enter 发送 `ESC[13;2u`（键码 13，修饰符 2 = 1 + shift 位）。

jcode 在启动时请求该协议（`enable_keyboard_enhancement`），crossterm 解码结果，因此在**支持该协议的终端上 Shift+Enter 无需任何配置即可工作**：kitty、Ghostty、WezTerm、Alacritty、foot、iTerm2 3.5+、Warp 和 VS Code 1.109+。

仍有三种情况会失效，jcode 对每种情况都做了明确处理：

| 情况 | 修复 | 位置 |
| --- | --- | --- |
| 终端忽略该请求（Terminal.app） | 更换终端，或手动将 Shift+Return 映射为 `\033[13;2u` | `/terminal-setup` 会说明 |
| tmux 不转发扩展键 | 将 `extended-keys` 设置写入 `~/.tmux.conf` | `/terminal-setup` 会应用 |
| WezTerm 需要显式启用标志 | 设置 `enable_kitty_keyboard = true` | `/terminal-setup` 会应用 |

## `/terminal-setup`

当 Shift+Enter 变成提交而不是插入换行时运行它。它会实际查询终端的支持情况而不是凭空假设，然后要么确认该组合键已经可用，要么应用所需配置，要么说明为什么配置无法解决。

查询很重要：写入激活转义序列几乎总会在忽略它的终端上"成功"，因此 `supports_modified_enter_reporting` 直接询问终端（`CSI ? u` 后跟 `CSI c`）。

## 回退方案

以下方案在每个终端上都能用，因为它们不依赖修饰键上报：

- **结尾反斜杠再按 Enter** 插入换行，与 shell 行续接一致。第一次使用时，jcode 会引导你使用 `/terminal-setup`。
- **Option/Alt+Enter** 在终端发送 `ESC` + `CR` 的任何地方都可用，包括启用了"将 Option 用作 Meta 键"的 Terminal.app。

## 为什么不直接让用户使用回退方案？

因为 Shift+Enter 才是人们期待的，而且在大多数终端上它已经可以实现。回退方案是安全网，不是组合键正常工作的替代品。

## 测试

- `tui::app::tests::shift_enter_csi_u_sequence_decodes_to_enter_plus_shift`
  将精确的字节序列送入真实 PTY，断言 crossterm 解码为 Enter+SHIFT。这把写入终端配置的序列与应用实际理解的序列钉在一起。
- `tui::app::tests::bare_carriage_return_decodes_without_shift` 钉住底层问题，使设置存在的原因在代码中有文档记录。
- `tui::terminal_setup::tests::*` 覆盖配置生成、幂等性以及不覆盖用户配置。
