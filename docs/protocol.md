# 状态文件协议

Claude 的会话文件由 hooks 写，用量文件由 status line 脚本写，app 只读；Codex 的状态观察和阅读设备的回复使用文末的 App Server 协议；Antigravity CLI 由 `statusLine` 与极简 `hooks.json` 写同目录的独立状态与用量文件，app 只读并复核进程存活。领域词汇见根目录 [CONTEXT.md](../CONTEXT.md)。

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

## Codex CLI 原生观察

安装仅将 `thread-id` 加到 `tui.terminal_title` 首位。运行时由标题中的会话 ID（0.162.1 标题缩为 29 字符加省略号）和当前 tty 上的 Codex 进程共同绑定会话。Terminal 读 custom title/tty；Ghostty 读 name/id 并复用 OSC 7 映射；tmux 读 pane title/tty。退出或失去可信绑定时宠物离场，不扫描历史记录，缩略 ID 需与全部已加载 ID 唯一匹配，歧义时不绑定，不按 cwd 匹配。同 thread 多客户端只保留 tty 排序的第一个。

目录优先级：运行 app 的 `CODEX_HOME` → 安装脚本登记的 `~/.config/red-green/codex-home` → `~/.codex`。连接 `<目录>/app-server-control/app-server-control.sock`，以 WebSocket 握手后调用 `initialize`（experimentalApi）、发送 `initialized`。每 1.5 秒先读取 `thread/loaded/list`（遍历全部分页、只对当前已加载会话匹配 ID），再读取 `thread/read(includeTurns:false)` 与 `thread/turns/list(limit:1,itemsView:summary)`。不调用 resume/subscribe 或审批接口；子代理、notLoaded 会话不显示。断线清空快照并自动重连，重连首次状态不补叫声。

| 服务状态 / 最近回合 | 宠物状态 |
|---|---|
| active + waitingOnApproval / waitingOnUserInput | awaiting |
| active，无上述标志（包括重试） | running |
| systemError 或 idle + failed | aborted |
| idle + completed，末条非 commentary 答复末行命中问句规则 | your_turn |
| idle + completed，其余 | completed |
| idle + interrupted / 无回合（含首条消息前尚未 materialized） | idle |

问句规则与 Claude 的末行启发式一致；用户中断不是外部失败。归一化对象沿用会话结构，加 `source:"codex"`，`session_id:"codex-<UUID>"`，`event:"app-server"`。`since` 是回合 ID、状态、完成时间构成的稳定事件标识，不是采样时间。项目名首次观察时按 git 根目录锚定。复用已阅、前台静默与点击聚焦。

`get_usage` / `usage-changed` 返回 `{claude:<原 usage.json 或 null>,codex:<快照或 null>,antigravity:<antigravity-usage.json 或 null>}`。Codex 快照：

```json
{"updated_at":1792100000,"groups":[{"id":"codex:account:codex","source":"codex","account_id":"account","label":"Codex","windows":[{"key":"primary","used_percentage":23,"duration_minutes":300,"resets_at":1792103600}]}]}
```

`account/rateLimits/read` 首次有会话时请求，随后每 60 秒刷新。优先使用 rateLimitsByLimitId，回退单一 rateLimits；不按会话累加。账号变化通知清缓存。未知、非法、过期或超过 120 秒的快照不显示成零；失败时旧快照仅在有效期内继续显示。用量表仅列有存活宠物的来源，多额度组时每 8 秒自动轮播（悬停时停在当前组），点击也可手动切换并重新计时，未知显示 `?` / `未知`。Claude 的既有过期规则不变。

隔离验证可设置 `RED_GREEN_STATUS_DIR` 改用临时状态目录；配合 `RED_GREEN_DISABLE_CODEX=1` 停止原生观察器，并从该目录的 `codex-usage.json` 注入用量快照。此文件、`usage.json` 与 `antigravity-usage.json` 均不当作会话。常规运行不读取 Codex 用量文件。

## Antigravity CLI 接入

安装脚本（`scripts/install-antigravity.sh`）将 `hooks/antigravity.sh` 链接为 `~/.gemini/antigravity-cli/red-green-status.sh`，并幂等写入：

- `~/.gemini/antigravity-cli/settings.json`：优先配置 `statusLine`（`stack_with_default: true`，脚本零输出以保留内置底栏）；已有自定义 `statusLine` 时回退到零输出 `title` 槽位；两槽均被占用时不覆盖，提示在自定义脚本中转发 stdin。
- `~/.gemini/config/hooks.json`：写入独立 `"red-green"` 条目，仅订阅 `Stop` 与 `matcher: "ask_question"` 的 `PreToolUse` / `PostToolUse`。脚本严格按 `/.gemini/antigravity-cli/` 路径过滤，忽略 Antigravity App、IDE 及无交互终端的 `agy -p` 运行。

