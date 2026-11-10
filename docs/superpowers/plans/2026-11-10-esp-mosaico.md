# ESP-Mosaico 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 完成 #18：通过 USB 将宠物、用量表、叫声独占输出到 Mosaico，失效后安全回退桌面。

**Architecture:** manager 继续裁决状态、已阅与前台静默，只提交完整输出快照和当次叫声事件。Rust 输出模块独占管理桌面窗口、音频进程、串口与有限时授权。独立 ESP-IDF 固件负责本地 LVGL 渲染、预置叫声和授权看门狗。

**Tech Stack:** Tauri 2 / Rust、现有原生 JS、ESP-IDF master（固定提交）、ESP-Mosaico BSP 716fc7bca30ed9c61166a0343bf6f5f995b5131f、LVGL 9.5、TinyUSB CDC。

**Spec:** `docs/superpowers/specs/2026-11-10-esp-mosaico-design.md`

## Global Constraints

- CoreBoard V1.2.1，USB-C，仅一台 Mac 对一台设备，不增加触摸/按键业务操作。
- 主角＋总览固定 61.8/38.2；1–5 个会话总览同屏，超五分页，不静默丢会话。
- 设备请求每 500 ms 一次；请求创建起最多 2 s 授权；主机最后可能发送授权后至少 2.5 s 才恢复桌面。
- 主机启动也等待 2.5 s；manager 超过 3 s 不更新则不续期；接管前须隐藏成功、停止本地音频。
- 只在新状态转移发声；事件不跨输出端重投，不重连重播；待播放超过 2 s 丢弃。
- USB 不传路径、tty、正文或凭据；有限长度帧及原子快照；超过上限保持桌面，不截断后接管。
- 无线、自动升级、全片擦除均不在此次范围；先只读识别与备份当前设备，再决定可恢复安装路径。
- 全部实现留在本地 Issue #18 分支；原有常驻进程只在端到端验证前停止，验证后恢复。

## Review Focus

- 隐藏窗口失败、部分窗口新建失败：不得授权设备，回退按最新快照重建；Task 3 验证。
- 延迟授权、单向丢包、主机重启：设备不得凭接收时刻延长授权；Tasks 1、4、6 验证。
- 名称含中文、同名不同来源、同名并行会话：身份隔离、无方框和未转义协议字符；Tasks 2、5 验证。
- 用量长期不变、窗口重置、账号切换：Claude 不套 Codex 的 120 s 规则，未知不是零；Task 2 验证。
- 一个会话持续待确认：其他会话仍定期出现，声音和已阅不随轮播改变；Task 5 验证。

### Task 1: 有界协议与独占授权状态机

**Files:** 新建 `app/src-tauri/src/output/{mod.rs,protocol.rs,coordinator.rs}`；修改 `app/src-tauri/src/lib.rs`。
**Interfaces:** `FrameDecoder::push(&[u8]) -> Result<Vec<Value>, String>`、`encode(&Value) -> Result<Vec<u8>, String>`；`Coordinator` 接收单调毫秒、设备就绪/健康请求、桌面停止结果，产出接管/授权/回退决定。资源版本由 Task 5 产物提供。

- [ ] 先写公开边界测试：分段/粘包、坏魔数/超长帧、版本不符不接管、两次间隔 500 ms 健康回应、隐藏失败不授权、断连后 2499 ms 不回退/2500 ms 回退、超过 3 s 无状态更新不续期、旧 nonce 不重复授权。
- [ ] 执行 `cargo test --manifest-path app/src-tauri/Cargo.toml --lib output`，确认新增行为尚未实现。
- [ ] 实现 `RGP1 + u32 LE 长度 + UTF-8 JSON`，帧上限 65536 bytes；boot/generation/nonce 绑定，授权不排队重试。
- [ ] 同命令通过，提交 `feat: 增加 Mosaico 有界协议与输出授权状态机`。

### Task 2: manager 完整快照与设备用量投影

**Files:** 修改 `app/ui/{manager.js,usage-model.js}`、`app/tests/{manager.test.js,usage-model.test.js}`；新建 `app/src-tauri/src/output/model.rs`。
**Interfaces:** `submit_output({snapshot:{pets:[{sid,slot,model}], desktop_usage, device_usage}, calls:[{sid,name}]})`；Rust 的 `OutputSnapshot` 留存桌面原模型，设备投影只允许 opaque ID、项目名、来源、皮肤、状态、顺序、用量显示字段。

- [ ] 先添加首见静默、前台已阅、状态转移只提交一次叫声、删除后完整快照不含旧会话、敏感字段不进入 wire、Claude 陈旧但未重置仍有效/无重置未知的行为测试。
- [ ] 执行 `node --test app/tests/*.test.js` 和 Rust output 测试，确认新契约失败。
- [ ] manager 改为单个串行提交路径；设备用量投影单独增加，保留桌面现有规则；Rust 严格验证枚举、字段长度、帧容量，拒绝不支持快照。
- [ ] 两套测试通过，提交 `feat: 统一宠物输出快照并投影设备用量`。

