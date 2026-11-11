#include "panel.h"
#include "bsp/display.h"
#include "esp_lcd_panel_ops.h"

bool rg_panel_set_on(bool on) {
  esp_lcd_panel_handle_t panel = bsp_display_get_panel();
  return panel && esp_lcd_panel_disp_on_off(panel, on) == ESP_OK;
}
