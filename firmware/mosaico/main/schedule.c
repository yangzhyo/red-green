#include "schedule.h"
#include <stdio.h>
#include <string.h>
static size_t choose(rg_schedule *s, bool advance) {
  // 多会话时，每个完整轮次之后都换一只；初次选择/删除重选不排除索引。
  bool skip_current = advance && s->count > 1;
  if (++s->turn % 4 == 0) {
    size_t n = s->normal_cursor % s->count;
    if (skip_current && n == s->focus)
      n = (n + 1) % s->count;
    s->normal_cursor = n + 1;
    return n;
  }
  int best = 5;
  for (size_t i = 0; i < s->count; i++)
    if ((!skip_current || i != s->focus) && s->sessions[i].priority < best)
      best = s->sessions[i].priority;
  for (size_t i = 0; i < s->count; i++) {
    size_t n = (s->priority_cursor + i) % s->count;
    if ((!skip_current || n != s->focus) && s->sessions[n].priority == best) {
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
  if (n)
    memcpy(s->sessions, items, n * sizeof(*items));
  s->count = n;
  if (!n) {
    s->focus = 0;
    return;
  }
  for (size_t i = 0; i < n; i++)
    if (!strcmp(previous, items[i].id)) {
      s->focus = i;
      return;
    }
  s->focus = choose(s, false);
  s->since = now;
}
bool rg_schedule_tick(rg_schedule *s, uint64_t now) {
  if (!s->count)
    return false;
  if (now - s->since < RG_FOCUS_ROUND_MS)
    return false;
  s->focus = choose(s, true);
  s->since = now;
  return true;
}
