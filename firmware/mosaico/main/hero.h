#pragma once
#include "schedule.h"

typedef struct {
  char current[33], target[33];
  uint64_t since;
  bool transitioning, entering;
} rg_hero;
typedef struct {
  const char *id; // 指向 rg_hero 内部，调用方不持久保存。
  uint8_t opacity;
} rg_hero_frame;
void rg_hero_reset(rg_hero *h);
rg_hero_frame rg_hero_update(rg_hero *h, const rg_schedule *s, uint64_t now);
uint8_t rg_focus_opacity(uint64_t elapsed);
