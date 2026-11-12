#include "idle_screen.h"

void rg_idle_screen_init(rg_idle_screen *screen, uint64_t now) {
  *screen = (rg_idle_screen){.deadline = now + 30000};
}

bool rg_idle_screen_update(rg_idle_screen *screen, uint64_t now, bool busy,
                           bool touched) {
  if (touched || (screen->busy && !busy))
    screen->deadline = now + 30000;
  screen->busy = busy;
  return busy || now < screen->deadline;
}
