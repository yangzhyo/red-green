#pragma once
#include "device.h"
#include <stdbool.h>
#include <stdint.h>
bool rg_audio_init(void);
void rg_audio_task(void *context);
void rg_audio_authorize(uint32_t deadline);
bool rg_audio_healthy(uint32_t now);
bool rg_take_call(rg_call *call);
