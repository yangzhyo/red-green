#include "ui.h"
#include "assets.h"
#include "bsp/display.h"
#include "device.h"
#include "health.h"
#include "idle_screen.h"
#include "panel.h"
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
    *hero_bubble, *bubble_image, *hero_image, *balloon, *footer, *footer_meta;
static lv_obj_t *names[5], *states[5], *tiles[5], *header_count, *usage_title,
    *usage_page, *usage_labels[2], *usage_values[2], *usage_reset[2],
    *tracks[2], *fills[2];
static bool usage_unknown[2];
static lv_font_t *font16, *font20, *font28;
static lv_image_dsc_t image_dsc;
static lv_image_dsc_t bubble_dsc[5];
static bool active;
static atomic_uint_least32_t rendered;
static atomic_uint_least32_t progressed;
static atomic_bool dark, panel_failed;
static rg_idle_screen idle_screen;
static lv_display_t *display;
static lv_indev_t *input;
static bool touched;
static bool submitted;
static uint32_t frames;
static uint64_t epoch, last_text;
static size_t page;
static int usage_item, usage_pair;
static char usage_id[193];
static uint64_t usage_since;
static char focus_id[65];
static uint64_t focus_since;
static int current_tile = -1;
extern const uint8_t font16_start[] asm("_binary_font16_bin_start");
extern const uint8_t font16_end[] asm("_binary_font16_bin_end");
extern const uint8_t font20_start[] asm("_binary_font20_bin_start");
extern const uint8_t font20_end[] asm("_binary_font20_bin_end");
extern const uint8_t font28_start[] asm("_binary_font28_bin_start");
extern const uint8_t font28_end[] asm("_binary_font28_bin_end");
static const char *state_names[] = {"空闲",     "运行中", "待确认",
                                    "异常中止", "轮到你", "已完成"};
static const char *state_keys[] = {"idle",    "running",   "awaiting",
                                   "aborted", "your_turn", "completed"};
static const uint32_t state_colors[] = {0xacb2b9, 0xe7b42b, 0xff677e,
                                        0xf3896e, 0x54b2f1, 0x4bc695};
