#include "native_panel.h"
static lv_indev_t *input;
bool test_panel_on = true, test_panel_fails, test_pressed;
unsigned test_panel_calls;
bool rg_panel_set_on(bool on) {
  test_panel_calls++;
  if (test_panel_fails)
    return false;
  test_panel_on = on;
  return true;
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
