# #42：Antigravity CLI 会话、账号用量与电纸书阅读接入设计

关联需求：[支持 Antigravity CLI 会话](https://github.com/yangzhyo/red-green/issues/42)。2026-11-28 用户确认三项设计取舍：采用 `statusLine` + 极简 `hooks.json`；Nomad 与 Kindle 首期对齐 Claude Code（当前可读会话阅读与跟随更新）；同步更新 Mac 协议层与 ESP-Mosaico 固件源码及测试。

## 接入与配置

安装脚本 [install-antigravity.sh](../../../scripts/install-antigravity.sh) 将 [antigravity.sh](../../../hooks/antigravity.sh) 链接为 `~/.gemini/antigravity-cli/red-green-status.sh`，并调用 [merge-antigravity-settings.mjs](../../../scripts/merge-antigravity-settings.mjs) 幂等合并两份 JSON 配置（修改前生成带时间戳备份、拒绝符号链接或非法 JSON）：

1. `~/.gemini/antigravity-cli/settings.json`：优先配置 `statusLine`（`stack_with_default: true`，脚本零输出以保留内置底栏）；若用户已有自定义 `statusLine`，自动改用零输出 `title` 槽位；若两槽均已占用，则保留原配置并提示在自定义脚本中转发 stdin。
2. `~/.gemini/config/hooks.json`：合并独立的 `"red-green"` 条目，仅订阅 `Stop` 与 `matcher: "ask_question"` 的 `PreToolUse` / `PostToolUse`，对普通工具调用零开销。钩子入口严格校验 `transcriptPath` / `artifactDirectoryPath` 位于 `/.gemini/antigravity-cli/` 下，自动忽略 Antigravity App、Antigravity IDE 及无交互终端的 `agy -p` 运行。

基线版本：**Antigravity CLI (`agy`) 1.5.18**。用户日常继续直接运行 `agy`，不引入包装命令。

## 终端绑定、会话身份与生命周期

实测确认 `statusLine` 与钩子均作为交互式 `agy` 进程的直接子进程运行，向上沿进程树可直接取得 `agy` PID 与准确控制 `tty`，完整复用 Apple Terminal、Ghostty 1.3+ 与 tmux 的点击聚焦、已阅与前台静默。

- **桌面宠物标识**：使用 `source: "antigravity"` 与 `session_id: "pid-<pid>"`，文件写入 `~/.claude/session-status/antigravity-pid-<pid>.json`（尊重 `RED_GREEN_STATUS_DIR`）。`agy` 启动后首次提问前 `conversation_id` 为空，仍可立即出现空闲宠物；进入对话或执行 `/clear`、`/resume` 切换 `conversation_id` 时宠物窗口与槽位保持稳定。项目名首见即按 Git 仓库根目录锚定，不随后续 `cd` 漂移。
- **退出感知**：Antigravity CLI 无 `SessionEnd` 钩子。Rust 侧 `antigravity::filter_alive` 批量调用一次 `ps -o pid=,tty=,comm= -p ...`，校验进程仍存活、命令名为 `agy` / `agy-cli` 且 `tty` 匹配（防止 PID 复用）；主程序 1.5 秒心跳发现存活 Antigravity 集合变化时主动推送 `sessions-changed` 与 `usage-changed`，确保退出或关窗后 1.5 秒内宠物离场、托盘计数同步更新。脚本亦在新建会话时清理残留死进程文件。隔离验证注入的不带 `pid` 假文件直接放行。

## 六种会话状态映射与已知盲区

| 来源事件 / 字段 | 会话状态 | 说明 |
| --- | --- | --- |
| `statusLine`: `authenticating` / `initializing` / 初始 `idle` | `idle` | 启动入场，宠物睡觉 |
| `statusLine`: `working` / `thinking` / `tool_use`（无待确认标志） | `running` | 若当前正处于 `PreToolUse:ask_question`，`tool_use` 不覆盖 `awaiting` |
| `statusLine`: `tool_confirmation_pending: true` 或 `agent_state: "reviewing"` | `awaiting` | 命令/编辑权限审批与产物评审 |
| 钩子 `PreToolUse`（`ask_question`） | `awaiting` | 补齐 `ask_question` 多选弹窗不置 `tool_confirmation_pending` 的缺口；`PostToolUse` 恢复 `running` |
| 钩子 `Stop`（`NO_TOOL_CALL` 等正常终止） | `your_turn` / `completed` | 读取当前回合末条可见助手答复，按与 Claude / Codex 相同的末行问句启发式裁决 |
| 钩子 `Stop`（`USER_CANCELED`） | `idle` | 用户按 Esc 中断不是外部异常中止 |
| 钩子 `Stop`（`ERROR` / `MAX_INVOCATIONS` / `MAX_FORCED_INVOCATIONS` / `MAX_TOKEN_BUDGET_EXCEEDED` / 非空 `error`）或 `statusLine: error` | `aborted` | 外部错误或超限中止 |

为处理 `Stop` 钩子与 `statusLine: idle` 的毫秒级并发到达，写文件采用每 PID 目录锁 + 原子重命名：`Stop` 先到后，`statusLine: idle` 保留终态与 `since`；若 `statusLine: idle` 先到，先按对话记录兜底分类，随后 `Stop` 若结果一致则沿用同一 `since`，不重置已阅、不重复叫声。

**已知盲区与限制**：
- `statusLine` 受 CLI 300ms 节流约束，极短瞬时状态可能合并。
- 若用户手动禁用全局 `hooks.json` 中的 `red-green`，`ask_question` 弹窗期间仅显示 `running`，回合结束回退为读取对话记录判定（无答复时归为 `idle`）。
- 仅支持本机交互式 `agy` 会话；`agy -p` 非交互模式、`agy acp` 编辑器后端、Antigravity App 与 Antigravity IDE 不在支持范围。

## 账号用量

`statusLine` 原生提供 `email`、`plan_tier` 与 `quota` 映射（并在 v1.5.16+ 长回合流式生成期间周期刷新）。脚本按账号写入独立 `antigravity-usage.json`，按模型分组（如 `Gemini`：`gemini-5h` + `gemini-weekly`；`Claude / GPT`：`3p-5h` + `3p-weekly`）输出标准化额度组，同账号多会话不重复计入，切换账号时立即刷新并清空旧账号缓存。

实测 `agy` 1.5.18 对尚未消耗的桶返回 `remaining_fraction: 1` 与 `reset_time: "1970-01-01T00:00:00Z"`（Unix 0），解析为真实 `0%` 且暂无重置时刻；真实 `resets_at > 0` 且已过期、或缺少/非法数据时一律显示 `?` / `未知`，绝不伪装为零用量。用量表保留 Codex 默认优先，三来源并存时互不覆盖、点击轮换所有在场来源的额度组。

## 电纸书可读会话与 ESP-Mosaico

- **Supernote Nomad 与 Kindle Paperwhite**：实测 `agy` 1.5.18 的 `statusLine` 将 `transcript_path` 误报为 `/.gemini/antigravity/brain/.../transcript.jsonl`，采集脚本自动修正为 `~/.gemini/antigravity-cli/brain/<cid>/.system_generated/logs/transcript_full.jsonl` 登记至 `.reader/antigravity-<cid>`（不进入宠物快照）。新增 [reader/antigravity.rs](../../../app/src-tauri/src/reader/antigravity.rs) 增量解析 `USER_INPUT`（剥离 `<USER_REQUEST>` 外壳与 `<ADDITIONAL_METADATA>` 等内部块）与 `PLANNER_RESPONSE`（仅保留可见 `content`，过滤 `thinking`、`tool_calls` 与 `GENERIC` 工具输出）。会话名优先使用 `conversation_title`，缺席时回退首条可见提问。进程退出或同终端切换会话后返回 `410`（会话已结束）。首期不包含 Nomad 历史浏览、远程回复与继续对话。
- **ESP-Mosaico**：同步更新 [output/model.rs](../../../app/src-tauri/src/output/model.rs)、[device.c](../../../firmware/mosaico/main/device.c) 与 [ui.c](../../../firmware/mosaico/main/ui.c) 接受并展示 `antigravity` 来源（显示 `Antigravity`），补充 Rust 与 C 自动化测试。已刷旧版固件遇到含 Antigravity 会话的快照时会校验拒绝并安全回退 Mac 桌面，重新构建刷机后可在设备端展示。

## 实测验证摘要

1. **自动化测试**：Rust 83 项、Node.js 32 项（含 `antigravity-hook.test.js`、`usage-model.test.js`、`manager.test.js`）、Mosaico C 核心 5 组与串口模拟器、Python 6 项全部通过。
2. **隔离注入验证**：启动调试版 `red-green`（`RED_GREEN_STATUS_DIR` + `RED_GREEN_DISABLE_CODEX=1`），验证首见静默、六态叫声（`robot-awaiting` / `your_turn` / `completed` / `aborted`）、同状态不重叫、三来源同屏共存、Antigravity 用量表（含过期显示 `?`）与退出清理。
3. **真实 `agy` 1.5.18 会话端到端验证**：通过 `--settings` 与隔离 `.agents/hooks.json` 驱动真实交互 `agy`，依次实测启动 `idle` + 真实 Gemini/3p 用量采集 → 命令审批 `statusLine:confirm` (`awaiting`) → 批准后问句收尾 `Stop:NO_TOOL_CALL` (`your_turn`) + 真实 `transcript_full.jsonl` 映射 → `ask_question` 触发 `PreToolUse:ask_question` (`awaiting`) → 陈述收尾 (`completed`) → `Esc` 中断归 `idle` → 退出离场。
