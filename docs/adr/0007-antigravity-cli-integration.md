# ADR 0007：通过 statusLine 与极简钩子接入 Antigravity CLI

状态：已接受；关联 #42。

Antigravity CLI（`agy`）没有向外部桌面程序开放类似 Codex App Server 的固定全局只读 socket（内置 `ConversationService` 仅向会话内子进程注入临时端口与 CSRF 令牌），且没有 `SessionEnd` 钩子。但交互式 `agy` TUI 在 `~/.gemini/antigravity-cli/settings.json` 原生提供 `statusLine`（支持 `stack_with_default: true`）与 `title` 扩展槽，且把进程状态、`tool_confirmation_pending`、账号 `quota` 与 `conversation_id` 经 stdin 管道给直接子进程——子进程自然持有 `agy` PID 与准确终端 `tty`，无需包装命令或猜标题。

实测 `agy` 1.5.18 发现两处关键边界：其一，`ask_question` 多选弹窗不会置位 `tool_confirmation_pending`；其二，`statusLine` 在回合结束时仅报告 `idle`，无法权威区分用户取消（`USER_CANCELED`）与外部中止（`ERROR` / 超限）。同时 `statusLine` 的 `transcript_path` 在 1.5.18 误指向 `~/.gemini/antigravity/brain/...`，而真实对话记录位于 `~/.gemini/antigravity-cli/brain/<conversation_id>/.system_generated/logs/transcript_full.jsonl`。

因此采用「`statusLine` 为主 + 极简 `hooks.json` 补齐」方案：

- 在 `~/.gemini/antigravity-cli/settings.json` 配置零输出 `statusLine`（`stack_with_default: true`，保留内置底栏；若用户已有自定义 `statusLine` 则自动回退到零输出 `title` 槽位），负责入场、实时状态、权限/产物审批待确认、账号用量采集、会话名与阅读路径登记。
- 在 `~/.gemini/config/hooks.json` 仅登记 `red-green` 的 `Stop` 与 `ask_question`（`PreToolUse` / `PostToolUse`）钩子，脚本入口严格按 `/.gemini/antigravity-cli/` 路径过滤，Antigravity App / IDE 与无终端的 `-p` 非交互模式零副作用；所有非 `ask_question` 工具零钩子开销。
- 桌面宠物以 `agy` 进程身份（`pid-<pid>`）为稳定标识，启动即入场、`/clear` 或 `/resume` 切会话不闪烁；退出由 Rust 侧 1.5 秒心跳只读复核 `ps`（校验 PID、命令名 `agy` 与 `tty` 一致）感知离场。电纸书阅读（Supernote Nomad 与 Kindle）以 `antigravity-<conversation_id>` 为键，增量读取 `transcript_full.jsonl`，进程退出或切会话后返回 `410`；开启设备回复后，核对原 `agy` 进程存活、会话未切换且非运行中/待确认，通过受支持终端（tmux / Ghostty 1.3+ / Apple Terminal）的括号粘贴送入回复，不开放历史会话与继续对话。
- 账号用量按经哈希脱敏的账号标识与模型组（`Gemini`、`Claude / GPT`）写入独立 `antigravity-usage.json`，剩余 100% 且 `reset_time` 为 Unix 0 的未使用窗口保留真实 `0%`（无重置时刻），缺失、非法、非初始化态无 `quota` 或已过期窗口一律显示未知（`?`）。

代价：需写入全局 `~/.gemini/config/hooks.json` 的独立 `red-green` 条目；`statusLine` 受 CLI 300ms 节流，极短瞬时状态可能合并；`agy` 退出依赖 1.5s 心跳清理；电纸书回复依赖受支持的终端环境且只针对当前存活会话，不提供历史浏览与继续对话。

协议与状态映射见 [protocol.md](../protocol.md)，调研与验收记录见 [设计](../superpowers/specs/2026-11-28-antigravity-cli-design.md)。
