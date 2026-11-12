#include "idle_screen.h"
#include <assert.h>
#include <stdio.h>

static void exercise(uint64_t origin) {
  rg_idle_screen screen;
  rg_idle_screen_init(&screen, origin);
  assert(rg_idle_screen_update(&screen, origin, false, false));
  for (uint64_t t = 1000; t < 30000; t += 1000)
    assert(rg_idle_screen_update(&screen, origin + t, false, false));
  assert(rg_idle_screen_update(&screen, origin + 29999, false, false));
  assert(!rg_idle_screen_update(&screen, origin + 30000, false, false));
  assert(!rg_idle_screen_update(&screen, origin + 40000, false, false));

  assert(rg_idle_screen_update(&screen, origin + 50000, false, true));
  assert(rg_idle_screen_update(&screen, origin + 79999, false, false));
  assert(!rg_idle_screen_update(&screen, origin + 80000, false, false));

  // 再次触摸、持续按压以最后一次触摸为准。
  assert(rg_idle_screen_update(&screen, origin + 90000, false, true));
  assert(rg_idle_screen_update(&screen, origin + 110000, false, true));
  assert(rg_idle_screen_update(&screen, origin + 120000, false, false));
  assert(rg_idle_screen_update(&screen, origin + 139999, false, false));
  assert(!rg_idle_screen_update(&screen, origin + 140000, false, false));

  assert(rg_idle_screen_update(&screen, origin + 150000, true, false));
  assert(rg_idle_screen_update(&screen, origin + 250000, true, false));
  // 活动结束重新计算期限，不沿用活动前已过期的期限。
  assert(rg_idle_screen_update(&screen, origin + 260000, false, false));
  assert(rg_idle_screen_update(&screen, origin + 289999, false, false));
  assert(!rg_idle_screen_update(&screen, origin + 290000, false, false));
  // 边界时刻的新活动优先于熄屏。
  assert(rg_idle_screen_update(&screen, origin + 290000, true, false));
}

int main(void) {
  exercise(0);
  exercise(UINT64_C(0xffffffff) - 10000);
  puts("idle screen: 30 秒期限、重复输入、触摸续期及活动唤醒通过");
}