基线为 Antigravity CLI 1.5.18。交互式 `agy` 会话以进程身份写入 `~/.claude/session-status/antigravity-pid-<pid>.json`（`source: "antigravity"`、`session_id: "pid-<pid>"`，附 `pid`、`conversation_id`、`title`、`tty`、`project` 等字段）；项目名首次写入即按 Git 根目录锚定。app 侧保持只读，在读取快照与 1.5 秒心跳中通过一次批量 `ps` 校验 `pid` 仍为对应 `tty` 上存活的 `agy` 进程；进程退出后宠物在 1.5 秒内离场。隔离验证注入的不带 `pid` 假文件或设置 `RED_GREEN_DISABLE_ANTIGRAVITY_PID_CHECK=1` 时跳过进程复核。

| 来源事件 / 字段 | 宠物状态 | 备注 |
| --- | --- | --- |
| `statusLine`: `authenticating` / `initializing` / 初始 `idle` | `idle` | 启动入场；未提问前 `conversation_id` 为空 |
| `statusLine`: `working` / `thinking` / `tool_use` | `running` | 若处于 `PreToolUse:ask_question`，`tool_use` 不覆盖 `awaiting` |
| `statusLine`: `tool_confirmation_pending: true` 或 `agent_state: "reviewing"` | `awaiting` | 权限确认与产物评审 |
| 钩子 `PreToolUse` / `PostToolUse`（`ask_question`） | `awaiting` → `running` | 补齐 `ask_question` 弹窗不置 `tool_confirmation_pending` 的缺口 |
| 钩子 `Stop`（`NO_TOOL_CALL` 等正常结束） | `your_turn` / `completed` | 读取当前回合末条可见助手答复，复用末行问句启发式 |
| 钩子 `Stop`（`USER_CANCELED`） | `idle` | 用户取消不是外部异常中止 |
| 钩子 `Stop`（`ERROR` / 超限 / 非空 `error`）或 `statusLine: error` | `aborted` | |

`statusLine` 与 `Stop` 钩子并发写同一文件时经系统临时目录下的每 PID 锁目录串行化并原子重命名（不落入被监听的状态目录，避免未变化重绘触发 watcher）；`Stop` 先到后 `statusLine: idle` 保留终态与 `since`，`statusLine: idle` 先到时按对话记录末条非工具答复兜底判定且后续同态 `Stop` 沿用同一 `since`。`/clear` 产生显式空 `conversation_id` 时重置为 `idle` 并将旧会话阅读映射标为 `ended`。

账号用量由 `statusLine` 中的 `quota` 与经单向哈希脱敏后的账号标识（不落盘明文邮箱，也不经 Mosaico 串口外发）归一化写入 `~/.claude/session-status/antigravity-usage.json`（格式同 Codex 额度组，按 `Gemini`、`Claude / GPT` 等模型组拆分，值未变不重写；切换账号、登出或会话进行中缺席 `quota` 时立即刷新为空分组以展示未知，同账号新窗口启动初期尚未拉回 `quota` 时不覆盖缓存以免闪烁）。`remaining_fraction = 1` 且 `reset_time = 1970-01-01T00:00:00Z` 视为未消耗的真实 `0%`（`resets_at: null`）；缺失、非法或真实 `resets_at` 已过期时在用量表显示 `?` / `未知`，绝不显示成零。

电纸书阅读路径写入 `.reader/antigravity-<conversation_id>`（不进入宠物快照），并自动修正 `agy` 1.5.18 `statusLine` 误指向 `/.gemini/antigravity/` 的路径，优先读取 `~/.gemini/antigravity-cli/brain/<conversation_id>/.system_generated/logs/transcript_full.jsonl`。服务端仅投影 `USER_INPUT`（`USER_EXPLICIT`，剥离 `<USER_REQUEST>` 与系统元数据块）和 `PLANNER_RESPONSE`（`MODEL` 非空 `content`，忽略 `thinking`、`tool_calls` 与 `GENERIC` 工具输出）。会话名优先使用 `conversation_title`，缺席时取首条可见提问。进程退出或同终端切换会话后返回 `410`。开启设备回复权限后，Nomad 与 Kindle 可向当前存活且非 `running`/`awaiting` 的 Antigravity CLI 会话发送一次性凭据回复：服务端核对 `agy` 进程与 `conversation_id` 仍匹配、拒绝含终端控制字符的文本后，经 tmux `paste-buffer -p`、Ghostty 1.3+ `input text` 或 Apple Terminal 括号粘贴不抢焦点送入原终端并回车；Mac 终端继续处理权限确认与 `ask_question`。不开放 Antigravity 历史会话与继续对话。

