// Python PTY 模拟器调用真实设备协议核心，避免第二套授权实现。
#include "device.h"
#include "memory_sample.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
static rg_device device;
static rg_memory_clock memory_clock;
void rg_sim_init(const char *boot) {
  rg_clear(&device);
  rg_init(&device, boot);
  memory_clock.next_ms = 0;
}
int rg_sim_receive(const char *json, uint64_t now) {
  rg_result r = rg_receive(&device, json, strlen(json), now);
  if (r == RG_SNAPSHOT)
    rg_commit(&device);
  return r;
}
const char *rg_sim_request(uint64_t now, int healthy) {
  static char output[1024];
  cJSON *j = rg_request(&device, now, healthy);
  if (!j)
    return NULL;
  if (rg_active(&device, now) && rg_memory_due(&memory_clock, now)) {
    rg_heap_sample internal = {100000, 50000, 90000};
    rg_heap_sample psram = {3000000, 2500000, 2900000};
    rg_memory_attach(j, now, &internal, &psram, device.snapshot, true, "simulator");
  }
  char *text = cJSON_PrintUnformatted(j);
  cJSON_Delete(j);
  if (!text)
    return NULL;
  snprintf(output, sizeof(output), "%s", text);
  free(text);
  return output;
}
int rg_sim_active(uint64_t now) { return rg_active(&device, now); }
const char *rg_sim_pop(uint64_t now) {
  static rg_call call;
  return rg_pop_call(&device, now, &call) ? call.name : NULL;
}
