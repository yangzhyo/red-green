---
name: verify
description: 驱动 red-green 桌面宠物 app 做端到端验证——注入假会话状态文件、观察叫声(afplay 进程参数)与宠物窗口渲染。
---

# 验证 red-green 的改动

## 启动被测实例

```bash
cd app && pnpm tauri dev        # 后台运行;增量构建约 3s,进程名 target/debug/red-green
```

**坑:已安装实例常驻。** `~/Applications/red-green.app` 由 LaunchAgent
`~/Library/LaunchAgents/dev.y9g.red-green.plist` 保活,直接 kill 会被秒拉起,
且它会与 dev 实例同时响应状态文件(双份叫声/窗口)。测试期间先卸载,测完恢复:

```bash
launchctl unload ~/Library/LaunchAgents/dev.y9g.red-green.plist   # 测前
launchctl load   ~/Library/LaunchAgents/dev.y9g.red-green.plist   # 测后
```

## 驱动:注入假会话

app 监听 `~/.claude/session-status/*.json`(hooks 契约见 docs/protocol.md),直接写文件即可驱动:

```json
{"session_id":"vfy-test-0001","state":"awaiting","cwd":"/tmp/x","project":"delta",
 "tty":"/dev/ttys099","detail":"","since":"<ISO时间>","event":"Notification"}
```

- `tty` 用不存在的号(如 ttys099)避开前台静默;`since` 变化会重置已阅。
- 皮肤按 `project` 哈希:`delta`→robot、`beta`→crab、`alpha`→cat(用
  `node -e 'await import("./app/ui/skins.js");console.log(SKINS.pick("名字"))' --input-type=module` 现算)。
- 状态转移 = 改 `state` 重写文件;删文件 = 宠物离场。文件监听秒级生效,留 2s 余量。

## 观察

**叫声**:afplay 是短命进程,轮询抓参数(路径里的 `皮肤-状态.wav` 就是证据):

```bash
while true; do ps -eo ppid,args | awk '$2=="afplay"'; sleep 0.04; done >> /tmp/afplay.log
```

ppid 区分实例(dev = target/debug 的进程号)。dev 实例的资源解析到
`app/src-tauri/target/debug/calls/`。

**渲染**:宠物窗口置顶,`screencapture -x out.png` 全屏截图即可看到精灵、项目名标签与状态特效。
窗口计数走 osascript 需要辅助功能权限(会弹系统授权框,别用)。

**生成脚本**:`node scripts/gen-calls.mjs <临时目录>` 后与 `app/src-tauri/calls/` 逐字节
`cmp`——固定种子,等价即逐字节相同。

## 值得驱动的流

- 转移进叫声状态(awaiting/aborted/your_turn/completed)→ 响对应 `皮肤-状态.wav`
- 转移进 running/idle → 无声;同状态重写(仅 `since` 变)→ 不重复叫
- app 启动前状态文件已是叫声状态 → 静默采纳,不补叫(首见不算转移)
- 未知 state → 不叫、不崩、精灵回退 robot idle
- 删状态文件 → 宠物离场
