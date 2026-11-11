#include "ui.h"
#include "assets.h"
#include "bsp/display.h"
#include "device.h"
#include "health.h"
#include "lvgl.h"
#include "src/misc/cache/instance/lv_image_header_cache.h"
#include "src/misc/cache/instance/lv_image_cache.h"
#include "schedule.h"
#include <math.h>
#include <stdatomic.h>
#include <stdio.h>
#include <string.h>

// 唯一写入者是 LVGL 任务或持有 BSP 显示锁的调用者。
static cJSON *model;
static rg_schedule schedule;
static lv_obj_t *screen, *business, *waiting, *hero_name, *hero_state,
    *hero_bubble, *bubble_image, *hero_image, *footer;
static lv_obj_t *names[5], *states[5], *marks[5], *overview_title, *usage_title,
    *usage_values[2], *arcs[2], *percent;
static lv_font_t *font20, *font28;
static lv_image_dsc_t image_dsc;
static lv_image_dsc_t bubble_dsc[5];
static bool active;
static atomic_uint_least32_t rendered;
static uint64_t epoch, last_text;
static size_t page;
static int usage_item, usage_pair;
static char usage_id[193];
static uint64_t usage_since;
extern const uint8_t font20_start[] asm("_binary_font20_bin_start");
extern const uint8_t font20_end[] asm("_binary_font20_bin_end");
extern const uint8_t font28_start[] asm("_binary_font28_bin_start");
extern const uint8_t font28_end[] asm("_binary_font28_bin_end");
static const char *state_names[] = {"空闲",     "运行中", "待确认",
                                    "异常中止", "轮到你", "已完成"};
static const char *state_keys[] = {"idle",    "running",   "awaiting",
                                   "aborted", "your_turn", "completed"};
static const uint32_t state_colors[] = {0xa8adb5, 0xffc53d, 0xff4d4f,
                                        0xf17b68, 0x4096ff, 0x52c41a};
