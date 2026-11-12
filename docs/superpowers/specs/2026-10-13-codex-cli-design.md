# #16：Codex CLI 会话与账号用量接入设计

关联需求：[支持 Codex CLI](https://github.com/yangzhyo/red-green/issues/16)。2026-10-16 用户确认：保持原生 `codex` 启动方式，可以实施。

## 接入

安装时在 Codex 的 `tui.terminal_title` 首位加入 `thread-id`，保留已有标题项。用户继续直接运行 `codex`，不增加启动器、不代理通信、不修改审批流程。标题会显示会话 ID，这是准确识别同目录多个会话的必要配置。

基线为 Codex CLI 0.162.1 的本机共享 App Server。通过只读 Unix WebSocket 接口读取会话状态、最近回合及账号用量；不订阅或恢复用户会话，不启动模型任务。`--no-daemon`、导致内嵌服务的配置组合及远程服务不在首版支持范围，必须在安装和使用文档说明。

## 终端和生命周期

Codex 的原生标题 `thread-id` 已从对应版本源码及真实终端确认，渲染会截成 29 字符加省略号。Terminal 的 custom title 与 tty 已实测可读；Ghostty 有 terminal name/id，由既有 OSC 7 探针对应 tty；tmux 使用 pane title/tty。所有绑定必须同时满足会话 ID 和活跃 Codex 进程的 tty，缩略 ID 必须在全部已加载会话中唯一匹配，不按 cwd 猜测。

独立观察线程每 1.5 秒扫描本机支持的终端；只显示确实仍在终端中打开、服务已加载的主会话。正常退出、终端关闭、进程异常退出均使宠物离场。多个 CLI 打开同一 thread 时视为一个会话；优先选稳定排序的第一个终端。轮询可能漏掉短于采样间隔的瞬时状态。

保持既有 tty 聚焦、已阅、前台静默和按项目分配皮肤。Codex 会话使用来源命名空间，名牌 tooltip 显示来源。应用不需要读写 Codex transcript 或账号凭据，也不需要安装 Hooks。

## 状态

App Server `active` → 运行中；activeFlags 中 waitingOnApproval / waitingOnUserInput → 待确认；systemError 或最近回合 failed → 异常中止。idle 时读取最近回合：completed 且最后答复末行命中既有问句规则 → 轮到你，否则已完成；interrupted → 空闲；没有回合 → 空闲。正在重试的错误、单次工具失败不能推导为回合失败。

轮到你不会被已阅清除。事件身份由回合 ID、状态和完成时间构成，重复轮询不刷新已阅或重复叫声。会话首次被观察时不补发旧叫声。断线清空不可信快照，恢复后静默采纳。

## 用量

一个观察线程统一读取 `account/rateLimits/read`，首次出现 Codex 会话时读取，此后每 60 秒刷新；同账号多个会话不重复请求或计算。按账号与 limitId 分组，保留 primary/secondary 的实际窗口时长、已用比例与重置时刻，不假设所有窗口都是五小时/七天。

用量表仍是列顶一扇窗口，常驻显示来源标签，支持在来源/额度组之间切换；悬停展开窗口明细和数据更新时间。默认选择 Codex 的 codex 额度组（只有 Claude 时选 Claude），其他来源可切换查看。用量不进入宠物状态机，不发出叫声。

缺失/非法/过期窗口及 API 计费没有额度数据时显示未知，不显示 0%。账号切换不沿用旧账号数据；读取失败可保留上一次成功快照至 120 秒，标明更新时间，之后显示不可用。Claude 的既有过期语义保持兼容。只有存在对应来源宠物时才展示该来源用量。

## 边界与文件

新增 Rust Codex 模块包含协议读取、终端发现、状态归一化，向主程序提供内存快照；主程序合并 Claude 的原有文件快照。纯模型模块承接状态/用量测试，RPC 适配测试使用本地假服务。前端抽取可测试的用量视图，并扩展同一用量表。安装脚本使用 TOML 解析器保留配置注释和其他设置，幂等、先备份、尊重 CODEX_HOME；只更改标题项。

测试目录覆盖用于隔离应用验证，继续支持注入假会话和用量。执行 Rust/JS 测试、现有 CI 检查、桌面渲染与真实 CLI 退出/聚焦验证；通过双轴审查后交付 PR，不自动合并。

## 依据与证据

- [Codex App Server 官方文档](https://learn.chatgpt.com/docs/app-server)：Unix WebSocket、只读状态与用量接口。
- [0.162.1 原生标题选项源码](https://github.com/openai/codex/blob/rust-v0.162.1/codex-rs/tui/src/bottom_pane/title_setup.rs)；[实际渲染源码](https://github.com/openai/codex/blob/rust-v0.162.1/codex-rs/tui/src/chatwidget/status_surfaces.rs)会将 ID 截成 29 字符加省略号。
- [上游对后台服务 Hook 的说明](https://github.com/openai/codex/issues/48500)：Hooks 不持有客户端身份；此设计使用客户端生成的标题绑定。
- 本机只读探针已验证握手、额度、thread/loaded/list、thread/read、thread/turns/list；真实 Terminal 已验证 OSC 0 标题与 tty 的读取。后续实现验证结果记入 PR。
