#include "hero.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>

static void frame(rg_hero *h, const rg_schedule *s, uint64_t now,
                  const char *id, unsigned opacity) {
  rg_hero_frame f = rg_hero_update(h, s, now);
  assert(!strcmp(f.id, id));
  assert(f.opacity == opacity);
}
static void sync_rounds(void) {
  rg_schedule s = {0};
  rg_hero h = {0};
  rg_session items[] = {{"a", 0}, {"b", 4}};
  uint64_t start = UINT64_C(4294966000);
  rg_schedule_update(&s, items, 2, start);
  frame(&h, &s, start, "a", 0); // 没有旧宠物，首次等待入场。
  frame(&h, &s, start + 500, "a", 0);
  frame(&h, &s, start + 750, "a", 127);
  frame(&h, &s, start + 1000, "a", 255);
  assert(rg_schedule_tick(&s, start + 6000));
  frame(&h, &s, start + 6000, "a", 255);
  rg_session reordered[] = {items[1], items[0]};
  rg_schedule_update(&s, reordered, 2, start + 6250);
  frame(&h, &s, start + 6250, "a", 127);
  frame(&h, &s, start + 6500, "b", 0);
  rg_schedule_update(&s, items, 2, start + 6750);
  frame(&h, &s, start + 6750, "b", 127);
  frame(&h, &s, start + 7000, "b", 255);
  frame(&h, &s, start + 9500, "b", 255); // 第二次仅色块隐藏。
  assert(rg_focus_opacity(9500) == 0);
  assert(rg_schedule_tick(&s, start + 12000));
  frame(&h, &s, start + 14000, "a", 255); // 跳帧越过整个过渡。
}
static void removal_and_reset(void) {
  rg_schedule s = {0};
  rg_hero h = {0};
  rg_session items[] = {{"a", 0}, {"b", 4}};
  rg_schedule_update(&s, items, 2, 0);
  frame(&h, &s, 1000, "a", 255);
  assert(rg_schedule_tick(&s, 6000));
  frame(&h, &s, 6100, "a", rg_focus_opacity(100));
  rg_schedule_update(&s, items + 1, 1, 6200); // 删除退场对象，不能保留旧 ID。
  frame(&h, &s, 6200, "b", 0);
  frame(&h, &s, 6750, "b", 127);
  frame(&h, &s, 7000, "b", 255);
  assert(rg_schedule_tick(&s, 12000));
  frame(&h, &s, 12000, "b", 255);
  frame(&h, &s, 12500, "b", 255); // 单只不能退场。
  rg_hero_reset(&h); // 授权失效，同 ID 重连不能沿用旧过渡。
  rg_schedule_update(&s, NULL, 0, 13000);
  frame(&h, &s, 13000, "", 255);
  rg_schedule_update(&s, items + 1, 1, 14000);
  frame(&h, &s, 14000, "b", 0);
  frame(&h, &s, 15000, "b", 255);

  rg_schedule_update(&s, items, 2, 16000);
  assert(rg_schedule_tick(&s, 20000));
  frame(&h, &s, 20100, "b", rg_focus_opacity(100));
  rg_schedule_update(&s, items + 1, 1, 20200); // 删除待入场对象，继续显示现存旧主角。
  frame(&h, &s, 20200, "b", 255);
  rg_schedule_update(&s, NULL, 0, 20300);
  frame(&h, &s, 20300, "", 255);
}
int main(void) {
  unsigned samples[] = {0, 250, 500, 750, 1000, 2999};
  unsigned opacity[] = {255, 127, 0, 127, 255, 255};
  for (unsigned i = 0; i < sizeof(samples) / sizeof(samples[0]); i++) {
    assert(rg_focus_opacity(samples[i]) == opacity[i]);
    assert(rg_focus_opacity(samples[i] + 3000) == opacity[i]);
  }
  sync_rounds();
  removal_and_reset();
  puts("hero: 同步透明度、中点换 ID、重排、删除、单只、重连与大时间通过");
}
