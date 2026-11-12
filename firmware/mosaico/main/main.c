#include "audio.h"
#include "bsp/esp_mosaico.h"
#include "device.h"
#include "device/dcd.h"
#include "esp_heap_caps.h"
#include "esp_app_desc.h"
#include "memory_sample.h"
#include "esp_log.h"
#include "esp_mac.h"
#include "esp_random.h"
#include "esp_system.h"
#include "esp_task_wdt.h"
#include "esp_timer.h"
#include "freertos/FreeRTOS.h"
#include "freertos/semphr.h"
#include "freertos/task.h"
#include "hal/usb_utmi_ll.h"
#include "tinyusb.h"
#include "tinyusb_cdc_acm.h"
#include "tinyusb_default_config.h"
#include "tusb.h"
#include "ui.h"
#include <stdatomic.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
static rg_device device;
static rg_decoder decoder;
static SemaphoreHandle_t lock;
static bool showing;
static uint64_t bound_at;
static char identity[18];
enum { BOOT_USB, BOOT_DISPLAY, BOOT_FONTS, BOOT_AUDIO, BOOT_RUNNING };
static atomic_uint boot_phase;
static const char *last_fault = "none";
uint64_t rg_now(void) { return (uint64_t)esp_timer_get_time() / 1000; }
static void usb_shutdown(void) {
  // S31 CPU 复位前停止 USB DMA，避免下一次启动访问旧缓冲区。
  // 与官方 ESP-Iris 的 S31 shutdown 顺序一致。
  dcd_int_disable(0);
  usb_utmi_ll_reset_register();
  usb_utmi_ll_enable_bus_clock(false);
}
static void fail_closed(void) {
  // 只在只读板型校验成功后启动这些任务。避开堵塞的 LVGL/I2C 锁。
  gpio_set_level(BSP_AUDIO_PA_CTRL, 0);
  gpio_set_level(BSP_LCD_RST_V1_2, 0);
  esp_restart();
}
static void quiet(void) {
  rg_audio_authorize(0);
  if (showing && !rg_ui_active(false))
    fail_closed();
  showing = false;
}
static void unbind(void) {
  quiet();
  rg_clear(&device);
  decoder.used = decoder.want = 0;
  bound_at = 0;
}
bool rg_take_call(rg_call *out) {
  if (xSemaphoreTake(lock, pdMS_TO_TICKS(20)) != pdTRUE)
    return false;
  bool ok = rg_pop_call(&device, rg_now(), out);
  xSemaphoreGive(lock);
  return ok;
}
static bool send_json(cJSON *j) {
  if (!j)
    return false;
  char *json = cJSON_PrintUnformatted(j);
  cJSON_Delete(j);
  if (!json)
    return false;
  size_t n = strlen(json);
  bool ok = false;
  if (n + 8 <= 2048 && tud_mounted() && tud_cdc_n_write_available(0) >= n + 8) {
    uint8_t frame[2048];
    memcpy(frame, "RGP1", 4);
    for (int i = 0; i < 4; i++)
      frame[4 + i] = (uint8_t)(n >> (8 * i));
    memcpy(frame + 8, json, n);
    ok = tinyusb_cdcacm_write_queue(TINYUSB_CDC_ACM_0, frame, n + 8) == n + 8;
    if (ok) {
      esp_err_t result = tinyusb_cdcacm_write_flush(TINYUSB_CDC_ACM_0, 0);
      ok = result == ESP_OK || result == ESP_ERR_NOT_FINISHED;
    }
  }
  free(json);
  if (!ok)
    tud_cdc_n_write_clear(0);
  return ok;
}
static cJSON *message(const char *type) {
  cJSON *j = cJSON_CreateObject();
  cJSON_AddStringToObject(j, "type", type);
  cJSON_AddStringToObject(j, "boot", device.boot);
  cJSON_AddStringToObject(j, "generation", device.generation);
  return j;
}
static void hello(void) {
  cJSON *j = message("hello");
  cJSON_AddStringToObject(j, "app", "red-green.mosaico");
  cJSON_AddStringToObject(j, "device", identity);
  cJSON_AddNumberToObject(j, "protocol", 1);
  cJSON_AddStringToObject(j, "assets", RG_RESOURCES);
  cJSON_AddStringToObject(j, "board", "v1.2");
  unsigned phase = atomic_load(&boot_phase);
  const char *phases[] = {"usb", "display", "fonts", "audio", "running"};
  cJSON_AddStringToObject(j, "boot_phase", phases[phase]);
  cJSON_AddStringToObject(j, "last_fault", last_fault);
  cJSON_AddNumberToObject(j, "reset_reason", esp_reset_reason());
  cJSON_AddNumberToObject(j, "free_heap", esp_get_free_heap_size());
  cJSON_AddNumberToObject(j, "largest_heap",
                          heap_caps_get_largest_free_block(MALLOC_CAP_8BIT));
  cJSON_AddBoolToObject(j, "display_ok",
                        phase == BOOT_RUNNING &&
                            rg_ui_healthy((uint32_t)rg_now()));
  cJSON_AddBoolToObject(j, "audio_ok",
                        phase == BOOT_RUNNING &&
                            rg_audio_healthy((uint32_t)rg_now()));
  send_json(j);
}
static bool consume(const char *bytes, size_t size, void *context) {
  (void)context;
  rg_result result = rg_receive(&device, bytes, size, rg_now());
  switch (result) {
  case RG_ERROR:
    last_fault = "invalid frame";
    return false;
  case RG_BIND:
    quiet();
    bound_at = rg_now();
    break;
  case RG_SNAPSHOT: {
    if (!rg_ui_apply(device.candidate) || !rg_commit(&device))
      return false;
    if (!cJSON_GetArraySize(
            cJSON_GetObjectItemCaseSensitive(device.snapshot, "sessions")))
      rg_audio_authorize(0);
    cJSON *j = message("applied");
    cJSON_AddNumberToObject(j, "version", device.version);
    if (!send_json(j))
      return false;
    break;
  }
  case RG_GRANT:
    if (!rg_ui_healthy((uint32_t)rg_now()) ||
        !rg_audio_healthy((uint32_t)rg_now())) {
      last_fault = "grant health";
      return false;
    }
    if (!showing && !rg_ui_active(true)) {
      last_fault = "grant display lock";
      return false;
    }
    showing = true;
    rg_audio_authorize((uint32_t)device.lease_deadline);
    break;
  default:
    break;
  }
  return true;
}
static void guard(void *context) {
  (void)context;
  ESP_ERROR_CHECK(esp_task_wdt_add(NULL));
  for (;;) {
    if (xSemaphoreTake(lock, pdMS_TO_TICKS(200)) != pdTRUE)
      fail_closed();
    uint64_t now = rg_now();
    if (showing && (!rg_active(&device, now) || !rg_ui_healthy((uint32_t)now) ||
                    !rg_audio_healthy((uint32_t)now))) {
      last_fault = !rg_active(&device, now)        ? "lease expired"
                   : !rg_ui_healthy((uint32_t)now) ? "display stalled"
                                                   : "audio stalled";
      unbind();
    }
    xSemaphoreGive(lock);
    ESP_ERROR_CHECK(esp_task_wdt_reset());
    vTaskDelay(pdMS_TO_TICKS(20));
  }
}
static rg_heap_sample read_heap(uint32_t caps) {
  multi_heap_info_t info;
  heap_caps_get_info(&info, caps | MALLOC_CAP_8BIT);
  return (rg_heap_sample){info.total_free_bytes, info.largest_free_block,
                          info.minimum_free_bytes};
}
static void serial_task(void *context) {
  (void)context;
  uint64_t last_hello = 0, last_request = 0;
  rg_memory_clock memory_clock = {0};
  uint8_t bytes[4096];
  for (;;) {
    // 初始化失败也保留诊断握手；尚未就绪时不处理任何业务输入。
    if (atomic_load(&boot_phase) != BOOT_RUNNING) {
      hello();
      vTaskDelay(pdMS_TO_TICKS(500));
      continue;
    }
    if (xSemaphoreTake(lock, pdMS_TO_TICKS(40)) != pdTRUE)
      fail_closed();
    uint64_t now = rg_now();
    size_t n = 0;
    if (!tud_mounted())
      unbind();
    else {
      if (tinyusb_cdcacm_read(TINYUSB_CDC_ACM_0, bytes, sizeof(bytes), &n) !=
              ESP_OK ||
          (n && !rg_feed(&decoder, bytes, n, consume, NULL)))
        unbind();
      if (!device.generation[0]) {
        if (now - last_hello >= 1000) {
          hello();
          last_hello = now;
        }
      } else if (!showing && now - bound_at > 5000)
        unbind();
      else if (device.version && now - last_request >= 500) {
        bool healthy =
            rg_ui_healthy((uint32_t)now) && rg_audio_healthy((uint32_t)now);
        cJSON *request = rg_request(&device, rg_now(), healthy);
        if (request && showing && rg_memory_due(&memory_clock, now)) {
          rg_heap_sample internal = read_heap(MALLOC_CAP_INTERNAL);
          rg_heap_sample psram = read_heap(MALLOC_CAP_SPIRAM);
          // 诊断分配失败只丢弃本次样本，仍发送原健康续期。
          (void)rg_memory_attach(request, now, &internal, &psram, device.snapshot,
                                 rg_ui_screen_on(), esp_app_get_description()->version);
        }
        if (!send_json(request))
          unbind();
        last_request = now;
      }
    }
    xSemaphoreGive(lock);
    vTaskDelay(pdMS_TO_TICKS(20));
  }
}
void app_main(void) {
  bsp_board_variant_t variant;
  if (bsp_board_variant_get(&variant) != ESP_OK ||
      variant != BSP_BOARD_VARIANT_V1_2) {
    ESP_LOGE("red-green", "不支持的板型；保持电脑输出");
    return;
  }
  uint8_t mac[6];
  ESP_ERROR_CHECK(esp_read_mac(mac, ESP_MAC_BASE));
  snprintf(identity, sizeof(identity), "%02x%02x%02x%02x%02x%02x", mac[0],
           mac[1], mac[2], mac[3], mac[4], mac[5]);
  char boot[65];
  snprintf(boot, sizeof(boot), "%s-%08lx%08lx", identity,
           (unsigned long)esp_random(), (unsigned long)esp_random());
  rg_init(&device, boot);
  lock = xSemaphoreCreateMutex();
  if (!lock || !rg_decoder_init(&decoder))
    return;
  tinyusb_config_t usb = TINYUSB_DEFAULT_CONFIG();
  ESP_ERROR_CHECK(tinyusb_driver_install(&usb));
  ESP_ERROR_CHECK(esp_register_shutdown_handler(usb_shutdown));
  tinyusb_config_cdcacm_t cdc = {.cdc_port = TINYUSB_CDC_ACM_0};
  ESP_ERROR_CHECK(tinyusb_cdcacm_init(&cdc));
  configASSERT(xTaskCreate(serial_task, "rg_usb", 12288, NULL, 5, NULL) ==
               pdPASS);
  bsp_display_config_t display = BSP_DISPLAY_DEFAULT_CONFIG();
  display.enable_touch = true;
  atomic_store(&boot_phase, BOOT_DISPLAY);
  if (!bsp_display_start_with_config(&display))
    return;
  atomic_store(&boot_phase, BOOT_FONTS);
  if (!rg_ui_init())
    return;
  atomic_store(&boot_phase, BOOT_AUDIO);
  if (!rg_audio_init()) {
    gpio_set_level(BSP_AUDIO_PA_CTRL, 0);
    return;
  }
  // BSP 屏幕/字体初始化会长时间占用 CPU；此时尚不允许接管。
  // 初始化结束后只监控高优先级授权看门任务，保持业务失效上限 2 s。
  const esp_task_wdt_config_t watchdog = {
      .timeout_ms = 2000, .idle_core_mask = 0, .trigger_panic = true};
  ESP_ERROR_CHECK(esp_task_wdt_init(&watchdog));
  configASSERT(xTaskCreate(rg_audio_task, "rg_audio", 4096, NULL, 6, NULL) ==
               pdPASS);
  configASSERT(xTaskCreate(guard, "rg_guard", 4096, NULL, 10, NULL) == pdPASS);
  atomic_store(&boot_phase, BOOT_RUNNING);
}
