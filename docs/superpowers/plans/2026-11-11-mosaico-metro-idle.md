# Mosaico Metro 与空闲熄屏 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** 将已确认 Metro 界面部署到 Mosaico，并实现全空闲 30 秒熄屏、触摸 30 秒临时亮屏和活动唤醒。

**Architecture:** 纯 C 状态机管理空闲/触摸期限；LVGL 界面独立判断当前授权快照是否有活动，控制屏幕刷新与面板 Display Off/On。USB、触摸、授权与叫声维持运行。界面沿用既有调度和资源，仅增加小字号子集字体。

**Tech Stack:** ESP-IDF 6.2-dev、锁定官方 BSP、LVGL 9.5.0、C、现有 Node 资源导出与 Python 原生预览。

**Spec:** `docs/superpowers/specs/2026-11-11-mosaico-metro-idle-design.md`；Issue #22。

## Global Constraints

- 空闲 30000 ms、最后触摸后 30000 ms；所有分页都参与活动判断；无会话为全空闲。
- 480×480、#000000、左右 16 px、栏间 12 px、269/167 px 分栏；顶/底文字至少内收 65 px。
- 当前色块 3000 ms 周期：2000 常亮、400 淡出、200 全隐、400 淡入；其余常亮。
- 维持精灵、基础尺度与原动作、彩色气泡、声音/授权/回退语义；#20 暂缓。
- 使用已批准的当前 Mosaico 功能分支与现有 SDK 构建目录；不动运行应用直到部署，不为此次续作复制硬件构建工作树。
- 只写真实 7 MiB 应用槽位、先备份、校验两侧保护区域；不执行两小时连续测试，不合并 PR。

## Review Focus

- 全空闲快照和用量每秒更新不能续亮；在任务 1/3 以固定时间和真实快照覆盖。
- 非当前分页的活动必须唤醒；在任务 3 注入六会话、末项 running。
- 熄屏仍接管且可触摸；任务 3 检查原生输入事件、刷新停止、UI 心跳失效与真实设备租约。
- 失联时触摸不得展示旧快照；任务 3 在熄屏时撤销 active，再触摸检查等待画面。
- 闪烁全隐不能移动其他行/遮字；任务 2 比较多个相位的真实像素，覆盖 R96 与中文长名称。

### Task 1: 空闲与触摸期限

**Files:** Create `firmware/mosaico/main/idle_screen.h`, `idle_screen.c`, `tests/idle_screen_test.c`; Modify `firmware/mosaico/tests/run.sh`。

**Interfaces:** `rg_idle_screen` 状态保存上次 busy 和亮屏期限；`rg_idle_screen_init(rg_idle_screen *, uint64_t now)`；`bool rg_idle_screen_update(rg_idle_screen *, uint64_t now, bool busy, bool touched)` 返回期望亮屏。时间单位毫秒，单调 64 位。

- [x] 写测试并观测 RED：初始化 29999 ms 亮/30000 ms 灭；重复 idle 不续期；busy 自动亮；busy→idle 从转换重新 30000 ms；触摸后 29999/30000 边界；连续触摸；零时刻、超过 32 位毫秒仍正确。
- [x] 实现上述纯 C 接口，仅在 busy→idle 或 touched 时重设期限。
- [x] `sh firmware/mosaico/tests/run.sh` 全部通过，提交 `feat: 增加 Mosaico 空闲亮屏期限状态机 #22`。

### Task 2: Metro 真正界面

**Files:** Modify `firmware/mosaico/main/ui.c`, `main/CMakeLists.txt`, `scripts/gen-mosaico-fonts.sh`, `scripts/gen-mosaico-assets.mjs`, `app/tests/mosaico-assets.test.js`, `firmware/mosaico/tests/preview/CMakeLists.txt`; Create `firmware/mosaico/tests/preview/metro.py`；更新 generated 字体/清单/版本；提交既有 Demo 与圆角研究作为依据。

**Interfaces:** 保持 `ui.h` 的现有初始化/应用快照/授权/健康接口；新增 16 px 固定文案子集，动态字段缺失小字形时回到 font20。LVGL 预览继续接受对象/序列快照并输出 PPM。

- [x] 写真实像素回归并观测 RED：纯黑背景、47%/22% 进度条有效长度、未知/过期虚线轨道、五行色块、焦点常亮/渐隐/全隐/恢复及其它行位置不变。
- [x] 增加 font16 子集与资源哈希；更新重建测试。保持 font20/28 原文件与精灵/声音资源相同。
- [x] 将用量、五行、顶底文案、名牌和气泡容器落实到 ui.c；使用现有调度/分页，不从 Demo 复制演示调度。背景与色板、缓动周期按 Spec。
- [x] 用真实 LVGL 检查 1/5/6 会话、三皮肤六状态、长中文、未知/过期、各闪烁相位，生成可查看图片；文字像素校核 R96 安全轮廓。必要的字体尺寸/位置微调记录到设计。
- [x] 运行全部 JS、设备核心、原生气泡/Metro 回归并构建固件，确认 0x700000 容量；提交 `feat: 实现 Mosaico 黑底进度条与 Metro 总览 #22`。

### Task 3: 屏幕电源、触摸与健康整合

**Files:** Create `firmware/mosaico/main/panel.h`, `panel.c`; Modify `main/ui.c`, `main/ui.h`, `main/main.c`, `main/CMakeLists.txt`; Modify native preview BSP stub/harness，增加 `tests/preview/idle_integration.c` 与 CMake target；更新 `docs/mosaico.md`。

**Interfaces:** `bool rg_panel_set_on(bool on)`：官方 panel Display Off/On，失败返回 false；原生测试提供记录调用/注入失败的替身。UI 从 `bsp_display_get_input_dev()` 注册输入事件；idle_screen 使用任务 1 接口。`rg_ui_healthy` 在亮屏检查真实刷新，在熄屏检查成功关闭状态及 UI tick 进展。

- [x] 在真实 LVGL harness 创建可控制的 pointer indev，先观测 RED：空闲期限到达关闭面板并停止 flush；触摸能亮 30 秒后再灭；同样快照和用量不续亮；第六会话非空闲能唤醒；等待连接触摸不显示旧内容。
- [x] 实现 panel 接口；开启 BSP touch 并验证设备存在；只暂停显示刷新计时器，保持输入与业务计时器。熄屏跳过动画/文字更新。唤醒更新最新画面、强制提交完整帧，然后打开面板。
- [x] 增加健康故障回归：故意熄屏健康持续，UI tick 停滞则失去健康；唤醒实际 flush 停滞不能报健康；面板失败拒绝接管/触发既有故障清屏路径。
- [x] 运行原生 UI/健康/电源回归、设备核心、JS、Rust 与 PTY 集成；构建固件及匹配桌面包、核对容量，提交 `feat: 支持 Mosaico 空闲熄屏和触摸唤醒 #22`。
- [x] 双轴 review 整个 #22 增量；问题以 RED→GREEN 修复。更新 PR #19、Issue #22 并记录评审，等待最新 CI 通过。
- [x] 使用既有安全安装程序的流程（读取并调整新镜像摘要/当前备份路径），确认下载模式后备份/写入/验证，不复用过期硬编码镜像摘要。配套更新常驻应用。
- [x] 真机检查新 UI、全空闲 30 秒、触摸 30 秒、重复触摸续期、非空闲自动唤醒、睡眠/断线回退；用户目视确认后恢复真实数据，并提交验收记录。
