#include "memory_sample.h"
#include <string.h>

#define SAMPLE_PERIOD_MS UINT64_C(60000)

bool rg_memory_due(rg_memory_clock *clock, uint64_t now) {
  uint64_t due = clock->next_ms ? clock->next_ms : SAMPLE_PERIOD_MS;
  if (now < due)
    return false;
  clock->next_ms = now + SAMPLE_PERIOD_MS;
  return true;
}
static bool add_heap(cJSON *memory, const char *name, const rg_heap_sample *heap) {
  cJSON *j = cJSON_AddObjectToObject(memory, name);
  return j && cJSON_AddNumberToObject(j, "free", heap->free) &&
         cJSON_AddNumberToObject(j, "largest", heap->largest) &&
         cJSON_AddNumberToObject(j, "minimum", heap->minimum);
}
bool rg_memory_attach(cJSON *request, uint64_t uptime,
                      const rg_heap_sample *internal, const rg_heap_sample *psram,
                      const cJSON *snapshot, bool screen_on, const char *firmware) {
  if (!cJSON_IsObject(request) || !internal || !psram || !firmware)
    return false;
  const cJSON *sessions = cJSON_GetObjectItemCaseSensitive(snapshot, "sessions");
  bool idle = cJSON_IsArray(sessions);
  const cJSON *session;
  cJSON_ArrayForEach(session, sessions) {
    const char *state = cJSON_GetStringValue(cJSON_GetObjectItemCaseSensitive(session, "state"));
    if (!state || strcmp(state, "idle"))
      idle = false;
  }
  cJSON *memory = cJSON_CreateObject();
  bool ok = memory && cJSON_AddNumberToObject(memory, "uptime_ms", (double)uptime) &&
            cJSON_AddStringToObject(memory, "firmware", firmware) &&
            cJSON_AddBoolToObject(memory, "idle", idle) &&
            cJSON_AddBoolToObject(memory, "screen_on", screen_on) &&
            cJSON_AddNumberToObject(memory, "sessions", cJSON_GetArraySize(sessions)) &&
            add_heap(memory, "internal", internal) && add_heap(memory, "psram", psram);
  if (ok && cJSON_AddItemToObject(request, "memory", memory))
    return true;
  cJSON_Delete(memory);
  return false;
}
