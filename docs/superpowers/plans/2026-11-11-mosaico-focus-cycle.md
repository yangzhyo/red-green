# Mosaico 主角与闪烁同步 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 每只主角完整闪烁两次后切换，轮播与色块使用同一个时钟。

**Architecture:** schedule 拥有唯一轮次起点和 3000×2 ms 常量。UI 直接使用该时间计算透明度，总览页跟随主角。内存采样 #24 已联合部署；后续实机反馈要求将周期内顺序改为先闪再停留，修正后重刷应用。

**Tech Stack:** 现有 C、LVGL、原生预览与 Python 像素回归。

**Spec:** [Issue #25](https://github.com/yangzhyo/red-green/issues/25)，用户确认 2 次／6 秒。

## Global Constraints

- 周期 3000 ms（按用户后续实机反馈调整顺序）：400 淡出、200 全隐、400 淡入、2000 常亮；两周期后才换主角。
- 状态/叫声即时，优先选择只在轮次边界执行；保留每四轮一次全体公平选择。有多只时不连续重复当前主角。
- 当前会话删除或授权失效立即清理，普通快照不重启周期。用量独立 8 秒规则不变。
- 沿用当前 Mosaico 分支和构建目录。部署前必须替换此前 #24 固件安装脚本的旧摘要和 CI 检查，保留原 Idea 和恢复备份。

## Review Focus

- 1 秒抢占和重复快照不能切断第一个/第二个周期：Task 1 边界测试。
- 同一高优先级不能占据连续两轮；普通会话不饥饿：Task 1 长时间调度测试。
- 单会话、删除当前、删除后重建、64 位时间：Task 1。
- 主角与色块在同一帧切换，普通更新不改 phase：Task 2 真正 LVGL 像素检查。
- 六会话时当前色块必须可见，前一页不能残留：Task 2。

### Task 1: 单一轮次调度

**Files:** `firmware/mosaico/main/schedule.h`, `schedule.c`, `tests/schedule_test.c`。
**Interfaces:** `RG_FOCUS_BLINK_MS=3000`，`RG_FOCUS_BLINK_COUNT=2`，`RG_FOCUS_ROUND_MS`；`schedule.since` 是唯一轮次起点。已有 update/tick/focus 接口不变。

- [x] 改测试并观察 RED：5999 ms 不换、6000 换；高优先级到达也等6000；重复更新不延期；多会话不连选、全部可见；单会话完成周期继续；删除当前立即重选。
- [x] 删除中途抢占计时，将优先/公平选择固定在轮次边界；选择下一项跳过当前项，更新仍保留当前 ID。
- [x] C sanitizer 全套通过并提交。

### Task 2: UI 对齐、验证及合并部署

**Files:** `firmware/mosaico/main/ui.c`, `tests/preview/metro.py`, `docs/mosaico.md`；同步更新原设计中已被新要求取代的时长。
**Interfaces:** 透明度使用 `(now-schedule.since)%RG_FOCUS_BLINK_MS`，总览页 `schedule.focus/5`；移除独立 focus_since。

- [x] 首版真正 LVGL 回归（周期内顺序由 Task 3 替代）：2500/5500同一块消失，6000新块从常亮开始，8500新块消失；普通快照和新优先状态不提前切换；第六会话焦点可见。
- [x] 全部现有预览/电源/气泡与协议测试通过；固件构建与容量通过。
- [x] code-review 双轴审查，修正重要问题，PR #19 关联 #25，CI 通过。
- [x] 与 #24 一起安装：仅应用槽位、写前备份及写后保护区校验；真实亮屏/熄屏内存样本及常驻恢复通过。新旧本地 Idea 版本边界已写入说明。
- [x] 用户确认真机两次闪烁／六秒切换的实际节奏。

### Task 3: 按实机反馈改为先闪烁再停留

**Files:** `main/ui.c`、`tests/preview/metro.py`、相关时序说明。
**Interfaces:** 保留 schedule.since、3000 ms 周期、两次／6000 ms 轮次；只调整周期内部的相位顺序。

- [x] RED→GREEN：首轮 0.24 秒已淡出、0.4/0.5 秒全隐、0.8 秒淡入、1–3 秒常亮；3.5 秒第二次隐藏、5.5 秒仍为原主角常亮、6 秒换主角、6.5 秒新块隐藏；频繁快照和优先状态仍不打断。
- [x] 原生 LVGL Metro/气泡/电源与 C sanitizer 回归；固件容量、双轴审查与 CI。
- [x] 仅重刷应用，备份当前 e77466b 并校验保护区域；保留持续内存记录与本地 Idea。真机确认先闪再停留，旧固件内存与新启动分组处理。
