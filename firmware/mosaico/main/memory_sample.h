#pragma once
#include "cJSON.h"
#include <stdbool.h>
#include <stdint.h>

typedef struct { uint64_t next_ms; } rg_memory_clock;
typedef struct { uint32_t free, largest, minimum; } rg_heap_sample;
bool rg_memory_due(rg_memory_clock *clock, uint64_t now);
bool rg_memory_attach(cJSON *request, uint64_t uptime,
                      const rg_heap_sample *internal, const rg_heap_sample *psram,
                      const cJSON *snapshot, bool screen_on, const char *firmware);
