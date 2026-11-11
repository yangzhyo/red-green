#include "native_panel.h"
#include "esp_lcd_panel_ops.h"
static lv_indev_t *input;
bool test_panel_on = true, test_panel_fails, test_pressed;
bool test_draw_fails;
unsigned test_panel_calls;
esp_err_t esp_lcd_panel_disp_on_off(esp_lcd_panel_handle_t panel, bool on) {
  (void)panel;
  test_panel_calls++;
  if (test_panel_fails)
    return ESP_FAIL;
  test_panel_on = on;
  return ESP_OK;
}
static esp_err_t draw(esp_lcd_panel_t *panel, int x1, int y1, int x2, int y2, const void *data) {
  (void)panel; (void)x1; (void)y1; (void)x2; (void)y2; (void)data;
  return test_draw_fails ? ESP_FAIL : ESP_OK;
}
static esp_lcd_panel_t panel = {.draw_bitmap = draw};
esp_lcd_panel_handle_t bsp_display_get_panel(void) { return &panel; }
esp_err_t esp_lcd_panel_draw_bitmap(esp_lcd_panel_handle_t p, int x1, int y1,
                                   int x2, int y2, const void *data) {
  return p->draw_bitmap(p, x1, y1, x2, y2, data);
}
lv_indev_t *bsp_display_get_input_dev(void) { return input; }
static void read_touch(lv_indev_t *device, lv_indev_data_t *data) {
  (void)device;
  data->point = (lv_point_t){100, 250};
  data->state = test_pressed ? LV_INDEV_STATE_PRESSED : LV_INDEV_STATE_RELEASED;
}
void test_input_create(void) {
  input = lv_indev_create();
  lv_indev_set_type(input, LV_INDEV_TYPE_POINTER);
  lv_indev_set_read_cb(input, read_touch);
}
