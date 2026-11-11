#include "lvgl.h"
#include "ui.h"
#include "native_panel.h"
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
static uint64_t now;
uint64_t rg_now(void) { return now; }
static uint32_t pixels[480 * 480], buffer[480 * 48];
static void flush(lv_display_t *d, const lv_area_t *a, uint8_t *data) {
  uint32_t *p = (uint32_t *)data;
  for (int y = a->y1; y <= a->y2; y++)
    for (int x = a->x1; x <= a->x2; x++)
      pixels[y * 480 + x] = *p++;
  lv_display_flush_ready(d);
}
int main(int argc, char **argv) {
  assert(argc >= 3);
  FILE *in = fopen(argv[1], "rb");
  assert(in);
  fseek(in, 0, SEEK_END);
  long size = ftell(in);
  rewind(in);
  char *json = calloc(size + 1, 1);
  assert(fread(json, 1, size, in) == (size_t)size);
  fclose(in);
  lv_init();
  lv_display_t *d = lv_display_create(480, 480);
  lv_display_set_color_format(d, LV_COLOR_FORMAT_XRGB8888);
  lv_display_set_buffers(d, buffer, NULL, sizeof(buffer),
                         LV_DISPLAY_RENDER_MODE_PARTIAL);
  lv_display_set_flush_cb(d, flush);
  test_input_create();
  assert(rg_ui_init());
  cJSON *s = cJSON_Parse(json);
  assert(s);
  // 数组夹具每 200 ms 应用一个快照，用于检查同一界面的状态切换。
  cJSON *initial = cJSON_IsArray(s) ? cJSON_GetArrayItem(s, 0) : s;
  assert(rg_ui_apply(initial));
  cJSON *invalid = cJSON_Duplicate(initial, true);
  cJSON *first = cJSON_GetArrayItem(
      cJSON_GetObjectItemCaseSensitive(invalid, "sessions"), 0);
  if (first) {
    cJSON_ReplaceItemInObjectCaseSensitive(first, "name",
                                           cJSON_CreateString("缺字测试😀"));
    assert(!rg_ui_apply(invalid));
  }
  cJSON_Delete(invalid);
  assert(rg_ui_active(true));
  unsigned end = argc > 3 ? atoi(argv[3]) : 2000;
  for (now = 10; now < end; now += 10) {
    if (cJSON_IsArray(s) && now % 200 == 0) {
      cJSON *next = cJSON_GetArrayItem(s, (int)(now / 200));
      if (next)
        assert(rg_ui_apply(next));
    }
    lv_tick_inc(10);
    lv_timer_handler();
  }
  FILE *out = fopen(argv[2], "wb");
  assert(out);
  fprintf(out, "P6\n480 480\n255\n");
  for (int i = 0; i < 480 * 480; i++) {
    fputc(pixels[i] >> 16, out);
    fputc(pixels[i] >> 8, out);
    fputc(pixels[i], out);
  }
  fclose(out);
  assert(rg_ui_healthy((uint32_t)now));
  printf("LVGL 真实界面帧 %s；缺字时拒绝接管\n", argv[2]);
  cJSON_Delete(s);
  free(json);
}
