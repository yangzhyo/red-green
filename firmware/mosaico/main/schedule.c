#include "schedule.h"
#include <stdio.h>
#include <string.h>
static size_t choose(rg_schedule *s) {
  if (++s->turn % 4 == 0)
    return s->normal_cursor++ % s->count;
  int best = 5;
  for (size_t i = 0; i < s->count; i++)
    if (s->sessions[i].priority < best)
      best = s->sessions[i].priority;
  for (size_t i = 0; i < s->count; i++) {
    size_t n = (s->priority_cursor + i) % s->count;
    if (s->sessions[n].priority == best) {
      s->priority_cursor = n + 1;
      return n;
    }
  }
  return 0;
}
const char *rg_schedule_focus(const rg_schedule *s) {
  return s->count ? s->sessions[s->focus].id : "";
}
void rg_schedule_update(rg_schedule *s, const rg_session *items, size_t n,
                        uint64_t now) {
  if (n > 32)
    return;
  char previous[33];
  snprintf(previous, sizeof(previous), "%s", rg_schedule_focus(s));
  for (size_t i = 0; i < n; i++) {
    bool changed = true;
    for (size_t j = 0; j < s->count; j++)
      if (!strcmp(items[i].id, s->sessions[j].id))
        changed = items[i].priority < s->sessions[j].priority;
    if (changed && items[i].priority <= 2 && !*s->pending)
      snprintf(s->pending, sizeof(s->pending), "%s", items[i].id);
  }
  if (n)
    memcpy(s->sessions, items, n * sizeof(*items));
  s->count = n;
  if (!n) {
    s->focus = 0;
    *s->pending = 0;
    return;
  }
  for (size_t i = 0; i < n; i++)
    if (!strcmp(previous, items[i].id)) {
      s->focus = i;
      return;
    }
  s->focus = choose(s);
  s->since = now;
  *s->pending = 0;
}
bool rg_schedule_tick(rg_schedule *s, uint64_t now) {
  if (!s->count)
    return false;
  if (*s->pending && now - s->since >= 1000) {
    for (size_t i = 0; i < s->count; i++)
      if (!strcmp(s->pending, s->sessions[i].id)) {
        s->focus = i;
        s->since = now;
        *s->pending = 0;
        return true;
      }
    *s->pending = 0;
  }
  if (now - s->since < 4000)
    return false;
  s->focus = choose(s);
  s->since = now;
  return true;
}
