#include "lvgl.h"
#include "ui.h"
#include "native_panel.h"
#include "bsp/display.h"
#include "esp_lcd_panel_ops.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>

static uint64_t now;
static uint32_t buffer[480 * 48], pixels[480 * 480];
static unsigned flushes;
uint64_t rg_now(void) { return now; }
static void flush(lv_display_t *d, const lv_area_t *a, uint8_t *data) {
  if (esp_lcd_panel_draw_bitmap(bsp_display_get_panel(), a->x1, a->y1,
                                 a->x2 + 1, a->y2 + 1, data) != ESP_OK) {
    // 固定版 adapter 在写屏错误后仍释放 flush，LVGL 仍会产生完成事件。
    lv_display_flush_ready(d);
    return;
  }
  flushes++;
  uint32_t *p = (uint32_t *)data;
  for (int y = a->y1; y <= a->y2; y++)
    for (int x = a->x1; x <= a->x2; x++)
      pixels[y * 480 + x] = *p++;
  lv_display_flush_ready(d);
}
static void advance(unsigned duration) {
  for (unsigned t = 0; t < duration; t += 10) {
    now += 10;
    lv_tick_inc(10);
    lv_timer_handler();
  }
}
static void snapshot(unsigned count, const char *last_state) {
  cJSON *s = cJSON_CreateObject(), *rows = cJSON_AddArrayToObject(s, "sessions");
  cJSON_AddArrayToObject(s, "usage");
  for (unsigned i = 0; i < count; i++) {
    cJSON *p = cJSON_CreateObject();
    char id[16];
    snprintf(id, sizeof(id), "s%u", i);
    cJSON_AddStringToObject(p, "id", id);
    cJSON_AddNumberToObject(p, "slot", i);
    cJSON_AddStringToObject(p, "name", "空闲测试");
    cJSON_AddStringToObject(p, "skin", "crab");
    cJSON_AddStringToObject(p, "source", "codex");
    cJSON_AddStringToObject(p, "state", i + 1 == count ? last_state : "idle");
    cJSON_AddItemToArray(rows, p);
  }
  assert(rg_ui_apply(s));
  cJSON_Delete(s);
}
static void touch(void) {
  test_pressed = true;
  advance(100);
  test_pressed = false;
  advance(100);
}
int main(int argc, char **argv) {
  assert(argc == 2);
  lv_init();
  lv_display_t *d = lv_display_create(480, 480);
  lv_display_set_color_format(d, LV_COLOR_FORMAT_XRGB8888);
  lv_display_set_buffers(d, buffer, NULL, sizeof(buffer), LV_DISPLAY_RENDER_MODE_PARTIAL);
  lv_display_set_flush_cb(d, flush);
  if (!strcmp(argv[1], "missing-touch")) {
    assert(!rg_ui_init());
    puts("没有触摸设备时拒绝就绪");
    return 0;
  }
  test_input_create();
  assert(rg_ui_init());
  snapshot(5, "idle");
  assert(rg_ui_active(true));
  advance(100);
  assert(rg_ui_healthy(now));
  if (!strcmp(argv[1], "awake-write-failure")) {
    test_draw_fails = true;
    advance(100);
    assert(!rg_ui_healthy(now) && !rg_ui_active(false));
    puts("亮屏底层写入失败立即拒绝健康和清屏成功");
    return 0;
  }
  if (!strcmp(argv[1], "awake-stall")) {
    lv_display_enable_invalidation(d, false);
    lv_timer_pause(lv_display_get_refr_timer(d));
    advance(600);
    assert(!rg_ui_healthy(now));
    puts("亮屏只认实际刷新，UI tick 无法掩盖停滞");
    return 0;
  }
  if (!strcmp(argv[1], "panel-off-failure"))
    test_panel_fails = true;
  // 不断更新快照，仍应从初始化开始算满 30 秒熄屏。
  for (unsigned i = 0; i < 29; i++) {
    snapshot(5, "idle");
    advance(1000);
  }
  assert(test_panel_on);
  advance(1000);
  if (!strcmp(argv[1], "panel-off-failure")) {
    assert(test_panel_on && !rg_ui_healthy(now));
    assert(!rg_ui_active(false));
    puts("面板关闭失败不能伪装成健康或清屏成功");
    return 0;
  }
  assert(!test_panel_on);
  assert(rg_ui_healthy(now));
  unsigned before = flushes;
  advance(1000);
  assert(flushes == before && rg_ui_healthy(now));
  assert(!rg_ui_healthy(now + 600));
  if (!strcmp(argv[1], "wake-write-failure")) {
    test_draw_fails = true;
    touch();
    assert(!test_panel_on && !rg_ui_healthy(now));
    puts("唤醒写屏失败保持熄灭，空完成事件无法掩盖错误");
    return 0;
  }
  if (!strcmp(argv[1], "wake-stall")) {
    // 模拟唤醒时无法提交新的失效区域；空的 REFR_READY 不能算新帧。
    lv_refr_now(d);
    lv_display_enable_invalidation(d, false);
    touch();
    assert(!test_panel_on && !rg_ui_healthy(now));
    puts("唤醒未提交新帧时保持关闭且拒绝健康");
    return 0;
  }
  if (!strcmp(argv[1], "panel-on-failure")) {
    test_panel_fails = true;
    touch();
    assert(!test_panel_on && !rg_ui_healthy(now));
    assert(!rg_ui_active(false));
    puts("面板打开失败不会继续报告健康");
    return 0;
  }
  touch();
  assert(test_panel_on && flushes > before && rg_ui_healthy(now));
  advance(20000);
  touch(); // 第二次触摸延长期限。
  advance(20000);
  assert(test_panel_on);
  advance(10100);
  assert(!test_panel_on);
  test_pressed = true;
  advance(31000); // 持续按住不会在手指下熄屏。
  assert(test_panel_on);
  test_pressed = false;
  advance(30100);
  assert(!test_panel_on);
  snapshot(6, "running"); // 当前分页以外也必须参与唤醒判断。
  advance(100);
  assert(test_panel_on);
  advance(30100);
  assert(test_panel_on);
  const char *attention[] = {"awaiting", "your_turn", "completed", "aborted"};
  for (unsigned i = 0; i < 4; i++) {
    snapshot(6, attention[i]);
    advance(30100);
    assert(test_panel_on); // 待处理/完成/异常不是 idle，不得被自动熄屏隐藏。
  }
  snapshot(6, "idle");
  advance(29900);
  assert(test_panel_on);
  advance(200);
  assert(!test_panel_on);
  assert(rg_ui_active(false)); // 已丢失授权的旧快照不能被触摸唤回。
  touch();
  assert(test_panel_on && (pixels[160 * 480 + 310] & 0xffffff) == 0);
  advance(30100);
  assert(!test_panel_on);
  snapshot(0, "idle");
  assert(rg_ui_active(true));
  advance(100);
  assert(!test_panel_on); // 无会话同样是全空闲。
  touch();
  assert(test_panel_on);
  advance(30100);
  assert(!test_panel_on);
  puts("实际 LVGL：空闲停刷、触摸/长按续期、跨页活动唤醒、失联清屏和健康通过");
}
