#pragma once
#include "lvgl.h"
static inline bool bsp_display_lock(int32_t timeout){(void)timeout;return true;}
static inline void bsp_display_unlock(void){}
lv_indev_t *bsp_display_get_input_dev(void);
