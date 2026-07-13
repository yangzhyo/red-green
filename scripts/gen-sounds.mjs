#!/usr/bin/env node
// Generate the 12 pet call sounds (3 skins x 4 states) as 16-bit mono WAV, no deps.
// 节奏属状态（听节奏辨事件），音色属物种（听声辨项目）——见 CONTEXT.md「叫声」。
// Usage: node gen-sounds.mjs [outdir]   (default: app/src-tauri/sounds/)

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const SR = 44100;

// 节奏语法：每个状态一串音符 {f: 频率Hz, d: 时长s, g: 后置间隔s}。
// vol 是整档响度，紧急度递减：待确认 > 异常中止 ≈ 轮到你 > 已完成（最轻）
const PATTERNS = {
  awaiting: {
    vol: 1.0,
    notes: [
      { f: 880, d: 0.065, g: 0.055 },
      { f: 880, d: 0.065, g: 0.055 },
      { f: 880, d: 0.065, g: 0 },
    ],
  },
  aborted: {
    vol: 0.85,
    notes: [
      { f: 587, d: 0.16, g: 0.05 },
      { f: 311, d: 0.26, g: 0 },
    ],
  },
  your_turn: {
    vol: 0.7,
    notes: [
      { f: 523, d: 0.11, g: 0.045 },
      { f: 784, d: 0.15, g: 0 },
    ],
  },
  completed: {
    vol: 0.45,
    notes: [{ f: 1047, d: 0.09, g: 0 }],
  },
};

// 线性起音/收音包络，消除音符边缘的爆点
function env(i, n, aSec, rSec) {
  const a = aSec * SR;
  const r = rSec * SR;
  let e = 1;
  if (i < a) e = i / a;
  if (n - i < r) e = Math.min(e, (n - i) / r);
  return e;
}

// 固定种子的噪声：重跑脚本得到逐字节相同的产物，方便 diff/版本化
let seed = 0x2b992ddf;
function noise() {
  seed = (seed * 1664525 + 1013904223) >>> 0;
  return (seed / 2 ** 32) * 2 - 1;
}

// —— 音色 —— 每个物种一个音符渲染器：(f, d) -> Float64Array

// 灯灯：25% 占空比脉冲波，8-bit 机器人哔声
function robot(f, d) {
  const n = Math.round(d * SR);
  const out = new Float64Array(n);
  for (let i = 0; i < n; i++) {
    const ph = ((i / SR) * f) % 1;
    out[i] = (ph < 0.25 ? 1 : -1) * env(i, n, 0.004, 0.015);
  }
  return out;
}

// 钳钳：噪声爆点 + 快速衰减的敲击共鸣，螃蟹钳子的咔哒
function crab(f, d) {
  const n = Math.round(d * SR);
  const out = new Float64Array(n);
  const clickLen = SR * 0.008;
  for (let i = 0; i < n; i++) {
    const t = i / SR;
    const ring = Math.sin(2 * Math.PI * f * t) * Math.exp(-t * 22);
    const click = i < clickLen ? noise() * (1 - i / clickLen) : 0;
    out[i] = (0.8 * ring + 0.5 * click) * env(i, n, 0.001, 0.01);
  }
  return out;
}

// 灰灰：由低滑入的正弦 + 颤音 + 第二泛音，合成器猫叫的「喵」轮廓
function cat(f, d) {
  const n = Math.round(d * SR);
  const out = new Float64Array(n);
  let phase = 0;
  for (let i = 0; i < n; i++) {
    const t = i / SR;
    const slide = 0.82 + 0.18 * Math.min(1, t / (d * 0.4));
    const vib = 1 + 0.03 * Math.sin(2 * Math.PI * 5.5 * t) * Math.min(1, t / 0.05);
    phase += (2 * Math.PI * f * slide * vib) / SR;
    out[i] = (Math.sin(phase) + 0.3 * Math.sin(2 * phase)) * env(i, n, 0.015, 0.04);
  }
  return out;
}

// 方波/脉冲的等峰值响度明显高于正弦，按音色微调增益拉平主观音量
const TIMBRES = {
  robot: { render: robot, trim: 0.75 },
  crab: { render: crab, trim: 1.0 },
  cat: { render: cat, trim: 0.95 },
};

function renderCall(timbre, pattern) {
  const total = pattern.notes.reduce((s, nt) => s + nt.d + nt.g, 0) + 0.05;
  const buf = new Float64Array(Math.round(total * SR));
  let at = 0;
  for (const nt of pattern.notes) {
    const s = timbre.render(nt.f, nt.d);
    const off = Math.round(at * SR);
    for (let i = 0; i < s.length; i++) buf[off + i] += s[i];
    at += nt.d + nt.g;
  }
  let peak = 0;
  for (const v of buf) peak = Math.max(peak, Math.abs(v));
  const k = (pattern.vol * timbre.trim * 0.9) / (peak || 1);
  const pcm = Buffer.alloc(buf.length * 2);
  for (let i = 0; i < buf.length; i++) {
    pcm.writeInt16LE(Math.round(Math.max(-1, Math.min(1, buf[i] * k)) * 32767), i * 2);
  }
  return pcm;
}

function wav(pcm) {
  const h = Buffer.alloc(44);
  h.write("RIFF", 0);
  h.writeUInt32LE(36 + pcm.length, 4);
  h.write("WAVE", 8);
  h.write("fmt ", 12);
  h.writeUInt32LE(16, 16);
  h.writeUInt16LE(1, 20); // PCM
  h.writeUInt16LE(1, 22); // mono
  h.writeUInt32LE(SR, 24);
  h.writeUInt32LE(SR * 2, 28);
  h.writeUInt16LE(2, 32);
  h.writeUInt16LE(16, 34);
  h.write("data", 36);
  h.writeUInt32LE(pcm.length, 40);
  return Buffer.concat([h, pcm]);
}

const here = path.dirname(fileURLToPath(import.meta.url));
const outdir = process.argv[2] ?? path.join(here, "..", "app", "src-tauri", "sounds");
fs.mkdirSync(outdir, { recursive: true });

for (const [skin, timbre] of Object.entries(TIMBRES)) {
  for (const [state, pattern] of Object.entries(PATTERNS)) {
    const file = path.join(outdir, `${skin}-${state}.wav`);
    fs.writeFileSync(file, wav(renderCall(timbre, pattern)));
    console.log(`wrote ${file}`);
  }
}