### Task 3: 桌面输出适配、串口发现与持续循环

**Files:** 新建 `app/src-tauri/src/output/{runtime.rs,transport.rs}` 和 `app/src-tauri/src/desktop_output.rs`；修改 `main.rs`、`Cargo.toml`。
**Interfaces:** Rust `OutputRuntime` 消费 Task 2 快照，调用 Task 1 coordinator；桌面适配 `reconcile / hide / stop_audio / play`；串口只对匹配设备应用握手的 CDC 接口激活。

- [ ] 先写真实 PTY/受控适配测试：未授权显示桌面，隐藏失败不 grant，设备模式新增会话不闪窗，断连回退不复活已删除会话，旧叫声不重播。
- [ ] 执行 Rust output 与 manager 测试确认缺失行为。
- [ ] 添加有限超时串口 I/O、完整帧立即写入、失败断连、准确保留/停止 afplay 子进程；Rust 驱动 50 ms 循环，不使用隐藏 WebKit 定时器续期。
- [ ] `cargo check` 与 Rust/JS 全套通过；提交 `feat: 接入 Mosaico 串口与桌面自动回退`。

### Task 4: 设备工程、CDC 与授权看门狗

**Files:** 新建 `firmware/mosaico/{CMakeLists.txt,sdkconfig.defaults,main/*,tests/*}`、`scripts/mosaico-env.sh`、`docs/mosaico.md`。
**Interfaces:** 与 Task 1 共用协议向量；设备 boot ID、绑定 generation、单调 nonce；snapshot 应用后 Applied，UI/音频健康后 request，grant 仅接受未过期请求。

- [ ] 先在主机 C 测试中模拟请求创建后 2001 ms 才到的 grant、旧 generation、重复 call、未知版本、截断/超长 JSON；断开必须到期清屏停声。
- [ ] 执行主机 C 测试，确认未实现；补核心解析与授权逻辑。
- [ ] 固定 SDK/BSP/USB 依赖；实现只读板型检查、CDC 业务/日志隔离、UI/音频健康计数、2 s 到期高优先级处理与任务看门狗。
- [ ] 主机 C 测试与 `idf.py --preview set-target esp32s31` / `idf.py build` 通过；提交 `feat: 建立 Mosaico 固件与有限时输出授权`。

### Task 5: 资源、黄金比例布局、轮播与声音

**Files:** 新建 `scripts/gen-mosaico-assets.mjs`、`firmware/mosaico/main/{ui.c,audio.c,assets.h}` 及生成资源；增加资源契约测试。
**Interfaces:** 精灵帧/动作与 12 个 WAV 来自现有资产，资源哈希参与 Ready；UI 消费 Task 4 的原子快照，音频消费单次 Call。

- [ ] 先验证导出三皮肤六状态所有帧、12 段音频、同项目身份、中文名称、轮播公平性和 2 s 队列过期。
- [ ] 导出可重跑资源；LVGL 主角＋总览 269/167 px，固定总览字号、滚动长名、来源与用量明细；主角 4 s、总览/用量 8 s。每三次优先主角轮播插入一次全体稳定顺序轮播，避免长期饥饿；新事件最短 1 s 后抢占。
- [ ] 串行声音队列按状态优先级、同级事件顺序；授权失效立即清空并停声；不播放重连旧事件。
- [ ] 资源/调度测试、主机 C 测试与固件编译通过；提交 `feat: 在 Mosaico 呈现黄金比例宠物与用量`。

### Task 6: 联调、部署与交付

**Files:** 新建 `scripts/mosaico-simulator.py`、补充 `.github/workflows/ci.yml`、`docs/mosaico.md`、`README.md`。
**Interfaces:** PTY 模拟器与真实固件遵守同一帧、授权、快照及资源版本；只读识别/备份工具不改 eFuse 或分区表。

- [ ] 用 PTY 跑集成场景：首次接管、正常变化、单向丢包、渲染停滞、延迟 grant、重复 Call、重启，留存事件与时序证据。
- [ ] 按 `.claude/skills/verify/SKILL.md` 使用隔离假会话目录验证桌面回退，避免污染真实状态。
- [ ] 设备可用时先识别板型和分区、保存恢复镜像，再安装构建产物；覆盖真实中英文项目名、叫声、拔插、睡眠唤醒及两小时持续运行。未验证项明确留在 Issue，不标记完成。
- [ ] 运行 CI 全部命令并做独立分支审查，修复重要问题，更新部署说明与已知限制。
- [ ] 创建包含 `Closes #18` 的 PR，附双轴审查小结；仅在用户说「合」后合并。硬件验收未完成时保留 Draft，不关闭 Issue。
