# Tauri 2 + 窗口即宠物

宠物 app 需要常驻 24 小时、置顶于全屏应用、动画用 Web 技术写（用户主力是 JS/TS）。选 Tauri 2 而非 Electron（常驻 ~15MB vs ~150MB）或 Swift+SpriteKit（用户非 Swift 主力，后续改动画门槛高）。

每只宠物是一个独立的精灵大小透明小窗，移动窗口本身即移动宠物——而不是一张全屏透明覆盖层内做像素级点击穿透。Tauri 的 `set_ignore_cursor_events` 只有窗口级粒度，全屏覆盖层方案在 Tauri 下无法做到"点宠物有效、点空白穿透"；小窗方案则完全绕开这个问题，代价是宠物游走需要移动 OS 窗口（低帧率可接受）。

## Considered Options

- Electron 全屏透明覆盖层：像素级穿透可行（`setIgnoreMouseEvents` + forward），但内存奢侈，被否。
- Swift + SpriteKit：工程上最优雅，但维护者语言不匹配，被否。
