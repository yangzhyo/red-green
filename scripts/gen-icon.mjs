#!/usr/bin/env node
// Generate the app icon (a little traffic light) as RGBA PNG, no deps.
// Usage: node gen-icon.mjs <output.png>

import zlib from "node:zlib";
import fs from "node:fs";

const W = 512;
const H = 512;
const px = Buffer.alloc(W * H * 4);

function set(x, y, r, g, b, a = 255) {
  if (x < 0 || y < 0 || x >= W || y >= H) return;
  const i = (y * W + x) * 4;
  px[i] = r;
  px[i + 1] = g;
  px[i + 2] = b;
  px[i + 3] = a;
}

// housing: dark rounded rectangle on transparent background
const HX0 = 176, HX1 = 336, HY0 = 48, HY1 = 464, R = 28;
for (let y = HY0; y < HY1; y++) {
  for (let x = HX0; x < HX1; x++) {
    const cx = Math.max(HX0 + R - x, x - (HX1 - 1 - R), 0);
    const cy = Math.max(HY0 + R - y, y - (HY1 - 1 - R), 0);
    if (cx * cx + cy * cy <= R * R) set(x, y, 26, 26, 29);
  }
}

function circle(cx, cy, rad, [r, g, b]) {
  for (let y = cy - rad; y <= cy + rad; y++)
    for (let x = cx - rad; x <= cx + rad; x++)
      if ((x - cx) ** 2 + (y - cy) ** 2 <= rad * rad) set(x, y, r, g, b);
}

circle(256, 128, 52, [255, 77, 79]); // 待确认
circle(256, 256, 52, [255, 197, 61]); // 运行中
circle(256, 384, 52, [82, 196, 26]); // 已完成

function chunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const t = Buffer.from(type);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(zlib.crc32(Buffer.concat([t, data])) >>> 0);
  return Buffer.concat([len, t, data, crc]);
}

const ihdr = Buffer.alloc(13);
ihdr.writeUInt32BE(W, 0);
ihdr.writeUInt32BE(H, 4);
ihdr[8] = 8; // bit depth
ihdr[9] = 6; // color type: RGBA

const raw = Buffer.alloc((W * 4 + 1) * H);
for (let y = 0; y < H; y++) {
  raw[y * (W * 4 + 1)] = 0; // filter: none
  px.copy(raw, y * (W * 4 + 1) + 1, y * W * 4, (y + 1) * W * 4);
}

const png = Buffer.concat([
  Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]),
  chunk("IHDR", ihdr),
  chunk("IDAT", zlib.deflateSync(raw, { level: 9 })),
  chunk("IEND", Buffer.alloc(0)),
]);

fs.writeFileSync(process.argv[2] ?? "icon.png", png);
console.log(`wrote ${process.argv[2] ?? "icon.png"} (${png.length} bytes)`);
