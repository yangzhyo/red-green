#pragma once
#include "cJSON.h"
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#define RG_MAX_FRAME 65536
#include "generated/version.h"
typedef enum {
  RG_ERROR = -1,
  RG_IGNORE,
  RG_BIND,
  RG_SNAPSHOT,
  RG_GRANT,
  RG_CALL
} rg_result;
typedef struct {
  char name[32], session[33];
  uint64_t expires, event;
  int priority;
} rg_call;
typedef struct {
  char boot[65], generation[81];
  bool request_healthy;
  uint64_t version, nonce, request_version, request_deadline, active_nonce,
      lease_deadline, last_event;
  cJSON *snapshot, *candidate;
  rg_call calls[16];
  size_t call_count;
} rg_device;
void rg_init(rg_device *d, const char *boot);
void rg_clear(rg_device *d);
rg_result rg_receive(rg_device *d, const char *json, size_t length,
                     uint64_t now);
bool rg_commit(rg_device *d);
cJSON *rg_request(rg_device *d, uint64_t now, bool healthy);
bool rg_active(const rg_device *d, uint64_t now);
void rg_expire(rg_device *d, uint64_t now);
bool rg_pop_call(rg_device *d, uint64_t now, rg_call *out);
const char *rg_string(const cJSON *obj, const char *key);
uint64_t rg_number(const cJSON *obj, const char *key);
typedef struct {
  uint8_t *bytes;
  size_t used, want;
} rg_decoder;
bool rg_decoder_init(rg_decoder *d);
void rg_decoder_clear(rg_decoder *d);
bool rg_feed(rg_decoder *d, const uint8_t *bytes, size_t n,
             bool (*consume)(const char *, size_t, void *), void *context);
