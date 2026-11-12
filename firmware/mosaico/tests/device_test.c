#include "device.h"
#include "health.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>
static rg_result receive(rg_device *d, const char *j, uint64_t now) {
  return rg_receive(d, j, strlen(j), now);
}
static void prepared(rg_device *d) {
  rg_init(d, "boot1");
  assert(receive(d,
                 "{\"type\":\"bind\",\"boot\":\"boot1\",\"generation\":\"g1\","
                 "\"protocol\":1,\"assets\":\"" RG_RESOURCES "\"}",
                 0) == RG_BIND);
  assert(receive(d,
                 "{\"type\":\"snapshot\",\"generation\":\"g1\",\"version\":1,"
                 "\"sessions\":[{\"id\":\"s1\",\"slot\":0,\"name\":\"工具箱\","
                 "\"source\":\"codex\",\"skin\":\"cat\",\"state\":\"awaiting\"}"
                 "],\"usage\":[]}",
                 0) == RG_SNAPSHOT);
  assert(!rg_active(d, 0));
  assert(rg_commit(d));
}
static const char *grant = "{\"type\":\"grant\",\"boot\":\"boot1\","
                           "\"generation\":\"g1\",\"nonce\":1,\"version\":1}";
static void late_grant_cannot_extend_from_arrival(void) {
  rg_device d;
  prepared(&d);
  cJSON_Delete(rg_request(&d, 100, true));
  assert(receive(&d, grant, 2101) == RG_IGNORE);
  assert(!rg_active(&d, 2101));
  rg_clear(&d);
  prepared(&d);
  cJSON_Delete(rg_request(&d, 100, true));
  assert(receive(&d, grant, 1900) == RG_GRANT);
  assert(rg_active(&d, 2099));
  assert(!rg_active(&d, 2100));
  rg_clear(&d);
}
static void calls_deduplicate_expire_and_reject_old_generation(void) {
  rg_device d;
  prepared(&d);
  cJSON_Delete(rg_request(&d, 100, true));
  assert(receive(&d, grant, 200) == RG_GRANT);
  const char *call =
      "{\"type\":\"call\",\"generation\":\"g1\",\"boot\":\"boot1\",\"nonce\":1,"
      "\"event\":1,\"session\":\"s1\",\"name\":\"cat-awaiting\"}";
  assert(receive(&d, call, 200) == RG_CALL);
  assert(receive(&d, call, 201) == RG_IGNORE);
  rg_call out;
  assert(rg_pop_call(&d, 202, &out));
  assert(strcmp(out.name, "cat-awaiting") == 0);
  assert(!rg_pop_call(&d, 203, &out));
  assert(receive(&d,
                 "{\"type\":\"grant\",\"boot\":\"boot1\",\"generation\":"
                 "\"old\",\"nonce\":1,\"version\":1}",
                 204) == RG_IGNORE);
  assert(receive(&d,
                 "{\"type\":\"call\",\"generation\":\"g1\",\"boot\":\"boot1\","
                 "\"nonce\":1,\"event\":2,\"session\":\"s1\",\"name\":\"cat-"
                 "completed\"}",
                 205) == RG_CALL);
  rg_expire(&d, 2100);
  assert(!rg_pop_call(&d, 2100, &out));
  rg_clear(&d);
}
static void empty_snapshot_discards_queued_calls(void) {
  rg_device d;
  prepared(&d);
  cJSON_Delete(rg_request(&d, 100, true));
  assert(receive(&d, grant, 200) == RG_GRANT);
  assert(receive(&d,
                 "{\"type\":\"call\",\"generation\":\"g1\",\"boot\":\"boot1\","
                 "\"nonce\":1,\"event\":1,\"session\":\"s1\",\"name\":\"cat-"
                 "awaiting\"}",
                 201) == RG_CALL);
  assert(receive(&d,
                 "{\"type\":\"snapshot\",\"generation\":\"g1\",\"version\":2,"
                 "\"sessions\":[],\"usage\":[]}",
                 202) == RG_SNAPSHOT);
  assert(rg_commit(&d));
  rg_call call;
  assert(!rg_pop_call(&d, 203, &call));
  rg_clear(&d);
}
static bool consume(const char *j, size_t n, void *x) {
  assert(n == 7 && memcmp(j, "{\"x\":1}", 7) == 0);
  (*(int *)x)++;
  return true;
}
static void frames_bound_memory_and_parse_fragments(void) {
  rg_decoder d;
  assert(rg_decoder_init(&d));
  int count = 0;
  const uint8_t frame[] = {'R', 'G', 'P', '1', 7,   0,   0,  0,
                           '{', '"', 'x', '"', ':', '1', '}'};
  assert(rg_feed(&d, frame, 5, consume, &count));
  assert(count == 0);
  assert(rg_feed(&d, frame + 5, 10, consume, &count));
  assert(count == 1);
  const uint8_t huge[] = {'R', 'G', 'P', '1', 1, 0, 1, 0};
  assert(!rg_feed(&d, huge, 8, consume, &count));
  rg_decoder_clear(&d);
  rg_device device;
  rg_init(&device, "boot1");
  assert(receive(&device, "{", 0) == RG_ERROR);
  assert(receive(&device, "{\"type\":\"bind\",\"protocol\":2}", 0) == RG_ERROR);
  rg_clear(&device);
}
static void unhealthy_request_and_json_suffix_are_rejected(void) {
  rg_device d;
  prepared(&d);
  cJSON_Delete(rg_request(&d, 100, false));
  assert(receive(&d, grant, 200) == RG_IGNORE);
  assert(
      receive(
          &d,
          "{\"type\":\"release\",\"generation\":\"g1\",\"boot\":\"boot1\"}junk",
          201) == RG_ERROR);
  rg_clear(&d);
}
int main(void) {
  empty_snapshot_discards_queued_calls();
  assert(rg_recent(1000, 1001));
  assert(rg_recent(50, UINT32_MAX - 10));
  assert(!rg_recent(1600, 1000));
  assert(!rg_recent(1000, 0));
  late_grant_cannot_extend_from_arrival();
  calls_deduplicate_expire_and_reject_old_generation();
  frames_bound_memory_and_parse_fragments();
  unhealthy_request_and_json_suffix_are_rejected();
  puts("device: 授权、去重、过期与有界帧测试通过");
  return 0;
}
