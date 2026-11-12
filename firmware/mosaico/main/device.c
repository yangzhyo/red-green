#include "device.h"
#include <ctype.h>
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

const char *rg_string(const cJSON *obj, const char *key) {
  const cJSON *v = cJSON_GetObjectItemCaseSensitive(obj, key);
  return cJSON_IsString(v) ? v->valuestring : "";
}
uint64_t rg_number(const cJSON *obj, const char *key) {
  const cJSON *v = cJSON_GetObjectItemCaseSensitive(obj, key);
  return cJSON_IsNumber(v) && isfinite(v->valuedouble) && v->valuedouble > 0 &&
                 v->valuedouble <= 9007199254740991.0 &&
                 floor(v->valuedouble) == v->valuedouble
             ? (uint64_t)v->valuedouble
             : 0;
}
static bool text(const cJSON *obj, const char *key, size_t max, bool empty) {
  const cJSON *v = cJSON_GetObjectItemCaseSensitive(obj, key);
  if (!cJSON_IsString(v) || strlen(v->valuestring) > max ||
      (!empty && !v->valuestring[0]))
    return false;
  const unsigned char *p = (const unsigned char *)v->valuestring;
  while (*p) {
    unsigned c = *p++, cp = c, min = 0;
    int count = 0;
    if (c < 0x80) {
      if (c < 0x20 || c == 0x7f)
        return false;
      continue;
    }
    if ((c & 0xe0) == 0xc0) {
      count = 1;
      cp = c & 0x1f;
      min = 0x80;
    } else if ((c & 0xf0) == 0xe0) {
      count = 2;
      cp = c & 0xf;
      min = 0x800;
    } else if ((c & 0xf8) == 0xf0) {
      count = 3;
      cp = c & 7;
      min = 0x10000;
    } else
      return false;
    while (count--) {
      if (!*p || (*p & 0xc0) != 0x80)
        return false;
      cp = (cp << 6) | (*p++ & 0x3f);
    }
    if (cp < min || cp > 0x10ffff || (cp >= 0xd800 && cp <= 0xdfff))
      return false;
  }
  return true;
}
static bool is_source(const char *s) {
  return !strcmp(s, "codex") || !strcmp(s, "claude");
}
static bool is_skin(const char *s) {
  return !strcmp(s, "robot") || !strcmp(s, "crab") || !strcmp(s, "cat");
}
static bool is_state(const char *s) {
  return !strcmp(s, "idle") || !strcmp(s, "running") ||
         !strcmp(s, "awaiting") || !strcmp(s, "aborted") ||
         !strcmp(s, "your_turn") || !strcmp(s, "completed");
}
static bool valid_snapshot(const cJSON *s) {
  const cJSON *pets = cJSON_GetObjectItemCaseSensitive(s, "sessions"),
              *usage = cJSON_GetObjectItemCaseSensitive(s, "usage");
  if (!cJSON_IsArray(pets) || !cJSON_IsArray(usage) ||
      cJSON_GetArraySize(pets) > 32 || cJSON_GetArraySize(usage) > 32)
    return false;
  const cJSON *p;
  cJSON_ArrayForEach(p, pets) {
    if (!text(p, "id", 32, false) || !text(p, "name", 208, true) ||
        !is_source(rg_string(p, "source")) || !is_skin(rg_string(p, "skin")) ||
        !is_state(rg_string(p, "state")))
      return false;
    for (const cJSON *q = p->next; q; q = q->next)
      if (!strcmp(rg_string(p, "id"), rg_string(q, "id")))
        return false;
  }
  cJSON_ArrayForEach(p, usage) {
    if (!text(p, "id", 192, false) || !text(p, "label", 128, true) ||
        !is_source(rg_string(p, "source")))
      return false;
    const cJSON *windows = cJSON_GetObjectItemCaseSensitive(p, "windows"), *w;
    if (!cJSON_IsArray(windows) || cJSON_GetArraySize(windows) > 32)
      return false;
    cJSON_ArrayForEach(w, windows) {
      const cJSON *pct = cJSON_GetObjectItemCaseSensitive(w, "used_percentage");
      if (!cJSON_IsNull(pct) &&
          (!cJSON_IsNumber(pct) || !isfinite(pct->valuedouble) ||
           pct->valuedouble < 0 || pct->valuedouble > 100))
        return false;
      if (!text(w, "duration_label", 32, true) ||
          !text(w, "reset_label", 48, true))
        return false;
    }
  }
  return true;
}
void rg_init(rg_device *d, const char *boot) {
  memset(d, 0, sizeof(*d));
  snprintf(d->boot, sizeof(d->boot), "%s", boot);
}
void rg_clear(rg_device *d) {
  char boot[65];
  memcpy(boot, d->boot, sizeof(boot));
  cJSON_Delete(d->snapshot);
  cJSON_Delete(d->candidate);
  rg_init(d, boot);
}
bool rg_active(const rg_device *d, uint64_t now) {
  return d->generation[0] && d->version > 0 && d->lease_deadline > now;
}
void rg_expire(rg_device *d, uint64_t now) {
  if (!rg_active(d, now)) {
    d->lease_deadline = 0;
    d->call_count = 0;
  }
}
bool rg_commit(rg_device *d) {
  if (!d->candidate)
    return false;
  cJSON_Delete(d->snapshot);
  d->snapshot = d->candidate;
  d->candidate = NULL;
  d->version = rg_number(d->snapshot, "version");
  for (size_t i = 0; i < d->call_count;) {
    bool present = false;
    const cJSON *p;
    cJSON_ArrayForEach(
        p, cJSON_GetObjectItemCaseSensitive(
               d->snapshot, "sessions")) if (!strcmp(rg_string(p, "id"),
                                                     d->calls[i].session))
        present = true;
    if (!present)
      memmove(&d->calls[i], &d->calls[i + 1],
              (--d->call_count - i) * sizeof(rg_call));
    else
      i++;
  }
  return true;
}
cJSON *rg_request(rg_device *d, uint64_t now, bool healthy) {
  if (!d->generation[0] || !d->version)
    return NULL;
  d->nonce++;
  d->request_version = d->version;
  d->request_deadline = now + 2000;
  d->request_healthy = healthy;
  if (!healthy) {
    d->lease_deadline = 0;
    d->call_count = 0;
  }
  cJSON *j = cJSON_CreateObject();
  cJSON_AddStringToObject(j, "type", "lease_request");
  cJSON_AddStringToObject(j, "boot", d->boot);
  cJSON_AddStringToObject(j, "generation", d->generation);
  cJSON_AddNumberToObject(j, "nonce", d->nonce);
  cJSON_AddNumberToObject(j, "version", d->version);
  cJSON_AddBoolToObject(j, "healthy", healthy);
  return j;
}
static rg_result accept(rg_device *d, cJSON *j, uint64_t now) {
  const char *type = rg_string(j, "type");
  if (!strcmp(type, "bind")) {
    if (rg_number(j, "protocol") != 1 ||
        strcmp(rg_string(j, "assets"), RG_RESOURCES) ||
        strcmp(rg_string(j, "boot"), d->boot) ||
        !text(j, "generation", 80, false))
      return RG_ERROR;
    rg_clear(d);
    snprintf(d->generation, sizeof(d->generation), "%s",
             rg_string(j, "generation"));
    return RG_BIND;
  }
  if (!d->generation[0] || strcmp(rg_string(j, "generation"), d->generation))
    return RG_IGNORE;
  if (!strcmp(type, "snapshot")) {
    uint64_t version = rg_number(j, "version");
    if (!version || !valid_snapshot(j))
      return RG_ERROR;
    if (version <= d->version ||
        (d->candidate && version <= rg_number(d->candidate, "version")))
      return RG_IGNORE;
    cJSON *next = cJSON_Duplicate(j, true);
    if (!next)
      return RG_ERROR;
    cJSON_Delete(d->candidate);
    d->candidate = next;
    return RG_SNAPSHOT;
  }
  if (strcmp(rg_string(j, "boot"), d->boot))
    return RG_IGNORE;
  if (!strcmp(type, "grant")) {
    if (!d->request_healthy || !d->nonce || rg_number(j, "nonce") != d->nonce ||
        now >= d->request_deadline ||
        rg_number(j, "version") != d->request_version ||
        d->version != d->request_version)
      return RG_IGNORE;
    d->active_nonce = d->nonce;
    d->lease_deadline = d->request_deadline;
    return RG_GRANT;
  }
  if (!strcmp(type, "release")) {
    rg_clear(d);
    return RG_BIND;
  }
  if (!strcmp(type, "call")) {
    uint64_t event = rg_number(j, "event");
    if (!rg_active(d, now) || rg_number(j, "nonce") != d->active_nonce ||
        !event || event <= d->last_event)
      return RG_IGNORE;
    const char *name = rg_string(j, "name"), *id = rg_string(j, "session");
    const cJSON *p;
    bool known = false;
    cJSON_ArrayForEach(
        p, cJSON_GetObjectItemCaseSensitive(d->snapshot, "sessions")) {
      const char *skin = rg_string(p, "skin");
      size_t n = strlen(skin);
      if (!strcmp(id, rg_string(p, "id")) && strlen(name) > n &&
          strncmp(name, skin, n) == 0 && name[n] == '-')
        known = true;
    }
    const char *state = strchr(name, '-');
    int priority = -1;
    if (state) {
      state++;
      if (!strcmp(state, "awaiting"))
        priority = 0;
      else if (!strcmp(state, "aborted"))
        priority = 1;
      else if (!strcmp(state, "your_turn"))
        priority = 2;
      else if (!strcmp(state, "completed"))
        priority = 3;
    }
    if (!known || priority < 0 || strlen(name) >= 32)
      return RG_ERROR;
    d->last_event = event;
    if (d->call_count >= 16)
      return RG_IGNORE;
    rg_call *c = &d->calls[d->call_count++];
    snprintf(c->name, sizeof(c->name), "%s", name);
    snprintf(c->session, sizeof(c->session), "%s", id);
    c->event = event;
    c->priority = priority;
    c->expires =
        d->lease_deadline < now + 2000 ? d->lease_deadline : now + 2000;
    return RG_CALL;
  }
  return RG_ERROR;
}
rg_result rg_receive(rg_device *d, const char *json, size_t length,
                     uint64_t now) {
  if (!json || !length || length > RG_MAX_FRAME)
    return RG_ERROR;
  const char *end = NULL;
  cJSON *j = cJSON_ParseWithLengthOpts(json, length, &end, false);
  if (!j)
    return RG_ERROR;
  while (end < json + length && isspace((unsigned char)*end))
    end++;
  if (end != json + length) {
    cJSON_Delete(j);
    return RG_ERROR;
  }
  rg_result result = accept(d, j, now);
  cJSON_Delete(j);
  return result;
}
bool rg_pop_call(rg_device *d, uint64_t now, rg_call *out) {
  rg_expire(d, now);
  size_t best = 16;
  for (size_t i = 0; i < d->call_count;) {
    if (d->calls[i].expires <= now) {
      memmove(&d->calls[i], &d->calls[i + 1],
              (--d->call_count - i) * sizeof(rg_call));
      continue;
    }
    i++;
  }
  for (size_t i = 0; i < d->call_count; i++)
    if (best == 16 || d->calls[i].priority < d->calls[best].priority ||
        (d->calls[i].priority == d->calls[best].priority &&
         d->calls[i].event < d->calls[best].event))
      best = i;
  if (best == 16)
    return false;
  *out = d->calls[best];
  memmove(&d->calls[best], &d->calls[best + 1],
          (--d->call_count - best) * sizeof(rg_call));
  return true;
}
bool rg_decoder_init(rg_decoder *d) {
  memset(d, 0, sizeof(*d));
  d->bytes = malloc(RG_MAX_FRAME + 9);
  return d->bytes != NULL;
}
void rg_decoder_clear(rg_decoder *d) {
  free(d->bytes);
  memset(d, 0, sizeof(*d));
}
bool rg_feed(rg_decoder *d, const uint8_t *bytes, size_t n,
             bool (*consume)(const char *, size_t, void *), void *context) {
  if (!d->bytes)
    return false;
  for (size_t i = 0; i < n; i++) {
    if (d->used >= RG_MAX_FRAME + 8)
      return false;
    d->bytes[d->used++] = bytes[i];
    if (d->used == 8) {
      if (memcmp(d->bytes, "RGP1", 4))
        return false;
      d->want = (uint32_t)d->bytes[4] | ((uint32_t)d->bytes[5] << 8) |
                ((uint32_t)d->bytes[6] << 16) | ((uint32_t)d->bytes[7] << 24);
      if (!d->want || d->want > RG_MAX_FRAME)
        return false;
    }
    if (d->used >= 8 && d->used == d->want + 8) {
      d->bytes[d->used] = 0;
      if (!consume((const char *)d->bytes + 8, d->want, context))
        return false;
      d->used = d->want = 0;
    }
  }
  return true;
}
