# Mosaico 内存采样 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 每分钟采集设备内存，保留可比较的本机记录，供日常使用 2–4 小时后分析。

**Architecture:** 已有 lease_request 增加可选 memory 对象，不新增串口连接或控制消息。采样与 ESP-IDF 查询分开；Rust 验证当前连接样本后交给有界异步写盘线程。Python 只读分析日志，不控制设备。

**Tech Stack:** 现有 C/cJSON/ESP-IDF、Rust 标准库/serde_json、Python 标准库。

**Spec:** [Issue #24](https://github.com/yangzhyo/red-green/issues/24)，用户已确认上一轮聊天中的每 60 秒采样、2–4 小时日常观察方案。

## Global Constraints

- 60000 ms 采样周期；内部 RAM 与 PSRAM 分开记录 free/largest/minimum，附 uptime、boot、firmware、会话数、全空闲与屏幕开关。
- memory 是可选诊断字段，旧主机忽略；丢样或日志错误不能改变授权和回退。默认两份各 2 MiB JSONL，队列最多 8 个样本。
- 延续已批准的 Mosaico 功能分支与硬件构建目录；不重新复制工作树，不动界面资源或原 Idea。
- 原两小时日常使用验收已通过；本轮长时间内存趋势必须有采样后再判断，不自动合并。

## Review Focus

- Task 1：空闲熄屏仍采样，不因绑定重试/用量更新加快频率；跨 32 位时钟正确。
- Task 2：旧 boot/generation、倒退 uptime、重复/畸形样本不能混入当前记录，也不能破坏正常续期。
- Task 2：磁盘失败、队列满/写盘阻塞时仍能授权；文件轮转/恢复失败不无限增长。
- Task 3：断档、重启、样本不足及不同空闲会话数量不能被包装成持续稳定。
- Task 3：历史最低剩余下降代表曾有峰值，不直接判定泄漏；看预热后的可比状态趋势。

### Task 1: 固件可选采样

**Files:** Create `firmware/mosaico/main/memory_sample.h`, `memory_sample.c`, `tests/memory_sample_test.c`; Modify `main/main.c`, `ui.h`, `ui.c`, `main/CMakeLists.txt`, `tests/run.sh`。

**Interfaces:** `bool rg_memory_due(rg_memory_clock *, uint64_t now)`；`bool rg_memory_attach(cJSON *request, uint64_t uptime, const rg_heap_sample *internal, const rg_heap_sample *psram, const cJSON *snapshot, bool screen_on, const char *firmware)`；`bool rg_ui_screen_on(void)` 原子读取。

- [x] RED：0/59999 不采、60000 采、重复不采、晚到不追补、超过 32 位；编码正确，全空闲按全部会话判断；失败不删除基础心跳。
- [x] 实现并接入真实 ESP-IDF heap 查询；C ASan/UBSan 与现有测试通过。
- [x] 提交固件采样。

### Task 2: 验证与异步记录

**Files:** Create `app/src-tauri/src/output/memory.rs`; Modify `output/mod.rs`, `runtime.rs`; Extend `scripts/mosaico-simulator.py`。

**Interfaces:** `MemorySample` 类型、`Recorder` 有界后台线程；Engine 从已验证且接受的 lease_request 提取样本，提供 `take_memory()`，run 用非阻塞提交写盘；默认 `~/Library/Logs/red-green-mosaico-memory.jsonl`，测试可覆盖路径。

- [x] RED：真实 Engine 正常续期并拿到样本；旧代/畸形/重复不记录；缺字段旧固件继续工作；队列满无阻塞，文件轮转正确且失败可恢复。
- [x] 实现 typed validation、时间戳与 boot 关联、异步日志；每次写盘错误只报告诊断，不影响输出。
- [x] Rust 全套与 PTY 集成通过，提交。

### Task 3: 趋势报告、构建与交付

**Files:** Create `scripts/mosaico-memory-report.py`, `scripts/tests/test_mosaico_memory_report.py`; Modify `docs/mosaico.md`, `.github/workflows/ci.yml`, `CONTEXT.md`。

**Interfaces:** `python3 scripts/mosaico-memory-report.py [JSONL ...]` 输出按 boot/firmware/空闲状态与会话数分组的时长、样本数、缺口和首尾窗口中位数差，不自动宣称零泄漏。

- [x] RED→GREEN：稳定、持续下降、短记录、重启/断档、只有低水位下降、畸形行及重复样本；接入 CI。
- [x] 更新原日常验收记录与操作说明；全部测试、固件容量与桌面构建通过。
- [x] 按 code-review 双轴审查和最终整体审查，重要问题测试先行修正；PR #19 关联 #24，CI 通过。
- [ ] 备份后更新常驻应用与设备（硬件下载模式需用户操作），验证真实内存样本与现有显示/音频健康；恢复真实会话。
- [ ] 记录采样已启用及长时间趋势尚待数据，更新本地 Idea 时保留旧副本；不提前宣称 2–4 小时内存测试通过。
