#pragma once
#include "esp_lcd_panel_interface.h"
esp_err_t esp_lcd_panel_disp_on_off(esp_lcd_panel_handle_t panel, bool on);
esp_err_t esp_lcd_panel_draw_bitmap(esp_lcd_panel_handle_t panel, int x1, int y1,
                                   int x2, int y2, const void *data);