static int state_index(const char *s) {
  for (int i = 0; i < 6; i++)
    if (!strcmp(s, state_keys[i]))
      return i;
  return 0;
}
static int priority(int state) {
  return state == 2 ? 0 : state == 3 ? 1 : state == 4 ? 2 : 4;
}
static const cJSON *sessions(void) {
  return cJSON_GetObjectItemCaseSensitive(model, "sessions");
}
static lv_obj_t *label(lv_obj_t *p, int x, int y, int w, const lv_font_t *f) {
  lv_obj_t *o = lv_label_create(p);
  lv_obj_set_pos(o, x, y);
  lv_obj_set_width(o, w);
  lv_obj_set_style_text_font(o, f, 0);
  lv_obj_set_style_text_color(o, lv_color_hex(0xf5f6f8), 0);
  lv_label_set_long_mode(o, LV_LABEL_LONG_SCROLL_CIRCULAR);
  lv_obj_set_style_anim_duration(o, 6000, 0);
  lv_label_set_text(o, "");
  return o;
}
static void set_text(lv_obj_t *o, const char *t) {
  if (strcmp(lv_label_get_text(o), t))
    lv_label_set_text(o, t);
}
static void ready(lv_event_t *event) {
  (void)event;
  atomic_store(&rendered, (uint32_t)rg_now());
}
static bool glyphs(const lv_font_t *f, const char *s) {
  const unsigned char *p = (const unsigned char *)s;
  while (*p) {
    uint32_t cp = *p++;
    int n = 0;
    if (cp >= 0xf0) {
      cp &= 7;
      n = 3;
    } else if (cp >= 0xe0) {
      cp &= 15;
      n = 2;
    } else if (cp >= 0xc0) {
      cp &= 31;
      n = 1;
    }
    while (n--) {
      if (!*p)
        return false;
      cp = (cp << 6) | (*p++ & 63);
    }
    lv_font_glyph_dsc_t d = {0};
    if (!lv_font_get_glyph_dsc(f, &d, cp, 0) || d.is_placeholder)
      return false;
  }
  return true;
}
static bool supported(const cJSON *j) {
  if (cJSON_GetArraySize(cJSON_GetObjectItemCaseSensitive(j, "sessions")) > 32)
    return false;
  const cJSON *p, *w;
  cJSON_ArrayForEach(
      p, cJSON_GetObjectItemCaseSensitive(
             j, "sessions")) if (!glyphs(font20, rg_string(p, "name")) ||
                                 !glyphs(font28,
                                         rg_string(p, "name"))) return false;
  cJSON_ArrayForEach(p, cJSON_GetObjectItemCaseSensitive(j, "usage")) {
    if (!glyphs(font20, rg_string(p, "label")))
      return false;
    cJSON_ArrayForEach(
        w,
        cJSON_GetObjectItemCaseSensitive(
            p,
            "windows")) if (!glyphs(font20, rg_string(w, "duration_label")) ||
                            !glyphs(font20,
                                    rg_string(w, "reset_label"))) return false;
  }
  return true;
}
static const rg_sprite *sprite(const cJSON *p) {
  for (size_t i = 0; i < 18; i++)
    if (!strcmp(rg_sprites[i].skin, rg_string(p, "skin")) &&
        !strcmp(rg_sprites[i].state, rg_string(p, "state")))
      return &rg_sprites[i];
  return &rg_sprites[0];
}
static void animate(const cJSON *p, uint64_t now) {
  const rg_sprite *s = sprite(p);
  const rg_frame *f =
      &s->frames[s->interval ? (now / s->interval) % s->count : 0];
  if (image_dsc.data != f->pixels) {
    lv_image_cache_drop(&image_dsc);
    lv_image_header_cache_drop(&image_dsc);
    image_dsc = (lv_image_dsc_t){.header = {.magic = LV_IMAGE_HEADER_MAGIC,
                                            .cf = LV_COLOR_FORMAT_ARGB8888,
                                            .w = f->w,
                                            .h = f->h,
                                            .stride = f->w * 4},
                                 .data_size = f->w * f->h * 4,
                                 .data = f->pixels};
    lv_image_set_src(hero_image, &image_dsc);
  }
  float x = 0, y = 0, sx = 1, sy = 1, angle = 0;
  if (s->motion_count > 1) {
    float phase = (float)(now % s->period) * 100 / s->period;
    for (size_t i = 1; i < s->motion_count; i++)
      if (phase <= s->motion[i].at) {
        rg_motion a = s->motion[i - 1], b = s->motion[i];
        float t = (phase - a.at) / (b.at - a.at);
        if (!strcmp(s->state, "running"))
          t = 0; // 桌面的 steps(2) 敲击节奏。
        x = a.x + (b.x - a.x) * t;
        y = a.y + (b.y - a.y) * t;
        sx = a.sx + (b.sx - a.sx) * t;
        sy = a.sy + (b.sy - a.sy) * t;
        angle = a.angle + (b.angle - a.angle) * t;
        break;
      }
  }
  const int scale = 9;
  lv_image_set_scale_x(hero_image, (uint32_t)(256 * scale * sx));
  lv_image_set_scale_y(hero_image, (uint32_t)(256 * scale * sy));
  lv_image_set_pivot(hero_image, f->w / 2, f->h / 2);
  lv_image_set_rotation(hero_image, (int32_t)(angle * 10));
  lv_obj_set_pos(hero_image, 16 + (269 - f->w) / 2 + (int)x,
                 282 - f->h / 2 + (int)y);
}
static void draw_bubble(int state, uint64_t now) {
  if (state == 1) {
    lv_obj_add_flag(bubble_image, LV_OBJ_FLAG_HIDDEN);
    lv_obj_remove_flag(hero_bubble, LV_OBJ_FLAG_HIDDEN);
    set_text(hero_bubble, (now / 15000) % 2 ? "Thinking…" : "Pondering…");
    return;
  }
  lv_obj_add_flag(hero_bubble, LV_OBJ_FLAG_HIDDEN);
  for (size_t i = 0; i < 5; i++) {
    if (!strcmp(rg_bubbles[i].state, state_keys[state])) {
      const rg_frame *f = &rg_bubbles[i].frame;
      lv_image_set_src(bubble_image, &bubble_dsc[i]);
      lv_obj_set_pos(bubble_image, 24 + (253 - f->w) / 2, 146);
      lv_obj_remove_flag(bubble_image, LV_OBJ_FLAG_HIDDEN);
      return;
    }
  }
}
static int usage_pages(const cJSON *u) {
  int n = cJSON_GetArraySize(cJSON_GetObjectItemCaseSensitive(u, "windows"));
  return n > 2 ? (n + 1) / 2 : 1;
}
static void draw_usage(uint64_t now) {
  const cJSON *groups = cJSON_GetObjectItemCaseSensitive(model, "usage"), *g;
  int count = 0, current = -1;
  cJSON_ArrayForEach(g, groups) {
    if (!strcmp(usage_id, rg_string(g, "id"))) {
      current = count;
      if (usage_pair >= usage_pages(g))
        usage_pair = 0;
    }
    count += usage_pages(g);
  }
  if (!count) {
    set_text(usage_title, "暂无用量");
    for (int i = 0; i < 2; i++) {
      set_text(usage_values[i], "");
      lv_arc_set_value(arcs[i], 0);
    }
    set_text(percent, "--");
    return;
  }
  if (current < 0) {
    usage_item = 0;
    usage_pair = 0;
    usage_since = now;
  } else {
    usage_item = (current + usage_pair) % count;
    if (now - usage_since >= 8000) {
      usage_item = (usage_item + 1) % count;
      usage_since = now;
    }
  }
  int at = usage_item % count;
  g = NULL;
  const cJSON *item;
  cJSON_ArrayForEach(item, groups) {
    int n = usage_pages(item);
    if (at < n) {
      g = item;
      break;
    }
    at -= n;
  }
  if (!g)
    return;
  usage_pair = at;
  snprintf(usage_id, sizeof(usage_id), "%s", rg_string(g, "id"));
  char title[384];
  snprintf(title, sizeof(title), "%s · %s%s%s",
           !strcmp(rg_string(g, "source"), "codex") ? "Codex" : "Claude",
           rg_string(g, "label"), *rg_string(g, "updated_label") ? " · " : "",
           rg_string(g, "updated_label"));
  set_text(usage_title, title);
  const cJSON *windows = cJSON_GetObjectItemCaseSensitive(g, "windows");
  for (int i = 0; i < 2; i++) {
    const cJSON *w = cJSON_GetArrayItem(windows, at * 2 + i),
                *v = cJSON_GetObjectItemCaseSensitive(w, "used_percentage");
    bool known = cJSON_IsNumber(v);
    int pct = known ? (int)round(v->valuedouble) : 0;
    lv_arc_set_value(arcs[i], pct);
    lv_obj_set_style_arc_color(arcs[i],
                               lv_color_hex(pct >= 90   ? 0xff4d4f
                                            : pct >= 70 ? 0xffc53d
                                                        : 0x52c41a),
                               LV_PART_INDICATOR);
    char value[160], number[12];
    if (known)
      snprintf(number, sizeof(number), "%d%%", pct);
    else
      snprintf(number, sizeof(number), "%s",
               !strcmp(rg_string(w, "status"), "expired") ? "过期" : "未知");
    if (w)
      snprintf(value, sizeof(value), "%s  %s\n重置 %s",
               rg_string(w, "duration_label"), number,
               *rg_string(w, "reset_label") ? rg_string(w, "reset_label")
                                            : "--");
    else
      value[0] = 0;
    set_text(usage_values[i], value);
    if (i == 0)
      set_text(percent, known ? number : "--");
    if (w)
      lv_obj_remove_flag(arcs[i], LV_OBJ_FLAG_HIDDEN);
    else
      lv_obj_add_flag(arcs[i], LV_OBJ_FLAG_HIDDEN);
  }
}
static void refresh(lv_timer_t *timer) {
  (void)timer;
  uint64_t now = rg_now();
  // 等待画面也周期刷新，健康取自真正结束的刷新回调。
  lv_obj_invalidate(screen);
  if (!active || !model || !schedule.count)
    return;
  rg_schedule_tick(&schedule, now);
  const cJSON *p = cJSON_GetArrayItem(sessions(), (int)schedule.focus);
  animate(p, now);
  if (now - last_text < 100)
    return;
  last_text = now;
  size_t pages = (schedule.count + 4) / 5;
  page = ((now - epoch) / 8000) % pages;
  char text[128];
  snprintf(text, sizeof(text), "总览 %zu/%zu", page + 1, pages);
  set_text(overview_title, text);
  for (size_t i = 0; i < 5; i++) {
    const cJSON *row = cJSON_GetArrayItem(sessions(), (int)(page * 5 + i));
    int state = state_index(rg_string(row, "state"));
    set_text(names[i], row ? rg_string(row, "name") : "");
    set_text(states[i], row ? state_names[state] : "");
    lv_obj_set_style_text_color(states[i], lv_color_hex(state_colors[state]),
                                0);
    set_text(marks[i],
             row && !strcmp(rg_string(row, "id"), rg_schedule_focus(&schedule))
                 ? ">"
                 : "");
  }
  int state = state_index(rg_string(p, "state"));
  set_text(hero_name, rg_string(p, "name"));
  set_text(hero_state, state_names[state]);
  lv_obj_set_style_text_color(hero_state, lv_color_hex(state_colors[state]), 0);
  draw_bubble(state, now);
  int pending = 0;
  for (size_t i = 0; i < schedule.count; i++)
    if (schedule.sessions[i].priority <= 2)
      pending++;
  snprintf(text, sizeof(text), "%s · %zu/%zu · 待处理 %d",
           !strcmp(rg_string(p, "source"), "codex") ? "Codex" : "Claude",
           schedule.focus + 1, schedule.count, pending);
  set_text(footer, text);
  draw_usage(now);
}
bool rg_ui_init(void) {
  if (!bsp_display_lock(1000))
    return false;
  font20 = lv_binfont_create_from_buffer((void *)font20_start,
                                         font20_end - font20_start);
  font28 = lv_binfont_create_from_buffer((void *)font28_start,
                                         font28_end - font28_start);
  if (!font20 || !font28) {
    bsp_display_unlock();
    return false;
  }
  // Noto 全字集携带极端字形的行距；屏幕采用明确的中文正文行盒。
  font20->line_height = 24;
  font20->base_line = 4;
  font28->line_height = 36;
  font28->base_line = 7;
  screen = lv_screen_active();
  lv_obj_set_style_bg_color(screen, lv_color_hex(0x161b22), 0);
  business = lv_obj_create(screen);
  lv_obj_remove_style_all(business);
  lv_obj_set_size(business, 480, 480);
  lv_obj_remove_flag(business, LV_OBJ_FLAG_SCROLLABLE);
  lv_obj_add_flag(business, LV_OBJ_FLAG_HIDDEN);
  waiting = label(screen, 32, 215, 416, font28);
  lv_obj_set_style_text_align(waiting, LV_TEXT_ALIGN_CENTER, 0);
  set_text(waiting, "等待电脑连接");
  hero_name = label(business, 24, 365, 253, font28);
  hero_state = label(business, 24, 407, 253, font20);
  hero_bubble = label(business, 24, 144, 253, font28);
  lv_obj_set_style_text_align(hero_name, LV_TEXT_ALIGN_CENTER, 0);
  lv_obj_set_style_text_align(hero_state, LV_TEXT_ALIGN_CENTER, 0);
  lv_obj_set_style_text_align(hero_bubble, LV_TEXT_ALIGN_CENTER, 0);
  bubble_image = lv_image_create(business);
  lv_obj_add_flag(bubble_image, LV_OBJ_FLAG_HIDDEN);
  for (size_t i = 0; i < 5; i++) {
    const rg_frame *f = &rg_bubbles[i].frame;
    bubble_dsc[i] = (lv_image_dsc_t){
        .header = {.magic = LV_IMAGE_HEADER_MAGIC,
                   .cf = LV_COLOR_FORMAT_ARGB8888,
                   .w = f->w, .h = f->h, .stride = f->w * 4},
        .data_size = f->w * f->h * 4, .data = f->pixels};
  }
  hero_image = lv_image_create(business);
  lv_image_set_antialias(hero_image, false);
  overview_title = label(business, 297, 146, 167, font20);
  for (int i = 0; i < 5; i++) {
    marks[i] = label(business, 293, 180 + i * 52, 14, font20);
    names[i] = label(business, 309, 176 + i * 52, 155, font20);
    states[i] = label(business, 309, 201 + i * 52, 155, font20);
  }
  footer = label(business, 16, 449, 448, font20);
  usage_title = label(business, 148, 12, 316, font20);
  usage_values[0] = label(business, 148, 40, 316, font20);
  usage_values[1] = label(business, 148, 88, 316, font20);
  for (int i = 0; i < 2; i++) {
    arcs[i] = lv_arc_create(business);
    lv_obj_set_pos(arcs[i], 16 + i * 16, 12 + i * 16);
    lv_obj_set_size(arcs[i], 116 - i * 32, 116 - i * 32);
    lv_arc_set_bg_angles(arcs[i], 0, 360);
    lv_arc_set_rotation(arcs[i], 270);
    lv_arc_set_range(arcs[i], 0, 100);
    lv_arc_set_value(arcs[i], 0);
    lv_obj_remove_style(arcs[i], NULL, LV_PART_KNOB);
    lv_obj_remove_flag(arcs[i], LV_OBJ_FLAG_CLICKABLE);
    lv_obj_set_style_arc_width(arcs[i], 8, LV_PART_MAIN);
    lv_obj_set_style_arc_width(arcs[i], 8, LV_PART_INDICATOR);
    lv_obj_set_style_arc_color(arcs[i], lv_color_hex(0x303942), LV_PART_MAIN);
  }
  percent = label(business, 31, 54, 86, font20);
  lv_obj_set_style_text_align(percent, LV_TEXT_ALIGN_CENTER, 0);
  epoch = usage_since = rg_now();
  lv_timer_create(refresh, 33, NULL);
  lv_display_add_event_cb(lv_display_get_default(), ready, LV_EVENT_REFR_READY,
                          NULL);
  bsp_display_unlock();
  return true;
}
bool rg_ui_apply(const cJSON *snapshot) {
  if (!bsp_display_lock(100))
    return false;
  bool ok = supported(snapshot);
  cJSON *copy = ok ? cJSON_Duplicate(snapshot, true) : NULL;
  if (copy) {
    cJSON_Delete(model);
    model = copy;
    rg_session items[32] = {0};
    size_t n = 0;
    const cJSON *p;
    cJSON_ArrayForEach(p, sessions()) {
      snprintf(items[n].id, sizeof(items[n].id), "%s", rg_string(p, "id"));
      items[n++].priority = priority(state_index(rg_string(p, "state")));
    }
    rg_schedule_update(&schedule, items, n, rg_now());
    last_text = 0;
    if (active) {
      if (n) {
        lv_obj_remove_flag(business, LV_OBJ_FLAG_HIDDEN);
        lv_obj_add_flag(waiting, LV_OBJ_FLAG_HIDDEN);
      } else {
        lv_obj_add_flag(business, LV_OBJ_FLAG_HIDDEN);
        lv_obj_remove_flag(waiting, LV_OBJ_FLAG_HIDDEN);
        set_text(waiting, "暂无会话");
      }
    }
  }
  bsp_display_unlock();
  return copy != NULL;
}
bool rg_ui_active(bool value) {
  if (!bsp_display_lock(40))
    return false;
  active = value;
  if (active && schedule.count) {
    lv_obj_remove_flag(business, LV_OBJ_FLAG_HIDDEN);
    lv_obj_add_flag(waiting, LV_OBJ_FLAG_HIDDEN);
  } else {
    lv_obj_add_flag(business, LV_OBJ_FLAG_HIDDEN);
    lv_obj_remove_flag(waiting, LV_OBJ_FLAG_HIDDEN);
    set_text(waiting, active ? "暂无会话" : "等待电脑连接");
  }
  bsp_display_unlock();
  return true;
}
bool rg_ui_healthy(uint32_t now) {
  uint32_t stamp = atomic_load(&rendered);
  return rg_recent(now, stamp);
}