static const uint32_t tile_colors[] = {0x41474e, 0xe7b42b, 0xbf2942,
                                       0xa63c27, 0x086eb5, 0x087e56};
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
static lv_obj_t *box(lv_obj_t *parent, int x, int y, int w, int h,
                     uint32_t color) {
  lv_obj_t *o = lv_obj_create(parent);
  lv_obj_remove_style_all(o);
  lv_obj_set_pos(o, x, y);
  lv_obj_set_size(o, w, h);
  lv_obj_set_style_bg_color(o, lv_color_hex(color), 0);
  lv_obj_set_style_bg_opa(o, LV_OPA_COVER, 0);
  lv_obj_remove_flag(o, LV_OBJ_FLAG_SCROLLABLE);
  return o;
}
static void ready(lv_event_t *event) {
  if (lv_event_get_code(event) == LV_EVENT_FLUSH_FINISH) {
    submitted = true;
  } else if (submitted) {
    // REFR_READY 在无重绘时也会触发，不能单凭该事件续健康期限。
    submitted = false;
    frames++;
    atomic_store(&rendered, (uint32_t)rg_now());
  }
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
static void small_text(lv_obj_t *o, const char *text) {
  // 小字集只包含固定文案；动态内容用完整字体兜底，绝不显示缺字框。
  const lv_font_t *font = glyphs(font16, text) ? font16 : font20;
  if (lv_obj_get_style_text_font(o, 0) != font)
    lv_obj_set_style_text_font(o, font, 0);
  set_text(o, text);
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
    if (!glyphs(font20, rg_string(p, "label")) ||
        !glyphs(font20, rg_string(p, "updated_label")))
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
    lv_obj_set_size(balloon, 182, 36);
    lv_obj_set_pos(balloon, 59, 144);
    return;
  }
  lv_obj_add_flag(hero_bubble, LV_OBJ_FLAG_HIDDEN);
  for (size_t i = 0; i < 5; i++) {
    if (!strcmp(rg_bubbles[i].state, state_keys[state])) {
      const rg_frame *f = &rg_bubbles[i].frame;
      lv_image_set_src(bubble_image, &bubble_dsc[i]);
      lv_obj_set_size(balloon, f->w + 12, 36);
      lv_obj_set_pos(balloon, 150 - (f->w + 12) / 2, 144);
      lv_obj_remove_flag(bubble_image, LV_OBJ_FLAG_HIDDEN);
      return;
    }
  }
}
static void dashed_track(lv_event_t *event) {
  size_t i = (size_t)(uintptr_t)lv_event_get_user_data(event);
  if (!usage_unknown[i])
    return;
  lv_area_t area;
  lv_obj_get_coords(tracks[i], &area);
  lv_draw_rect_dsc_t dsc;
  lv_draw_rect_dsc_init(&dsc);
  dsc.bg_color = lv_color_hex(0x737373);
  for (int x = area.x1; x <= area.x2; x += 7) {
    int end = x + 3 < area.x2 ? x + 3 : area.x2;
    lv_area_t top = {x, area.y1, end, area.y1};
    lv_area_t bottom = {x, area.y2, end, area.y2};
    lv_draw_rect(lv_event_get_layer(event), &dsc, &top);
    lv_draw_rect(lv_event_get_layer(event), &dsc, &bottom);
  }
}
static void usage_bar(int i, bool known, int pct) {
  usage_unknown[i] = !known;
  lv_obj_set_style_bg_color(tracks[i], lv_color_hex(known ? 0x262626 : 0), 0);
  lv_obj_set_width(fills[i], known ? (432 * pct + 50) / 100 : 0);
  lv_obj_set_style_bg_color(fills[i],
      lv_color_hex(pct >= 85 ? 0xed526c : pct >= 60 ? 0xe7b42b : 0x35b982), 0);
  lv_obj_invalidate(tracks[i]);
}
static void tile_effect(uint64_t now) {
  uint32_t phase = (uint32_t)((now - focus_since) % 3000);
  float opacity = 1;
  if (phase >= 2400 && phase < 2600)
    opacity = 0;
  else if (phase >= 2000) {
    float t = (float)(phase < 2400 ? phase - 2000 : phase - 2600) / 400;
    float eased = t * t * (3 - 2 * t);
    opacity = phase < 2400 ? 1 - eased : eased;
  }
  for (int i = 0; i < 5; i++)
    lv_obj_set_style_opa(tiles[i], i == current_tile ? (lv_opa_t)(255 * opacity) : LV_OPA_COVER, 0);
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
    set_text(usage_page, "");
    for (int i = 0; i < 2; i++) {
      set_text(usage_labels[i], "");
      set_text(usage_values[i], "");
      set_text(usage_reset[i], "");
      lv_obj_add_flag(tracks[i], LV_OBJ_FLAG_HIDDEN);
    }
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
  char page_text[24];
  snprintf(page_text, sizeof(page_text), "%d/%d", usage_item + 1, count);
  set_text(usage_page, page_text);
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
    usage_bar(i, known, pct);
    char reset[160], number[16];
    if (known)
      snprintf(number, sizeof(number), "%d%%", pct);
    else
      snprintf(number, sizeof(number), "%s",
               !strcmp(rg_string(w, "status"), "expired") ? "过期" : "未知");
    snprintf(reset, sizeof(reset), "重置 %s",
             *rg_string(w, "reset_label") ? rg_string(w, "reset_label") : "--");
    small_text(usage_labels[i], w ? rg_string(w, "duration_label") : "");
    set_text(usage_values[i], w ? number : "");
    small_text(usage_reset[i], w ? reset : "");
    if (w)
      lv_obj_remove_flag(tracks[i], LV_OBJ_FLAG_HIDDEN);
    else
      lv_obj_add_flag(tracks[i], LV_OBJ_FLAG_HIDDEN);
  }
}
static void draw_frame(uint64_t now, bool force) {
  // 等待画面也周期刷新，健康取自真正结束的刷新回调。
  lv_obj_invalidate(screen);
  if (!active || !model || !schedule.count)
    return;
  rg_schedule_tick(&schedule, now);
  const cJSON *p = cJSON_GetArrayItem(sessions(), (int)schedule.focus);
  if (strcmp(focus_id, rg_string(p, "id"))) {
    snprintf(focus_id, sizeof(focus_id), "%s", rg_string(p, "id"));
    focus_since = now;
    last_text = 0;
  }
  animate(p, now);
  tile_effect(now);
  if (!force && now - last_text < 100)
    return;
  last_text = now;
  size_t pages = (schedule.count + 4) / 5;
  page = ((now - epoch) / 8000) % pages;
  char text[128];
  if (pages > 1)
    snprintf(text, sizeof(text), "%zu 个会话 · %zu/%zu", schedule.count, page + 1, pages);
  else
    snprintf(text, sizeof(text), "%zu 个会话", schedule.count);
  set_text(header_count, text);
  current_tile = -1;
  for (size_t i = 0; i < 5; i++) {
    const cJSON *row = cJSON_GetArrayItem(sessions(), (int)(page * 5 + i));
    int state = state_index(rg_string(row, "state"));
    set_text(names[i], row ? rg_string(row, "name") : "");
    set_text(states[i], row ? state_names[state] : "");
    if (row) {
      lv_obj_remove_flag(tiles[i], LV_OBJ_FLAG_HIDDEN);
      lv_obj_set_style_bg_color(tiles[i], lv_color_hex(tile_colors[state]), 0);
      lv_color_t ink = lv_color_hex(state == 1 ? 0x111111 : 0xffffff);
      lv_obj_set_style_text_color(names[i], ink, 0);
      lv_obj_set_style_text_color(states[i], ink, 0);
      if (!strcmp(rg_string(row, "id"), rg_schedule_focus(&schedule)))
        current_tile = (int)i;
    } else {
      lv_obj_add_flag(tiles[i], LV_OBJ_FLAG_HIDDEN);
    }
  }
  tile_effect(now);
  int state = state_index(rg_string(p, "state"));
  set_text(hero_name, rg_string(p, "name"));
  set_text(hero_state, state_names[state]);
  lv_obj_set_style_text_color(hero_state, lv_color_hex(state_colors[state]), 0);
  draw_bubble(state, now);
  int pending = 0;
  for (size_t i = 0; i < schedule.count; i++)
    if (schedule.sessions[i].priority <= 2)
      pending++;
  if (pending)
    snprintf(text, sizeof(text), "%d 个待处理", pending);
  else
    snprintf(text, sizeof(text), "所有会话正常");
  set_text(footer, text);
  snprintf(text, sizeof(text), "%s  %zu/%zu",
           !strcmp(rg_string(p, "source"), "codex") ? "Codex" : "Claude",
           schedule.focus + 1, schedule.count);
  set_text(footer_meta, text);
  draw_usage(now);
}
static void touch_event(lv_event_t *event) {
  (void)event;
  touched = true;
}
static bool has_activity(void) {
  if (!active)
    return false;
  const cJSON *p;
  cJSON_ArrayForEach(p, sessions()) {
    if (strcmp(rg_string(p, "state"), "idle"))
      return true;
  }
  return false;
}
static void refresh(lv_timer_t *timer) {
  (void)timer;
  uint64_t now = rg_now();
  atomic_store(&progressed, (uint32_t)now);
  if (atomic_load(&panel_failed))
    return;
  // PRESSED 捕获短点按；轮询当前状态覆盖持续按住（PRESSING 不转发给 indev）。
  bool on = rg_idle_screen_update(&idle_screen, now, has_activity(),
                                  touched || lv_indev_get_state(input) == LV_INDEV_STATE_PRESSED);
  touched = false;
  bool off = atomic_load(&dark);
  if (!on) {
    if (!off) {
      if (!rg_panel_set_on(false)) {
        atomic_store(&panel_failed, true);
        return;
      }
      // 标签滚动也会请求刷新；仅暂停 timer 会被重新唤醒，需一起禁止失效提交。
      lv_display_enable_invalidation(display, false);
      lv_timer_pause(lv_display_get_refr_timer(display));
      atomic_store(&dark, true);
    }
    return;
  }
  if (off)
    lv_display_enable_invalidation(display, true);
  draw_frame(now, off);
  if (off) {
    // 先写入最新的完整帧，再让面板发光，避免短暂出现熄屏前的旧状态。
    uint32_t previous = frames;
    lv_refr_now(display);
    if (frames == previous || !rg_panel_healthy() || !rg_panel_set_on(true)) {
      atomic_store(&panel_failed, true);
      return;
    }
    atomic_store(&dark, false);
  }
}
bool rg_ui_init(void) {
  if (!bsp_display_lock(1000))
    return false;
  display = lv_display_get_default();
  input = bsp_display_get_input_dev();
  if (!display || !input || !rg_panel_init() || !rg_panel_set_on(true)) {
    bsp_display_unlock();
    return false;
  }
  font16 = lv_binfont_create_from_buffer((void *)font16_start,
                                         font16_end - font16_start);
  font20 = lv_binfont_create_from_buffer((void *)font20_start,
                                         font20_end - font20_start);
  font28 = lv_binfont_create_from_buffer((void *)font28_start,
                                         font28_end - font28_start);
  if (!font16 || !font20 || !font28) {
    bsp_display_unlock();
    return false;
  }
  // Noto 全字集携带极端字形的行距；屏幕采用明确的中文正文行盒。
  font16->line_height = 20;
  font16->base_line = 4;
  font20->line_height = 24;
  font20->base_line = 4;
  font28->line_height = 36;
  font28->base_line = 7;
  screen = lv_screen_active();
  lv_obj_set_style_bg_color(screen, lv_color_hex(0), 0);
  business = lv_obj_create(screen);
  lv_obj_remove_style_all(business);
  lv_obj_set_size(business, 480, 480);
  lv_obj_remove_flag(business, LV_OBJ_FLAG_SCROLLABLE);
  lv_obj_add_flag(business, LV_OBJ_FLAG_HIDDEN);
  waiting = label(screen, 32, 215, 416, font28);
  lv_obj_set_style_text_align(waiting, LV_TEXT_ALIGN_CENTER, 0);
  set_text(waiting, "等待电脑连接");
  lv_obj_t *brand = label(business, 65, 16, 175, font16);
  set_text(brand, "red-green");
  header_count = label(business, 240, 16, 175, font16);
  lv_obj_set_style_text_align(header_count, LV_TEXT_ALIGN_RIGHT, 0);
  hero_name = label(business, 24, 366, 253, font28);
  hero_state = label(business, 24, 406, 253, font20);
  balloon = box(business, 128, 144, 44, 36, 0xffffff);
  box(business, 147, 180, 6, 5, 0xffffff);
  hero_bubble = label(balloon, 6, 6, 170, font20);
  lv_obj_set_style_text_color(hero_bubble, lv_color_hex(0x111111), 0);
  lv_obj_set_style_text_align(hero_name, LV_TEXT_ALIGN_CENTER, 0);
  lv_obj_set_style_text_align(hero_state, LV_TEXT_ALIGN_CENTER, 0);
  lv_obj_set_style_text_align(hero_bubble, LV_TEXT_ALIGN_CENTER, 0);
  bubble_image = lv_image_create(balloon);
  lv_obj_set_pos(bubble_image, 6, 2);
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
  for (int i = 0; i < 5; i++) {
    tiles[i] = box(business, 297, 148 + i * 58, 167, 52, 0);
    names[i] = label(tiles[i], 8, 3, 151, font20);
    states[i] = label(tiles[i], 8, 29, 151, font16);
    lv_obj_add_flag(tiles[i], LV_OBJ_FLAG_HIDDEN);
  }
  footer = label(business, 65, 440, 179, font16);
  footer_meta = label(business, 244, 440, 171, font16);
  lv_obj_set_style_text_align(footer_meta, LV_TEXT_ALIGN_RIGHT, 0);
  usage_title = label(business, 24, 46, 360, font20);
  usage_page = label(business, 386, 48, 70, font16);
  lv_obj_set_style_text_align(usage_page, LV_TEXT_ALIGN_RIGHT, 0);
  for (int i = 0; i < 2; i++) {
    usage_labels[i] = label(business, 24, 72 + i * 36, 70, font16);
    usage_values[i] = label(business, 96, 70 + i * 36, 80, font20);
    usage_reset[i] = label(business, 183, 72 + i * 36, 273, font16);
    lv_obj_set_style_text_align(usage_reset[i], LV_TEXT_ALIGN_RIGHT, 0);
    tracks[i] = box(business, 24, 95 + i * 36, 432, 7, 0x262626);
    fills[i] = box(tracks[i], 0, 0, 0, 7, 0x35b982);
    lv_obj_add_event_cb(tracks[i], dashed_track, LV_EVENT_DRAW_MAIN, (void *)(uintptr_t)i);
  }
  epoch = usage_since = rg_now();
  rg_idle_screen_init(&idle_screen, epoch);
  lv_indev_add_event_cb(input, touch_event, LV_EVENT_PRESSED, NULL);
  lv_timer_create(refresh, 33, NULL);
  lv_display_add_event_cb(lv_display_get_default(), ready, LV_EVENT_REFR_READY,
                          NULL);
  lv_display_add_event_cb(display, ready, LV_EVENT_FLUSH_FINISH, NULL);
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
  if (atomic_load(&panel_failed) || !rg_panel_healthy()) {
    bsp_display_unlock();
    return false;
  }
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
  if (atomic_load(&panel_failed) || !rg_panel_healthy())
    return false;
  uint32_t stamp = atomic_load(&dark) ? atomic_load(&progressed) : atomic_load(&rendered);
  return rg_recent(now, stamp);
}
