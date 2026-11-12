#pragma once
#include "cJSON.h"
#include <stdbool.h>
#include <stdint.h>
bool rg_ui_init(void);
bool rg_ui_apply(const cJSON *snapshot);
bool rg_ui_active(bool active);
bool rg_ui_healthy(uint32_t now);
uint64_t rg_now(void);
bool rg_ui_screen_on(void);
