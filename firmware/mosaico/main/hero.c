#include "hero.h"
#include <stdio.h>
#include <string.h>

uint8_t rg_focus_opacity(uint64_t elapsed) {
  unsigned phase = elapsed % RG_FOCUS_BLINK_MS;
  if (phase >= 1000)
    return 255;
  float t = (float)(phase < 500 ? phase : phase - 500) / 500;
  float eased = t * t * (3 - 2 * t);
  return (uint8_t)(255 * (phase < 500 ? 1 - eased : eased));
}
void rg_hero_reset(rg_hero *h) { memset(h, 0, sizeof(*h)); }

rg_hero_frame rg_hero_update(rg_hero *h, const rg_schedule *s, uint64_t now) {
  if (!s->count) {
    rg_hero_reset(h);
    return (rg_hero_frame){h->current, 255};
  }
  const char *target = rg_schedule_focus(s);
  bool exists = false;
  for (size_t i = 0; i < s->count; i++)
    if (!strcmp(h->current, s->sessions[i].id))
      exists = true;
  if (strcmp(h->target, target)) {
    snprintf(h->target, sizeof(h->target), "%s", target);
    h->since = s->since;
    h->transitioning = strcmp(h->current, target) != 0;
    h->entering = !exists;
  }
  if (!exists) {
    // 不持有旧快照或已删除的会话；只让仍存在的目标入场。
    snprintf(h->current, sizeof(h->current), "%s", target);
    h->entering = true;
  }
  uint64_t age = now >= h->since ? now - h->since : 0;
  if (h->transitioning && age >= 500)
    snprintf(h->current, sizeof(h->current), "%s", target);
  if (age >= 1000)
    h->transitioning = false;
  uint8_t opacity = h->transitioning
                        ? (h->entering && age < 500 ? 0 : rg_focus_opacity(age))
                        : 255;
  return (rg_hero_frame){h->current, opacity};
}
