#include "schedule.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>

static void complete_round_before_switch(void) {
  rg_schedule s = {0};
  rg_session items[] = {{"a", 4}, {"b", 4}};
  // 跨过 32 位毫秒边界；快照、排序、优先级变化都不截断当前轮次。
  uint64_t start = UINT64_C(4294966000);
  rg_schedule_update(&s, items, 2, start);
  assert(!strcmp(rg_schedule_focus(&s), "a"));
  items[1].priority = 0;
  rg_session reordered[] = {items[1], items[0]};
  for (unsigned elapsed = 100; elapsed < 6000; elapsed += 100) {
    rg_schedule_update(&s, elapsed % 200 ? items : reordered, 2, start + elapsed);
    assert(!rg_schedule_tick(&s, start + elapsed));
    assert(!strcmp(rg_schedule_focus(&s), "a"));
    assert(s.since == start);
  }
  assert(!rg_schedule_tick(&s, start + 5999));
  assert(rg_schedule_tick(&s, start + 6000));
  assert(!strcmp(rg_schedule_focus(&s), "b"));
  assert(s.since == start + 6000);
}

static void fair_without_repeating(void) {
  rg_schedule s = {0};
  rg_session items[] = {{"a", 0}, {"b", 4}, {"c", 4}, {"d", 4}, {"e", 4}};
  rg_schedule_update(&s, items, 5, 0);
  bool seen[5] = {true};
  unsigned priority_rounds = 1;
  for (unsigned round = 1; round <= 24; round++) {
    char previous = rg_schedule_focus(&s)[0];
    assert(rg_schedule_tick(&s, round * 6000));
    char current = rg_schedule_focus(&s)[0];
    assert(current != previous);
    seen[current - 'a'] = true;
    priority_rounds += current == 'a';
  }
  for (int i = 0; i < 5; i++)
    assert(seen[i]);
  assert(priority_rounds >= 8); // 优先仍有效，同时不给普通会话造成饥饿。
}

static void removal_and_single_session(void) {
  rg_schedule s = {0};
  rg_session items[] = {{"a", 0}, {"b", 4}};
  rg_schedule_update(&s, items, 2, 0);
  rg_schedule_update(&s, items + 1, 1, 2500);
  assert(!strcmp(rg_schedule_focus(&s), "b"));
  assert(s.since == 2500);
  assert(!rg_schedule_tick(&s, 8499));
  assert(rg_schedule_tick(&s, 8500));
  assert(!strcmp(rg_schedule_focus(&s), "b"));
  assert(s.since == 8500);
  rg_schedule_update(&s, NULL, 0, 8600);
  assert(!*rg_schedule_focus(&s));
  assert(!rg_schedule_tick(&s, 15000));
  rg_schedule_update(&s, items + 1, 1, 17000);
  assert(!strcmp(rg_schedule_focus(&s), "b"));
  assert(s.since == 17000);
  assert(!rg_schedule_tick(&s, 22999));
  assert(rg_schedule_tick(&s, 23000));
}

int main(void) {
  complete_round_before_switch();
  fair_without_repeating();
  removal_and_single_session();
  puts("schedule: 六秒完整轮次、快照保相位、优先与公平、不连选、删除和单会话通过");
}
