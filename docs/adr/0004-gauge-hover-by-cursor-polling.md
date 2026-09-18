# 用量表的悬停靠 Rust 轮询光标位置，而非页面里的 mouseenter

用量表是一个从不获得焦点的透明置顶小窗。macOS 上非焦点窗口收不到 WebKit 的 mousemove，页面里的 `mouseenter` / `mouseleave` 实测不触发（光标已在圆表上，页面毫无反应）。于是悬停判定放在 Rust 侧：一个线程每 120ms 读一次全局光标位置，与用量表窗口的位置、尺寸比较——两者先各自按所在显示器的缩放化为逻辑坐标（光标按主显示器缩放、窗口按自己所在显示器缩放，拖到缩放不同的外接屏上基准会不同）——进入窗口就推送窗口内坐标，页面用 `elementFromPoint` 判定是否真落在圆表上；离开推一次 null。展开后只要光标还在窗口内就保持，避免圆表与卡片形状不同导致反复开合。用量表不存在时线程什么都不做。

同一个原因（隐藏或非焦点 webview 拿不到宿主事件）此前已经把已阅心跳移到了 Rust 侧（见 ADR 0002 的背景），这是同一条原则的第二次应用。

## Considered Options

- 页面 `mouseenter`：最自然，但非焦点窗口收不到，被否。
- 点击切换展开：能用，但用户选了悬停。
- 通过私有 API 给 NSWindow 设 `acceptsMouseMovedEvents`：要引入 objc 依赖，且 WKWebView 的 tracking area 是否随之生效没有把握，未验证，被否。
