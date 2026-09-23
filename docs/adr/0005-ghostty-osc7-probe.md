# Ghostty 标签页靠 OSC 7 探针对上 tty

点击宠物要切到会话所在的标签页，已阅与前台静默要知道前台标签页是哪个会话的——两边都得把会话记录的 tty 和终端里的标签页对上号。Terminal.app 的 AppleScript 直接给出每个 tab 的 tty。Ghostty 1.3 起也有 AppleScript 字典，terminal 有稳定 id、能 `focus`，但没有 tty 属性，shell 环境里也没有 terminal id，两边之间缺一个共同的键。

做法是探针：往会话的 tty 写一条 OSC 7（shell 用来上报工作目录的转义序列），路径里带一次性记号，再读所有 terminal 的 `working directory`，变成记号的那个就是它；认出后立刻把原目录写回（OSC 7 路径按 URL 编码，主机名写 `localhost`——Ghostty 忽略空主机名）。terminal 存活期间 tty 不变，对应关系缓存在 app 内存里：点击时缓存失效（focus 报错）就当场重探；前台检测遇到没登记的 terminal 才探一轮，同样的「前台 terminal + 会话 tty」组合只探一次，普通 shell 标签页停在前台时不会反复往会话里写。

会话跑在哪个终端 app 里也由 app 现场判定：tty 上父进程不在该 tty 上的那个进程是 login，它的父进程就是终端 app。只认 Terminal.app 与 Ghostty；不认识的终端点击时什么都不做——以前的实现不分终端直接 `tell application "Terminal"`，而 tell 一个没在跑的 app 会把它启动起来，这正是 Ghostty 用户点宠物却弹出 Terminal 的原因。

代价：往用户正在用的 tty 写字节。OSC 7 不显示，但若恰好插进 Claude Code 自己正在输出的一段转义序列中间，那一帧会画花；探针只在对应关系未知时写，每个标签页一生通常一次，接受。认不出的探针无法写回（不知道记号落在了哪个 terminal 上），好在 shell 下次出提示符时会重新上报工作目录。

## Considered Options

- 标题探针（OSC 2）：同样能认，但 Claude Code 会不停刷新标题（运行中的转圈图标），探针会被覆盖，标签栏也会闪一下，被否。
- 按工作目录匹配：同一目录开多个会话（见 #9）或同目录另有普通 shell 标签页时有歧义，被否。
- 按 Ghostty 为各标签页起的 login 进程顺序对应 terminal 列表：两边顺序没有保证，分屏、关闭后更不可靠，被否。
