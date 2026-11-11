# Mosaico 主角同步淡入淡出实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** 将用户验收的同步 Demo 实现到固件并安全更新设备。

**Architecture:** 调度仍每 6 秒选择下一会话；独立、无堆分配的主角过渡状态按 ID 保存显示对象，在新轮次 500 ms 时换主角。宠物、气泡与当前色块调用同一个 500 ms 淡出／500 ms 淡入透明度函数，只有第一秒影响宠物。

**Tech Stack:** C11、LVGL 9、ESP-IDF、原生 LVGL 像素验证、Python 安全刷写脚本。

**Spec:** [Issue #26](https://github.com/yangzhyo/red-green/issues/26)；[用户已验收的 Demo 决定](../../../app/prototypes/mosaico/NOTES.md)。用户已要求实现和刷入；沿用当前分支、构建环境及本地实现方式，不重新请求实施许可。

## Global Constraints

- 每轮 6000 ms，两次 3000 ms 色块周期：500 ms 淡出、500 ms 淡入、2000 ms 常亮。
- 第一秒旧宠物与气泡一起退场，500 ms 时更换宠物和文字；第二次仅色块闪烁。单只宠物不循环退场。
- 快照/状态更新不重启过渡，按 ID 处理重排；删除、空列表、授权失效立即清理旧内容。宠物既有动作、声音、用量、30 秒熄屏与采样不变。
- 只写 main_app，写前核对设备、当前固件、分区和 CI，保存恢复副本，写后校验应用和保护区域；保留现有本地 Idea，不自动合并。

## Review Focus

- 退场与入场期间每 200 ms 快照或重排：Task 1 测试 ID 与时间不重置。
- 退场对象或待入场对象被删除：Task 1 删除测试与 Task 2 真实 UI 快照。
- 单会话多个轮次、断连同 ID 重连：Task 1 reset 测试与 Task 2 电源/授权测试。
- 六会话跨页，色块先切换而名称仍对应旧宠物：Task 2 像素/标签检查。
- 跨 32 位时间、跳帧越过 1 秒、睡眠唤醒：Task 1 大时间和跳帧；Task 2 现有 idle 集成。

### Task 1: 有界主角过渡与透明度

**Files:** 新建 `firmware/mosaico/main/hero.h`、`hero.c`、`tests/hero_test.c`；修改 `tests/run.sh`。
**Interfaces:** `rg_hero_reset(rg_hero *)` 清除旧身份；`rg_hero_update(rg_hero *, const rg_schedule *, uint64_t now)` 返回 `rg_hero_frame { const char *id; uint8_t opacity; }`；`rg_focus_opacity(uint64_t elapsed)` 返回共享色块透明度。状态只保存固定大小 ID 和时间，不持有 JSON 指针。

- [x] 写测试：0/250/500/750/1000 ms 为 255/127/0/127/255；6000 开始退场，6500 换 ID，7000 全亮，9500 宠物全亮；快照重排、删除、单会话、reset、空列表、跨 32 位与跳帧。执行 `sh firmware/mosaico/tests/run.sh` 观察缺失实现失败。
- [x] 实现以上接口，沿用 `schedule.since`，只有实际目标 ID 改变时建立过渡；退场 ID 缺失立即放弃它。
- [x] 同命令 sanitizer 全绿，提交核心实现。

### Task 2: LVGL 接入、验证和部署

**Files:** `main/ui.c`、两处 `CMakeLists.txt`、`tests/preview/metro.py`、`docs/mosaico.md` 与已验收原型说明。
**Interfaces:** 消费 Task 1 三个接口；主角根据返回 ID 从最新快照查找，标签跟随显示 ID，总览跟随调度目标，hero_image 与 balloon 设置同一个 opacity；失活/空列表 reset。

- [x] 先改真正 LVGL 回归并看失败：6040 ms 旧主角仍显示，6500 ms 宠物/气泡/色块隐藏，6540 ms 新主角标签；7000 ms 恢复；9500 ms 仅色块隐藏。加入快照重排、删除、单只、跨页断言。
- [x] 接入 UI 并完成 Metro、气泡 18 场景、idle 8 场景、核心 sanitizer；ESP-IDF reconfigure/build 并检查容量、资源摘要；提交。
- [x] 按仓库要求执行 code-review 双轴审查（既有 PR 其他功能已审，本次范围从 2ee8b57 起）；修复重要发现并验证；推送 PR #19、关联 #26、CI 通过。
- [ ] 准备本次独立安装脚本，等待设备下载模式；核对旧 6d0265e 摘要、写前备份、仅写应用，写后保护区域不变；真实连接恢复和新内存样本，记录用户视觉验收。
