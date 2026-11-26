#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
enum {
  RG_FOCUS_BLINK_MS = 3000,
  RG_FOCUS_BLINK_COUNT = 2,
  RG_FOCUS_ROUND_MS = RG_FOCUS_BLINK_MS * RG_FOCUS_BLINK_COUNT
};
typedef struct {
  char id[33];
  int priority;
} rg_session;
typedef struct {
  rg_session sessions[32];
  size_t count, focus, normal_cursor, priority_cursor, turn;
  uint64_t since;
} rg_schedule;
void rg_schedule_update(rg_schedule *s, const rg_session *items, size_t n,
                        uint64_t now);
bool rg_schedule_tick(rg_schedule *s, uint64_t now);
const char *rg_schedule_focus(const rg_schedule *s);
