#pragma once
#include <stdbool.h>
#include <stdint.h>
// 跨任务读取时 progress 可能比调用者采样的 now 晚一个 tick；也覆盖 32 位回绕。
static inline bool rg_recent(uint32_t now, uint32_t progress) {
  return progress && (int32_t)(now - progress) < 500;
}
