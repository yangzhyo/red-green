#include "panel.h"
#include "bsp/display.h"
#include "esp_lcd_panel_ops.h"
#include "esp_lcd_panel_interface.h"
#include <stdatomic.h>

static esp_lcd_panel_handle_t observed;
static esp_err_t (*original_draw)(esp_lcd_panel_t *, int, int, int, int, const void *);
static atomic_bool watching, failed;

static esp_err_t draw(esp_lcd_panel_t *panel, int x1, int y1, int x2, int y2,
                      const void *pixels) {
  esp_err_t result = original_draw(panel, x1, y1, x2, y2, pixels);
  // adapter 在错误后仍会结束 LVGL flush，必须保留底层结果。
  if (result != ESP_OK)
    atomic_store(&failed, true);
  return result;
}

bool rg_panel_init(void) {
  esp_lcd_panel_handle_t panel = bsp_display_get_panel();
  if (observed)
    return panel == observed && rg_panel_healthy();
  if (!panel || !panel->draw_bitmap)
    return false;
  observed = panel;
  original_draw = panel->draw_bitmap;
  panel->draw_bitmap = draw;
  atomic_store(&watching, true);
  return true;
}

bool rg_panel_healthy(void) {
  return atomic_load(&watching) && !atomic_load(&failed);
}

bool rg_panel_set_on(bool on) {
  esp_lcd_panel_handle_t panel = bsp_display_get_panel();
  return panel && esp_lcd_panel_disp_on_off(panel, on) == ESP_OK;
}
