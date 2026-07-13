# 状态文件协议

hooks 与宠物 app 之间的唯一契约。领域词汇见根目录 [CONTEXT.md](../CONTEXT.md)。

## 文件

每个存活的 Claude Code 会话对应一个文件：

```
~/.claude/session-status/<session_id>.json
```

`SessionEnd` 时删除。宠物 app 只读；hooks 只写。

## 内容

```json
{
  "session_id": "uuid",
  "state": "idle | running | awaiting | your_turn | completed | aborted",
  "cwd": "/Users/y9g/repositories/red-green",
  "project": "red-green（git 仓库根目录名；cwd 不在 git 仓库内时取 cwd 末级目录名。cwd 会随会话内的 cd 漂移，项目身份不随之漂移——首次写入即锚定，后续事件沿用文件中已有的值。两者都取不到时为 ?）",
  "tty": "/dev/ttys012",
  "detail": "最近一条通知消息或 Claude 最后一句话的摘录（≤300 字符）",
  "since": "2026-07-13T08:00:00Z"
}
```

`state` 是英文标识符，与 CONTEXT.md 术语的对应：`idle`=空闲、`running`=运行中、`awaiting`=待确认、`your_turn`=轮到你、`completed`=已完成、`aborted`=异常中止。

`tty` 用于点击聚焦（AppleScript 按 tty 匹配 Terminal 标签页）和已阅检测（前台标签页的 tty == 该会话 tty）。hook 进程自身没有 tty，脚本沿进程树向上找；找不到时保留上一次记录的值。

**tmux**：pane 里的会话记录的是 pane 的 tty，它不在 Terminal 的任何 tab 上。app 在点击聚焦时动态解析：pane tty → tmux 目标（session:window.pane，让 tmux 切过去）→ 挂载客户端的 tty → 真正的 Terminal tab。反向地，已阅检测发现前台 tab 是 tmux 客户端时，取其 session 活动 pane 的 tty 作为"用户实际在看"的 tty。解析放在点击/检测时而非记录时，因为 tmux 客户端可以随时换地方 re-attach。

文件另带 `event` 字段（产生当前状态的 hook 事件名），配合同目录 `.events.log`（滚动事件日志，每行 `时间 会话前缀 事件 -> 状态`）用于诊断"宠物状态与体感不符"。

## 已知盲区：纯思考期

`UserPromptSubmit` 与第一次工具调用之间若 Claude 长时间思考/生成文本，期间没有任何 hook 事件。若此前状态是 `idle`（如会话刚重启），宠物会睡到第一次工具调用才醒。事件流层面无解，属已接受的限制。

## 状态机总览

```
                       SessionStart
                            │
                            ▼
              ┌─────────  空闲  ◀─────────────── 已阅（聚焦该会话终端；
              │          (睡觉)                   仅对已完成/异常中止生效）
              │                                          ▲
     UserPromptSubmit                                    │
              │                                ┌─────────┴─────────┐
              ▼                                │                   │
   ┌──────  运行中  ◀───────────────┐       已完成             异常中止
   │     (敲键盘/verb)              │      (比耶,轻响)        (躺平,响铃)
   │        │    │                  │          ▲                   ▲
   │        │    │ Notification     │          │ Stop:陈述结尾      │ StopFailure
   │        │    │ (权限/弹窗提问)  │          │                   │
   │        │    ▼                  │          │                   │
   │        │  待确认 ──────────────┘──────────┴───────────────────┘
   │        │  (举牌跳,响铃)  用户确认后 PreToolUse/PostToolUse 回到运行中
   │        │
   │        │ Stop:问句结尾
   │        ▼
   │      轮到你 ─────── 用户作答 UserPromptSubmit ────▶ 运行中
   │  (探头张望,蓝灯闪,鸣响)  不适用已阅：问题没回答就一直成立
   │
   └── SessionEnd（任何状态）→ 删除状态文件，宠物离场
```

两层结构：**hooks 写的是原始状态**（上图实线），**app 在显示层叠加已阅**（已完成/异常中止 + 前台聚焦 → 显示为空闲，不写回文件）。叫声在进入待确认/异常中止/轮到你/已完成时各响一次：音色属物种、节奏属状态（节奏语法见 [CONTEXT.md](../CONTEXT.md)「叫声」），文件名 `皮肤-状态.wav`（契约定义在 `app/ui/calls.js`），由 `scripts/gen-calls.mjs` 生成、随 .app 的 bundle resources 打包。受前台静默约束：该会话终端标签页在前台则不响（tmux 解析到活动 pane），被静默即消失，不补发。

## 事件 → 状态映射

| Hook 事件 | 状态 | 备注 |
|-----------|------|------|
| `SessionStart` | `idle` | 宠物入场 |
| `UserPromptSubmit` | `running` | |
| `PreToolUse` / `PostToolUse` | `running` | 心跳；覆盖"权限确认后恢复运行"的转移。running→running 不重写文件，避免刷爆 watcher |
| `Notification`（matcher: `permission_prompt\|elicitation_dialog\|agent_needs_input`） | `awaiting` | 只有回合中途的真阻塞；`idle_prompt`（空闲计时器）被有意排除——它不携带语义，见下 |
| `Stop` | `completed` 或 `your_turn` | 问句启发式**只看消息最后一行**：以问号收尾（容忍 markdown 收尾符）或含明确请求答复短语 → `your_turn`，否则 `completed`。宽匹配（尾部任意问号、"是否"）已被实践证伪——长篇分析正文误报。`your_turn` 时 `detail` 存消息尾部（问题所在），其余存开头 |
| `StopFailure` | `aborted` | 限流/账单/服务器错误 |
| `SessionEnd` | （删除文件） | 宠物离场 |

## 已阅（app 内存态，不写回文件）

`completed` / `aborted` 的会话，当其 `tty` 成为前台 Terminal 标签页时视为已阅，app 内将其显示为空闲。`since` 变化（新事件）即重置已阅标记。不写回文件是为了保持"hooks 只写、app 只读"的单向数据流。

`your_turn` 不适用已阅：Claude 的问题没被回答前，"轮到你"持续成立，直到 `UserPromptSubmit`（用户作答）转回 `running`。

## 为什么 idle_prompt 被排除

`idle_prompt` 只是"回合结束后 60 秒无输入"的计时器，不携带任何语义——不管 Claude 是交付了还是提问了都会触发。若映射为 `awaiting`，每个回合结束 60 秒后都会变红，红色迅速贬值。"回合以提问收尾"这个真语义改由 Stop 时的问句启发式承担。

## 实现期待验证

- AskUserQuestion 是否触发 `Notification` hook（若不触发，兜底是 `Stop` + `last_assistant_message` 含问句的启发式）。
- `idle_prompt` 的确切触发时机（延迟多久）。
- 进程树向上找 tty 在真实 hook 环境中是否可靠。
