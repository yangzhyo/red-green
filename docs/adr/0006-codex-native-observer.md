# ADR 0006：通过原生标题与只读 App Server 接入 Codex CLI

状态：已接受；关联 #16。

Codex CLI 的默认共享服务执行 Hooks 时不持有客户端 tty。同目录可能有多个会话，不能通过 cwd 或 daemon 继承的环境变量判断应聚焦哪个终端。要求用户改用包装命令会改变日常启动方式。

一次性在 `tui.terminal_title` 首位加入 `thread-id`，之后仍直接运行 `codex`。从 Terminal custom title、Ghostty terminal name、tmux pane title 读取 ID，并与该 tty 上存活的 Codex 进程交叉验证。Ghostty 复用 ADR 0005 的 OSC 7 映射。0.162.1 会将 ID 截成 29 字符加省略号，因此还需在所有已加载会话中唯一匹配；歧义时不绑定。缺少明确绑定时不猜测。

观察器连接本机 App Server 的 Unix WebSocket，仅调用 initialize、thread/loaded/list、thread/read、thread/turns/list、account/rateLimits/read；不恢复、订阅或控制会话。状态归一化后合入 manager，Codex 会话 ID 加 `codex-` 前缀。用量由同一观察器按账号/额度组缓存，进入单独用量视图，避免每个会话重复计算。

代价：标题增加会话 ID（0.162.1 标题缩为 29 字符加省略号）；需自动化权限；轮询可能漏掉瞬时状态。基线为 0.162.1 的本机共享服务，内嵌/远程服务及同时使用多个 CODEX_HOME 暂不支持。回合列表接口属实验性能力，升级需验证。掉线清空观察快照，恢复时静默采纳。

协议与状态映射见 [protocol.md](../protocol.md)，调研依据见 [设计](../superpowers/specs/2026-10-13-codex-cli-design.md)。
