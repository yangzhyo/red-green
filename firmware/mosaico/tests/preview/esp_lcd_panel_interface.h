#pragma once
#include <stdbool.h>
// 主机侧只模拟应用使用的官方 panel 接口字段。
typedef int esp_err_t;
#define ESP_OK 0
#define ESP_FAIL -1
typedef struct esp_lcd_panel_t esp_lcd_panel_t;
typedef esp_lcd_panel_t *esp_lcd_panel_handle_t;
struct esp_lcd_panel_t {
  esp_err_t (*draw_bitmap)(esp_lcd_panel_t *, int, int, int, int, const void *);
};
