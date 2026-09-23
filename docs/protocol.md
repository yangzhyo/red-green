# 状态文件协议

hooks、status line 脚本与宠物 app 之间的唯一契约：会话文件由 hooks 写，用量文件由 status line 脚本写，app 只读。领域词汇见根目录 [CONTEXT.md](../CONTEXT.md)。

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

`tty` 用于点击聚焦和已阅检测（前台标签页的 tty == 该会话 tty）。hook 进程自身没有 tty，脚本沿进程树向上找；找不到时保留上一次记录的值。

**终端**：会话跑在哪个终端 app 里由 app 按 tty 现场判定——终端为每个标签页在 tty 上起一个 login，login 的父进程就是终端 app。Terminal.app 的 AppleScript 直接按 tty 匹配标签页。Ghostty（1.3 起有 AppleScript）的 terminal 不带 tty，app 往 tty 写一条带记号的 OSC 7、看哪个 terminal 的工作目录变成了记号，由此对上 terminal id 并缓存，认出后把原目录写回（见 [ADR 0005](adr/0005-ghostty-osc7-probe.md)）。其他终端不支持：点击什么都不做，也不会去启动 Terminal。

**tmux**：pane 里的会话记录的是 pane 的 tty，它不在终端的任何标签页上。app 在点击聚焦时动态解析：pane tty → tmux 目标（session:window.pane，让 tmux 切过去）→ 挂载客户端的 tty → 真正的终端标签页。反向地，已阅检测发现前台标签页是 tmux 客户端时，取其 session 活动 pane 的 tty 作为"用户实际在看"的 tty。解析放在点击/检测时而非记录时，因为 tmux 客户端可以随时换地方 re-attach。

文件另带 `event` 字段（产生当前状态的 hook 事件名），配合同目录 `.events.log`（滚动事件日志，每行 `时间 会话前缀 事件 -> 状态`）用于诊断"宠物状态与体感不符"。

## 用量文件

账号级用量与会话文件同目录、共用一个 watcher，靠文件名区分：

```
~/.claude/session-status/usage.json
```

写入方是 status line 脚本 `statusline/usage.sh`（装为 `~/.claude/red-green-usage.sh`，由 settings.json 的 `statusLine` 调用，不打印任何内容）。Claude Code 只在交给 status line 的 JSON 里给出 `rate_limits`，hooks 事件里没有，所以它是 hooks 之外唯一的写入通路。app 只读。

```json
{
  "five_hour": { "used_percentage": 23.5, "resets_at": 1738425600 },
  "seven_day": { "used_percentage": 41.2, "resets_at": 1738857600 },
  "updated_at": "2026-09-18T08:00:00Z"
}
```

- 两个窗口各含已用比例（0–100）与重置时刻（Unix 秒）；哪个窗口缺席就不写哪个键。
- 所有会话的 status line 都写这同一个文件（用量属于账号）；值没变不重写，避免每次重绘都触发 watcher。
- `rate_limits` 缺席（API 计费、会话首次响应之前）时不动现有文件——缺席不代表归零。
- app 侧：manager 把它裁决成一份视图交给用量表（独立小窗，见 CONTEXT.md）；重置时刻已过的窗口视为 0%、重置时刻未知（窗口已清零，下一次响应前没有新值）；所有窗口都已过重置时刻视同无数据。文件不存在、无法解析或无数据则没有用量表。用量不引入叫声，不进状态机。
- app 靠文件名跳过 `usage.json`，目录下其他 `*.json` 仍按会话文件处理。

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

`completed` / `aborted` 的会话，当其 `tty` 成为前台终端标签页时视为已阅，app 内将其显示为空闲。`since` 变化（新事件）即重置已阅标记。不写回文件是为了保持"hooks 只写、app 只读"的单向数据流。

`your_turn` 不适用已阅：Claude 的问题没被回答前，"轮到你"持续成立，直到 `UserPromptSubmit`（用户作答）转回 `running`。

## 为什么 idle_prompt 被排除

`idle_prompt` 只是"回合结束后 60 秒无输入"的计时器，不携带任何语义——不管 Claude 是交付了还是提问了都会触发。若映射为 `awaiting`，每个回合结束 60 秒后都会变红，红色迅速贬值。"回合以提问收尾"这个真语义改由 Stop 时的问句启发式承担。

## 实现期待验证

- AskUserQuestion 是否触发 `Notification` hook（若不触发，兜底是 `Stop` + `last_assistant_message` 含问句的启发式）。
- `idle_prompt` 的确切触发时机（延迟多久）。
- 进程树向上找 tty 在真实 hook 环境中是否可靠。
