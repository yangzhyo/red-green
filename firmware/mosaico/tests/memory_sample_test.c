#include "memory_sample.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>

int main(void) {
  rg_memory_clock clock = {0};
  assert(!rg_memory_due(&clock, 0));
  assert(!rg_memory_due(&clock, 59999));
  assert(rg_memory_due(&clock, 60000));
  assert(!rg_memory_due(&clock, 60000));
  assert(!rg_memory_due(&clock, 119999));
  assert(rg_memory_due(&clock, 150000));
  assert(!rg_memory_due(&clock, 150001));
  assert(!rg_memory_due(&clock, 209999));
  assert(rg_memory_due(&clock, UINT64_C(4294968000)));
  assert(!rg_memory_due(&clock, UINT64_C(4294968001)));

  cJSON *request = cJSON_Parse("{\"type\":\"lease_request\",\"healthy\":true}");
  cJSON *snapshot = cJSON_Parse("{\"sessions\":[{\"state\":\"idle\"},{\"state\":\"idle\"}]}");
  rg_heap_sample internal = {100000, 50000, 90000};
  rg_heap_sample psram = {3000000, 2500000, 2900000};
  assert(rg_memory_attach(request, 60000, &internal, &psram, snapshot, false, "test-build"));
  cJSON *memory = cJSON_GetObjectItemCaseSensitive(request, "memory");
  assert(cJSON_GetNumberValue(cJSON_GetObjectItemCaseSensitive(memory, "uptime_ms")) == 60000);
  assert(cJSON_IsTrue(cJSON_GetObjectItemCaseSensitive(memory, "idle")));
  assert(cJSON_IsFalse(cJSON_GetObjectItemCaseSensitive(memory, "screen_on")));
  assert(cJSON_GetNumberValue(cJSON_GetObjectItemCaseSensitive(memory, "sessions")) == 2);
  assert(strcmp(cJSON_GetStringValue(cJSON_GetObjectItemCaseSensitive(memory, "firmware")), "test-build") == 0);
  cJSON *heap = cJSON_GetObjectItemCaseSensitive(memory, "internal");
  assert(cJSON_GetNumberValue(cJSON_GetObjectItemCaseSensitive(heap, "free")) == 100000);
  assert(cJSON_GetNumberValue(cJSON_GetObjectItemCaseSensitive(heap, "largest")) == 50000);
  assert(cJSON_GetNumberValue(cJSON_GetObjectItemCaseSensitive(heap, "minimum")) == 90000);
  cJSON_Delete(request);
  request = cJSON_Parse("{\"type\":\"lease_request\"}");
  cJSON_AddItemToArray(cJSON_GetObjectItemCaseSensitive(snapshot, "sessions"), cJSON_Parse("{\"state\":\"running\"}"));
  assert(rg_memory_attach(request, 120000, &internal, &psram, snapshot, true, "test-build"));
  assert(cJSON_IsFalse(cJSON_GetObjectItemCaseSensitive(cJSON_GetObjectItemCaseSensitive(request, "memory"), "idle")));
  assert(!rg_memory_attach(NULL, 0, &internal, &psram, snapshot, false, "test-build"));
  cJSON_Delete(request);
  cJSON_Delete(snapshot);
  puts("memory: 每分钟采样、64 位时钟与空闲/屏幕上下文通过");
}
