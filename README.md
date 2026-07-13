# red-green

Claude Code 桌面宠物。每个本机 Claude Code 会话对应一只置顶于所有窗口的像素宠物，用行为动画告诉你它在干嘛：敲键盘 = 运行中（气泡里滚动显示 spinner 动词），举牌跳 = 待确认（卡住了，急！），探头张望 = 轮到你（提问等答复），比耶 = 已完成，躺平 = 异常中止，睡觉 = 空闲。点击宠物直接聚焦对应的 Terminal 标签页。需要拉回注意力的状态还有叫声：音色随皮肤（听声辨项目），节奏随状态（听节奏辨事件，节奏语法见 [CONTEXT.md](CONTEXT.md)「叫声」）；你正看着的会话不出声（`scripts/gen-calls.mjs` 生成，改音色重跑即可）。

领域词汇见 [CONTEXT.md](CONTEXT.md)，hooks 与 app 之间的契约见 [docs/protocol.md](docs/protocol.md)，架构决策见 [docs/adr/](docs/adr/)。

## 结构

```
hooks/session-status.sh   # Claude Code hook：把会话状态写进 ~/.claude/session-status/
scripts/merge-hooks.mjs   # 把 hook 配置合并进 ~/.claude/settings.json（幂等，先备份）
install.sh                # 安装以上两者
app/                      # Tauri 2 宠物 app（前端零依赖，withGlobalTauri）
  ui/                     # manager（隐藏逻辑中枢）+ pet（哑渲染器）
  src-tauri/              # Rust：文件 watcher + 6 个命令（快照/窗口×2/聚焦/前台 tty/叫声）
```

## 安装与运行

```sh
./install.sh                 # 1. 装全局 hooks（对新启动的 Claude Code 会话生效）
cd app && pnpm install       # 2. 前端依赖（仅 @tauri-apps/cli）
../scripts/install-app.sh    # 3. 构建 .app 装进 ~/Applications 并注册登录自启
```

开发迭代时用 `pnpm tauri dev` 跑开发版（先退掉常驻版：`launchctl unload ~/Library/LaunchAgents/dev.y9g.red-green.plist`）。

首次运行时 macOS 会请求「自动化」权限（控制 Terminal 和 System Events）——点击聚焦和已阅检测需要它；.app 是独立应用身份，会再各弹一次。

## TODO

- [x] 宠物形象：三套像素皮肤（灯灯🚦/钳钳🦀/灰灰🐱）按项目名哈希分配，见 `app/ui/sprites.js`
- [ ] 沿屏幕底边游走（移动窗口，低帧率）
- [ ] 静音开关（菜单栏托盘）+ bundle 图标
- [x] `tauri build` + 登录自启动（scripts/install-app.sh，LaunchAgent 崩溃自愈）
- [ ] 已阅状态持久化（app 重启后丢失）
- [ ] 拖动后的宠物位置持久化（目前重建窗口即回默认排位）
- [ ] 验证：AskUserQuestion 是否触发 Notification hook（若不触发，弹窗提问期间宠物会停在黄色）
- [ ] 观察 your_turn 问句启发式的误判率，必要时调整词表（hooks/session-status.sh）
- [ ] 观察语义缝隙：主回合 Stop 了但后台 agent/任务还在跑的会话，宠物会显示绿/蓝而非黄——后台活动是否触发主会话 hooks 待验证
