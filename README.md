<p align="center">
  <img src="docs/assets/hero.png" width="860"
       alt="red-green — a desktop pet for every Claude Code or Codex CLI session, so you always know which one needs you" />
</p>

<p align="center">
  <em>A desktop pet for every Claude Code or Codex CLI session — so you always know which one needs you.</em>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/license-MIT-52c41a?style=flat-square" alt="license: MIT" />
  <img src="https://img.shields.io/badge/platform-macOS-8a9099?style=flat-square" alt="platform: macOS" />
  <img src="https://img.shields.io/badge/built%20with-Tauri%202-ffc53d?style=flat-square" alt="built with Tauri 2" />
</p>

## 这是什么

同时开着好几个 Claude Code / Codex CLI 会话，一旦切到别的窗口，就再也感知不到哪个在等你确认、哪个已经跑完、哪个挂了。

**red-green** 把每个本机会话变成一只常驻桌面、置顶于所有窗口的**像素宠物**，用**行为动画**告诉你会话此刻的**状态**——敲键盘 = 运行中，举牌跳 = 待确认，探头张望 = 轮到你，比耶 = 已完成，躺平 = 异常中止，睡觉 = 空闲。

- **一会话一宠物**，脚下**名牌**写着项目名；**皮肤**按项目名哈希分配（灯灯🚦 / 钳钳🦀 / 灰灰🐱），同一项目永远同一只——看形象就知是哪个项目。
- **点宠物**直接切到会话所在的终端标签页（Apple Terminal 与 Ghostty 1.3+，tmux 也认）。
- 需要拉回注意力的状态还会**叫**：音色随皮肤（听声辨项目），节奏随状态（听节奏辨事件）；你正看着的那个会话不出声（**前台静默**）。
- 宠物列顶上有一枚**用量表**：按来源显示账号用量，点击切换 Claude / Codex 及额度组，默认显示 Codex。外圈和圆心是首窗口，内圈是第二窗口，灯色从绿到红；悬停展开实际窗口时长与重置时刻。Codex 数据缺失或过期显示未知。用量耗尽前就能看到，不用回终端敲 `/usage`。
- 整列沿屏幕右缘**从上往下**排，起点避开聊天窗口贴顶时的工具栏，下部留给发送按钮之类的东西；宠物和用量表都可拖动。
- **ESP-Mosaico** 可以通过 USB-C 接管宠物、叫声和用量表，拔线后自动回到 Mac。[在 CoreBoard V1.2.1 上使用](#在-esp-mosaico-上显示宠物和用量)
- **Supernote Nomad** 可以在同一 Wi-Fi 上查看会话、跟随新消息，并在 Mac 授权后回复 Codex。[在 Nomad 上查看和回复会话](#在-supernote-nomad-上查看和回复会话)

## 状态图鉴

<p align="center">
  <img src="docs/assets/states.png" width="860"
       alt="red-green 三套皮肤（灯灯 / 钳钳 / 灰灰）× 六个会话状态：运行中、待确认、轮到你、已完成、异常中止、空闲" />
</p>

六个**会话状态**由 Claude Code hook 或 Codex App Server 状态判定，驱动宠物的姿态、气泡与叫声。完整定义——含「轮到你」的问句启发式、「已阅」规则（聚焦终端即视为看到，但看到问题不等于回答了它）、前台静默——见 [CONTEXT.md](CONTEXT.md)。

## 安装与运行

```sh
./install.sh                 # Claude Code：装 hooks 与 status line
./scripts/install-codex.sh    # Codex CLI：一次性配置原生标题（可只装所需来源）
cd app && pnpm install       # 2. 前端依赖（仅 @tauri-apps/cli）
../scripts/install-app.sh    # 3. 构建 .app 装进 ~/Applications 并注册登录自启
```

常驻版正常或异常退出后都会自动重启。需要停用时执行 `launchctl unload ~/Library/LaunchAgents/dev.y9g.red-green.plist`，恢复用 `launchctl load ~/Library/LaunchAgents/dev.y9g.red-green.plist`。开发迭代用 `pnpm tauri dev`，先按上述方式停用常驻版。

首次运行时 macOS 会请求「自动化」权限（控制 System Events，以及 Terminal / Ghostty）——点击聚焦与已阅检测靠它；`.app` 是独立应用身份，会各弹一次。

### Codex CLI

安装后直接运行 `codex`，无需 `red-green-codex` 包装命令。安装脚本将 `thread-id` 加到 `tui.terminal_title` 首位，其余标题项、设置、注释保留，原配置旁生成 `.red-green-时间戳.bak` 备份。重复安装无重复项。安装时尊重 `CODEX_HOME`，并将目录登记到 `~/.config/red-green/codex-home`，使登录自启的 app 也能找到它。修改后重启 red-green，并新开 Codex 会话。手动配置亦可：

```toml
[tui]
terminal_title = ["thread-id", "activity", "thread-name", "project-name"]
```

已验证 **Codex CLI 0.162.1、本机默认共享 App Server**。标题会显示会话 ID（0.162.1 标题缩为 29 字符加省略号），用它准确区分同目录多会话；请保留该项并允许终端更新标题。Terminal、Ghostty 1.3+ 和 tmux 沿用原有聚焦通路。首次运行允许 macOS 自动化权限。

限制：`--no-daemon`、强制使用内嵌服务的 `-c` 配置、远程 App Server、多个不同 `CODEX_HOME` 同时运行，以及其他终端不在本版支持范围。使用自定义配置时优先写入 `config.toml`。同一 thread 同时在多个 CLI 打开只算一个会话，选择 tty 排序的第一个。桌面宠物只观察当前终端中仍存活的 CLI。已退出终端的 Codex 对话可在 Nomad 的「历史会话」中查看。每 1.5 秒采样，短于间隔的状态可能看不到；问句判断是启发式。Codex 升级若改变实验性回合接口，需重新验证兼容性。

账号用量每 60 秒读取一次，不按宠物累计。按账号与额度组区分，API 计费或读取不到限额时显示未知；超过重置时刻或成功快照超过 120 秒也显示未知，悬停提示含最后更新时间。只展示有对应来源宠物的用量。桌面观察器只读取状态和用量，审批由终端处理。

可运行 `app/src-tauri/target/debug/red-green --check-codex` 做只读诊断（输出已绑定会话、状态和额度组数，不输出答复或凭据）。宠物没出现时先检查 `codex --version`、标题中的会话 ID（0.162.1 标题缩为 29 字符加省略号）、`$CODEX_HOME/app-server-control/app-server-control.sock`（默认 `~/.codex`）以及自动化权限。标题为空、进程已退出、会话未在服务中加载或连接失败时不会猜测会话归属。配置文件是符号链接时安装脚本会停止，请手动配置标题。恢复安装前设置可使用备份，或只移除标题中的 `thread-id`；再删除 `~/.config/red-green/codex-home` 即可解除目录登记。

如果重新设置过 Codex 标题，请确认仍保留 `thread-id`；可再次运行安装脚本补回，其他标题项会保留。配置文件修改不会刷新已打开会话的旧标题：在各会话中通过 `/title` 启用 Thread ID 并保存，或在方便时重新打开会话。用 `launchctl list dev.y9g.red-green` 检查常驻进程是否有 PID；已加载但没有运行时，可用 `launchctl kickstart gui/$(id -u)/dev.y9g.red-green` 恢复。

### 在 Supernote Nomad 上查看和回复会话

**首次配对**　在 Mac 和 Nomad 上连接可互访的可信 Wi-Fi。点 Mac 菜单栏的「钳钳」螃蟹图标，选择「开启局域网会话共享」，再打开菜单查看地址和六位配对码。在 Nomad 笔记中插入「网页」链接，填入菜单显示的完整 `http://` 地址。打开链接并输入配对码。配对码十分钟有效，可在 Mac 菜单点按换码。Mac 一次只记住一个已配对的浏览器。用电脑浏览器成功配对后，Nomad 需要重新配对才能继续访问。

**选择会话**

- **首页**的浏览器标题是「red-green · 当前会话」。卡片显示项目、来源、状态和会话名。首次进入首页时，若只有一条可读会话，就会直接打开它。已有 Claude Code 会话在钩子下次提供记录路径后出现，Codex CLI 会话通过本机 App Server 与终端绑定。
- **历史会话**在首页右上角，浏览器标题是「red-green · 历史会话」。这里列出这台 Mac 上当前账号的未归档 Codex CLI 对话，包括终端中仍打开的会话。列表按最近更新排序，每条显示会话名、项目和更新时间，可按项目名筛选。项目取自会话工作目录，在 Git 仓库内显示仓库名。历史功能需要支持当前账号筛选的本机 App Server（Codex CLI 0.171.0 起）。点右上角的「返回首页」可回到当前会话列表。

**阅读与自动刷新**　详情有独立网址，刷新后仍在同一会话。正文从屏幕顶部开始，Markdown 标题、列表、引用、表格和代码块按网页排版。浏览器标题按「来源 · 状态 · 项目名 · 会话名」排列，例如「Codex CLI · 待确认 · red-green · 调整分页」。项目和会话同名时合并显示。Codex 会话没有名称时取预览首个非空行，两者都为空时显示「未命名会话」。

详情在前台约每 3 秒检查一次更新。即使没有新消息，Nomad 原生标题栏也会自动显示最新的会话状态。页面会保留阅读位置、草稿和输入焦点。从历史入口回看已激活的会话也会同步状态，尚未激活的历史会话显示「只读」。[Nomad 真机验收记录 #33](https://github.com/yangzhyo/red-green/issues/33)

在末尾阅读会自动跟随新文字。点「上一屏」回看，新消息到达时会保留当前段落，点「有新消息」可回到末尾。断网后重新连接，只要会话仍可读就会补齐消息。底部的「返回列表」回到首页，「返回历史」回到先前的筛选和位置。从笔记链接进入时，浏览器的后退键可直接回到笔记。

**回复或继续 Codex 对话**　在 Mac 菜单选择「允许配对设备回复 Codex」。从首页或历史进入已激活的会话，都可点底部的「回复」，在页面顶部输入并发送。审批在 Mac 终端处理。如果断网导致发送结果未知，先核对正文再发送。两端同时发送时，Codex 可能把两条输入合入同一回合。

已退出终端的对话可以直接回看。要继续时，点「继续对话」，Mac 会在原工作目录恢复同一会话，并在连接后展开 Nomad 的输入框。Mac 已运行 Ghostty 时优先新开标签，也支持系统「终端」的新窗口。若原目录已移动或删除，已有记录仍可阅读。若提示在 Mac 上查看终端，先处理新窗口内的提示。确认窗口未打开或已关闭后，可在 Mac 关闭再开启共享，重新允许回复后再试。

共享默认关闭。配对设备可通过局域网 HTTP 读取当前和历史列表内的会话。Mac 菜单可解除配对或关闭共享。在同一地址重新开启共享可沿用配对凭据。升级并重启 Mac 应用后，先重新开启共享，再刷新 Nomad 上已打开的详情页。需要回复时，在 Mac 菜单重新授权。换配对码也会关闭回复权限。切换 Wi-Fi 后重新开启共享，若地址变化，在笔记里更新链接。

### 在 ESP-Mosaico 上显示宠物和用量

已在 CoreBoard V1.2.1 上通过真机验收。如果设备上已保存验收版，用 USB-C 数据线连接 Mac，在 Vibe 的「我的 IDEA」中选择 `red_green_6d208cf` 并点「运行」。连接就绪后，设备会接管宠物、叫声和用量表。拔线或健康检查失败时，Mac 自动接回。

480×480 屏幕左侧轮播主角，右侧每页显示五个会话。所有会话空闲 30 秒后熄屏，触摸屏幕或出现非空闲会话即可唤醒。设备沿用桌面的已阅和前台静默规则。首次安装前需核对实机分区并完整备份，保留 Vibe 与恢复区域。固件构建、安装步骤和验收记录见 [Mosaico 开发与验证](docs/mosaico.md)。

## 在 Kindle Paperwhite（第 10 代）上阅读会话

Mac 开启局域网会话共享后，用 Kindle 内置浏览器打开菜单显示的地址，点「Kindle 阅读」。也可以在地址末尾加上 `/kindle`，直接收藏这一页。输入六位配对码后，在列表中选择当前会话。Mac 同时只记住一个配对浏览器，换设备时需要重新配对。

打开会话后从最新一轮的开头读起。每轮提问及其回复放在同一页，长文、表格和代码可上下滚动阅读。轻点正文左半屏回看上一轮，右半屏进入下一轮。页末右侧始终显示「查看最新」「返回列表」「上一屏」「下一屏」，到达首轮或末轮时，相应方向显示为灰色。停在末尾会跟随新内容，回看时点「有新消息」可跳到最新。关闭脚本后仍可用页末链接阅读和手动更新。

## 工作原理

```
hooks/session-status.sh    # Claude Code hook：把会话状态写进 ~/.claude/session-status/
statusline/usage.sh        # Claude Code status line：把账号用量写进同一目录（hooks 拿不到用量）
scripts/install-codex.sh   # 一次性设置 Codex 原生标题，绑定本机终端会话
scripts/merge-settings.mjs # 把 hook 与 status line 配置幂等合并进 ~/.claude/settings.json（先备份）
app/
  ui/          manager（隐藏的逻辑中枢）+ pet / gauge（哑渲染器）+ sprites/skins/calls/drag
  src-tauri/   Rust：文件 watcher + 命令（快照 / 用量 / 窗口 / 聚焦 / 前台 tty / 叫声）+ 光标轮询（用量表悬停）
```

Claude 链路：**hook 在回合边界写状态文件 → Rust watcher 感知变化 → manager 裁决皮肤 / 已阅 / 该不该叫 → 每只 pet 窗口哑渲染**。账号用量走旁路：status line 脚本写同目录的 usage.json，宠物列顶上的用量表显示一份（悬停展开明细）。Codex 由客户端标题绑定 tty，再通过本机服务只读接口获取状态与账号用量，合并到同一 manager。输入与 app 之间的契约见 [docs/protocol.md](docs/protocol.md)，关键架构决策见 [docs/adr/](docs/adr/)。

宠物形象与叫声都是预渲染、可重跑的：改皮肤看 `app/ui/sprites.js`，改叫声重跑 `scripts/gen-calls.mjs`，顶部这两张图重跑 `scripts/gen-readme-art.sh`。

## License

[MIT](LICENSE) © 2026 y9g
