#pragma once
#include <stddef.h>
#include <stdint.h>
typedef struct {
  int w, h;
  const uint8_t *pixels;
} rg_frame;
typedef struct {
  float at, x, y, sx, sy, angle;
} rg_motion;
typedef struct {
  const char *skin, *state;
  size_t count;
  int interval;
  const rg_frame *frames;
  int period;
  size_t motion_count;
  const rg_motion *motion;
} rg_sprite;
typedef struct {
  const char *name;
  const uint8_t *pcm;
  size_t length;
} rg_sound;
extern const rg_sprite rg_sprites[18];
extern const rg_sound rg_sounds[12];
