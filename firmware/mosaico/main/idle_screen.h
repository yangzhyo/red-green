#pragma once
#include <stdbool.h>
#include <stdint.h>

typedef struct {
  uint64_t deadline;
  bool busy;
} rg_idle_screen;

void rg_idle_screen_init(rg_idle_screen *screen, uint64_t now);
bool rg_idle_screen_update(rg_idle_screen *screen, uint64_t now, bool busy,
                           bool touched);
