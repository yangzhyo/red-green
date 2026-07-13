# 叫声用预渲染资产 + afplay，而非运行时 Web Audio 合成

叫声是 3 皮肤 × 4 状态的全矩阵（音色传身份、节奏传状态），由 `scripts/gen-sounds.mjs` 用固定种子离线合成为 12 个 WAV，随 bundle resources 打包，运行时经已验证的 afplay 通路播放。不用 Web Audio 运行时合成，因为发声决策在 manager——一个隐藏窗口，而 WebKit 会挂起不可见 webview 的定时器（已为此把已阅心跳移到 Rust 侧），AudioContext 在同一环境下同样面临被挂起/要求用户手势的风险。代价是调音色要重跑生成脚本，换来的是声音设计以代码形式可审阅、可版本化。

## Considered Options

- Web Audio 实时合成：零资产文件、参数即时可调，但落在隐藏窗口的已知雷区上，被否。
- 现成音效素材：凑不齐"三物种共享同一节奏语法"的成套一家人，被否。
