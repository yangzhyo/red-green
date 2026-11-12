#include "audio.h"
#include "assets.h"
#include "bsp/esp_mosaico.h"
#include "esp_codec_dev.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "health.h"
#include "ui.h"
#include <stdatomic.h>
#include <string.h>
static esp_codec_dev_handle_t speaker;
static atomic_uint_least32_t deadline, progress, cancellation;
static bool allowed(void) {
  uint32_t d = atomic_load(&deadline);
  return d && (int32_t)(d - (uint32_t)rg_now()) > 0;
}
bool rg_audio_init(void) {
  speaker = bsp_audio_codec_speaker_init();
  if (!speaker)
    return false;
  esp_codec_dev_sample_info_t format = {
      .sample_rate = 48000, .channel = 2, .bits_per_sample = 16};
  if (esp_codec_dev_open(speaker, &format) != ESP_CODEC_DEV_OK)
    return false;
  if (esp_codec_dev_set_out_vol(speaker, 45) != ESP_CODEC_DEV_OK)
    return false;
  if (esp_codec_dev_set_out_mute(speaker, true) != ESP_CODEC_DEV_OK)
    return false;
  gpio_set_level(BSP_AUDIO_PA_CTRL, 0);
  return true;
}
// 由持有状态锁的 I/O/看门任务调用；音频任务不能重新打开已关闭的功放。
void rg_audio_authorize(uint32_t value) {
  if (!value)
    atomic_fetch_add(&cancellation, 1);
  atomic_store(&deadline, value);
  gpio_set_level(BSP_AUDIO_PA_CTRL, value ? 1 : 0);
}
bool rg_audio_healthy(uint32_t now) {
  uint32_t stamp = atomic_load(&progress);
  return rg_recent(now, stamp);
}
static bool write_pcm(const void *bytes, size_t size) {
  if (esp_codec_dev_write(speaker, (void *)bytes, (int)size) !=
      ESP_CODEC_DEV_OK)
    return false;
  atomic_store(&progress, (uint32_t)rg_now());
  return true;
}
void rg_audio_task(void *context) {
  (void)context;
  const uint8_t silence[1920] = {0};
  for (;;) {
    rg_call call;
    uint32_t ticket = atomic_load(&cancellation);
    if (allowed() && rg_take_call(&call)) {
      const rg_sound *sound = NULL;
      for (int i = 0; i < 12; i++)
        if (!strcmp(rg_sounds[i].name, call.name))
          sound = &rg_sounds[i];
      if (sound && allowed() && ticket == atomic_load(&cancellation) &&
          rg_now() < call.expires) {
        if (esp_codec_dev_set_out_mute(speaker, false) != ESP_CODEC_DEV_OK)
          break;
        // 10 ms 小块；失效时功放由高优先级任务直接关闭。
        for (size_t p = 0; p < sound->length && allowed() &&
                           ticket == atomic_load(&cancellation);) {
          size_t n = sound->length - p;
          if (n > 1920)
            n = 1920;
          if (!write_pcm(sound->pcm + p, n))
            goto failed;
          p += n;
        }
        // 先排入静音让最后一段 DMA 播完，再静音，保留完整尾音。
        for (int i = 0;
             i < 6 && allowed() && ticket == atomic_load(&cancellation); i++)
          if (!write_pcm(silence, sizeof(silence)))
            goto failed;
        if (esp_codec_dev_set_out_mute(speaker, true) != ESP_CODEC_DEV_OK)
          break;
      }
    } else {
      if (!write_pcm(silence, sizeof(silence)))
        break;
      vTaskDelay(pdMS_TO_TICKS(10));
    }
  }
failed:
  atomic_store(&progress, 0);
  gpio_set_level(BSP_AUDIO_PA_CTRL, 0);
  vTaskDelete(NULL);
}