已知盲区：`statusLine` 有 300ms 节流，短于节流窗口的瞬时状态可能合并；未启用 `hooks.json` 时 `ask_question` 不进入 `awaiting` 且取消与结束仅凭对话记录推断。决策与实测依据见 [ADR 0007](adr/0007-antigravity-cli-integration.md) 与 [设计](superpowers/specs/2026-11-28-antigravity-cli-design.md)。

## 阅读设备配对

Nomad 通过 `POST /pair` 提交 JSON，Kindle 通过 `POST /kindle/pair` 提交 HTML 表单，表单还校验短期随机值。两个入口兑换同一类六位配对码。每个码十分钟内可成功兑换一次，连续五次输错后失效。每次成功兑换均向该浏览器签发独立的 `nomad_reader` Cookie，有效期为 30 天。

Mac 将设备凭据的 SHA-256 摘要逐行保存在原有配对文件中，旧版单条记录仍有效。新设备加入后，已配对设备可继续阅读。Mac 开启回复时授权当时已配对的所有设备，并使尚未使用的配对码失效。换码或重启共享会关闭回复权限。「解除所有设备配对」同时清除设备凭据、配对码和回复授权。

## Nomad 会话共享

首页 `/` 和历史列表 `/archive` 各有独立的 HTML 文档。当前会话详情使用 `/read/current/{key}`，历史详情使用 `/read/history/{key}`。Mac 校验配对凭据和会话可见性后，在初始 HTML 的 `<title>` 写入「来源 · 状态 · 项目名 · 会话名」，所以直达或刷新都能打开带标题的详情。未配对时返回的页面只提供配对入口。

页面在前台轮询正文，使用 `since` 游标获取增量，使用 `before` 游标向前补页。每条消息的 `startsTurn` 标记轮次起点，读取较早正文时按轮次边界返回，页面一次显示一轮。即使 `messages` 为空，也要处理同一响应里的会话名和状态。

| 读取接口 | 详情标题使用的状态 |
| --- | --- |
| `GET /sessions/{key}/messages` | `state` 取自当前可读会话快照。来源明确返回 410 时，页面显示「已结束」并保留已加载的正文。 |
| `GET /history/{key}/messages` | `live` 表示原线程已绑定可读终端。此时响应携带同一快照的 `state`，即使正在回看前文，仍可更新标题。未绑定时显示「只读」。 |

标题变化时，页面先更新 `document.title`，再用 `location.replace` 替换同一文档的 URL 片段，让浏览器收到导航事件。这样可以保留已加载的正文、滚动位置和输入焦点，浏览器后退仍可回到笔记。标题相同的轮询会跳过这一步。

历史列表通过本机 App Server 的 `thread/list` 按更新时间倒序分页，范围为当前账号、未归档、顶层 Codex CLI 会话。分页游标和列表心跳绑定同一账号连接，账号切换后清除设备上的旧列表。历史正文按原线程 ID 核对可见性后读取回合。设备跟随最新内容且终端已就绪时，切换到当前会话接口，使用该接口的游标和回复凭据。

在 Mac 为配对设备开启回复后，点「继续对话」会携带该线程的一次性恢复凭据。服务优先复用原线程已绑定的终端。如果上次打开的标签仍在等待绑定，重复请求会继续等待。需要新开终端时，服务在原工作目录启动 `codex resume` 并连接原本机服务。

终端就绪后，设备可以发送回复。发送文字须携带当前 Codex 或 Antigravity 会话的一次性发送凭据。Codex 核对会话仍已加载且空闲后以原线程 ID 调用 `turn/start`；Antigravity 核对 `agy` 进程仍存活于原 `tty`、同一 `conversation_id` 且非运行中/待确认后，经括号粘贴送入原终端并回车。Mac 终端继续处理审批，两端同时发送时可能合入同一回合。


## Kindle 会话共享

`GET /kindle` 返回配对表单或当前会话列表。`GET /kindle/read/{key}` 返回一轮完整对话的 HTML，首次打开定位到最新一轮开头。正文里的链接可切换相邻轮次。启用脚本时，阅读页用 `X-Kindle-Since` 轮询同一地址。请求头 `X-Kindle-State` 携带页面已显示的会话状态。正文、状态及回复权限均无变化时返回 204。仅状态或权限变化时返回带有最新状态和回复入口的 JSON，客户端就地更新底栏。回看时可只请求更新状态，保留阅读位置。

允许回复后，当前 Codex 与 Antigravity 会话提供 `/kindle/reply/{key}`。GET 返回输入表单，POST 携带该会话的一次性发送凭据。提交成功后返回 303，浏览器回到最新正文。配对、手动翻轮和提交回复均可在关闭脚本时使用。
