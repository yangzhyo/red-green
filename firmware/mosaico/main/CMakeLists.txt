idf_component_register(SRCS "main.c" "memory_sample.c" "device.c" "ui.c" "audio.c" "schedule.c" "hero.c" "idle_screen.c" "panel.c" "generated/assets.c" INCLUDE_DIRS "."
  REQUIRES cjson esp-mosaico-bsp esp_timer esp_hw_support esp_system esp_tinyusb esp_lcd
  PRIV_REQUIRES esp_app_format
  EMBED_FILES "generated/assets.bin" "generated/font16.bin" "generated/font20.bin" "generated/font28.bin")
