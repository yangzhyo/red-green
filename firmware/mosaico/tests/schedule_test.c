#include "schedule.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>
int main(void) {
  rg_schedule s = {0};
  rg_session a[5] = {{"a", 0}, {"b", 4}, {"c", 4}, {"d", 4}, {"e", 4}};
  rg_schedule_update(&s, a, 5, 0);
  assert(!strcmp(rg_schedule_focus(&s), "a"));
  bool seen[5] = {0};
  for (unsigned t = 0; t <= 80000; t += 100) {
    if (t % 1500 == 0)
      rg_schedule_update(&s, a, 5, t);
    rg_schedule_tick(&s, t);
    seen[rg_schedule_focus(&s)[0] - 'a'] = true;
  }
  for (int i = 0; i < 5; i++)
    assert(seen[i]);
  rg_schedule empty = {0};
  a[0].priority = 4;
  rg_schedule_update(&empty, a, 5, 0);
  a[1].priority = 0;
  rg_schedule_update(&empty, a, 5, 100);
  rg_schedule_tick(&empty, 999);
  assert(!strcmp(rg_schedule_focus(&empty), "a"));
  rg_schedule_tick(&empty, 1000);
  assert(!strcmp(rg_schedule_focus(&empty), "b"));
  rg_schedule_update(&empty, a + 2, 3, 1100);
  assert(strchr("cde", rg_schedule_focus(&empty)[0]));
  rg_schedule_update(&empty, NULL, 0, 1200);
  assert(!*rg_schedule_focus(&empty));
  puts("schedule: 持续优先状态不饿死普通会话，抢占遵守最短停留");
}
