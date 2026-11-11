idf_component_register(SRCS "main.c" "device.c" "ui.c" "audio.c" "schedule.c" "generated/assets.c" INCLUDE_DIRS "."
  REQUIRES cjson esp-mosaico-bsp esp_timer esp_hw_support esp_system esp_tinyusb
  EMBED_FILES "generated/assets.bin" "generated/font16.bin" "generated/font20.bin" "generated/font28.bin")
