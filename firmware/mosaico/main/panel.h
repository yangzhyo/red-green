#pragma once
#include <stdbool.h>
// 在 LVGL 任务/显示锁内调用；不暂停输入任务，也不进入控制器 Sleep In。
bool rg_panel_set_on(bool on);
