#pragma once
#include "lvgl.h"
extern bool test_panel_on, test_panel_fails, test_pressed;
extern bool test_draw_fails;
extern unsigned test_panel_calls;
void test_input_create(void);
