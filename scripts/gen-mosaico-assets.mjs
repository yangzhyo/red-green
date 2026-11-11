// 可重跑的设备资源导出：不执行浏览器、不重新合成声音。
import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";
import crypto from "node:crypto";
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const require = createRequire(path.join(root, "app/package.json"));
const { Resvg } = require("@resvg/resvg-js");
const out = process.argv[2] || path.join(root, "firmware/mosaico/main/generated");
fs.mkdirSync(out, { recursive: true });
const read = p => fs.readFileSync(path.join(root, p));
const hash = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
const context = { window: {} };
vm.runInNewContext(read("app/ui/sprites.js").toString(), context);
const css = read("app/ui/pet.css").toString();
const motions = {};
for (const [, name, body] of css.matchAll(/@keyframes\s+(\w+)\s*\{([\s\S]*?)\n\}/g)) {
  const frames = [];
  for (const [, at, transform] of body.matchAll(/([\d%,\s]+)\{\s*transform:\s*([^;]+);\s*\}/g)) {
    let x = 0, y = 0, sx = 1, sy = 1, angle = 0;
    for (const [, op, arg] of transform.matchAll(/(\w+)\(([^)]+)\)/g)) {
      const values = arg.split(",").map(parseFloat);
      if (op === "translateX") x = values[0];
      else if (op === "translateY") y = values[0];
      else if (op === "rotate") angle = values[0];
      else if (op === "scale") [sx, sy] = [values[0], values[1] ?? values[0]];
      else throw Error(`未知动作 ${op}`);
    }
    for (const p of at.split(",")) frames.push([parseFloat(p), x, y, sx, sy, angle]);
  }
  motions[name] = frames.sort((a,b) => a[0]-b[0]);
}
const animation = {};
for (const [, state, name, seconds] of css.matchAll(/body\[data-state="(\w+)"\] #pet\s*\{\s*animation:\s*(\w+)\s+([\d.]+)s/g))
  animation[state] = { name, period: Math.round(Number(seconds) * 1000) };
const chunks = []; let offset = 0;
function append(bytes) { const pos = offset; chunks.push(bytes); offset += bytes.length; return pos; }
const sprites = [];
for (const skin of ["robot", "crab", "cat"]) for (const state of ["idle", "running", "awaiting", "your_turn", "completed", "aborted"]) {
  const spec = context.window.SPRITES.spec(skin, state);
  const frames = spec.frames.map(f => {
    const w = Math.max(...f.g.map(row => row.length)), h = f.g.length;
    const pixels = Buffer.alloc(w * h * 4);
    f.g.forEach((row, y) => row.forEach((ch, x) => {
      if (f.p[ch]) pixels.writeUInt32LE((0xff000000 | parseInt(f.p[ch].slice(1), 16)) >>> 0, (y*w+x)*4);
    }));
    return { offset: append(pixels), w, h };
  });
  const a = animation[state];
  sprites.push({ skin, state, frames, interval: spec.interval || 0, period: a?.period || 0, motion: a ? motions[a.name] : [] });
}
const calls = [];
for (const skin of ["robot", "crab", "cat"]) for (const state of ["awaiting", "aborted", "your_turn", "completed"]) {
  const name = `${skin}-${state}`, wav = read(`app/src-tauri/calls/${name}.wav`);
  let fmt, data;
  if (wav.toString("ascii",0,4)!=="RIFF" || wav.toString("ascii",8,12)!=="WAVE") throw Error("不是 WAV");
  for (let p=12;p+8<=wav.length;) {
    const n=wav.readUInt32LE(p+4), id=wav.toString("ascii",p,p+4), bytes=wav.subarray(p+8,p+8+n);
    if (bytes.length!==n) throw Error("WAV 截断");
    if (id==="fmt ") fmt=bytes;
    if (id==="data") data=bytes;
    p+=8+n+(n%2);
  }
  if (!fmt || !data || fmt.readUInt16LE(0)!==1 || fmt.readUInt16LE(2)!==1 || fmt.readUInt16LE(14)!==16) throw Error("需要 PCM16 单声道");
  const sourceRate=fmt.readUInt32LE(4), count=data.length/2, n=Math.round(count*48000/sourceRate), pcm=Buffer.alloc(n*4);
  for (let i=0;i<n;i++) {
    const x=i*sourceRate/48000, a=Math.min(Math.floor(x),count-1), b=Math.min(a+1,count-1), t=x-a;
    const value=Math.round(data.readInt16LE(a*2)*(1-t)+data.readInt16LE(b*2)*t);
    pcm.writeInt16LE(value,i*4);pcm.writeInt16LE(value,i*4+2);
  }
  calls.push({name,offset:append(pcm),length:pcm.length,rate:48000,channels:2});
}
// 固定气泡预渲染为 32 px 彩图；设备无需 emoji 字体或 SVG 解码器。
const bubbleSources = {
  idle: ["1f4a4"], awaiting: ["1f6a9", "2757"],
  aborted: ["1f4a5"], your_turn: ["1f44b", "1f4ac"], completed: ["2728"],
};
const bubbleFiles = [];
const bubbles = Object.entries(bubbleSources).map(([state, symbols]) => {
  const w = symbols.length * 32 + (symbols.length - 1) * 4, h = 32;
  const pixels = Buffer.alloc(w * h * 4);
  symbols.forEach((symbol, index) => {
    const source = `firmware/mosaico/assets/twemoji/${symbol}.svg`;
    bubbleFiles.push(source);
    const rendered = new Resvg(read(source), {
      fitTo: { mode: "width", value: 32 }, font: { loadSystemFonts: false },
    }).render();
    if (rendered.width !== 32 || rendered.height !== 32) throw Error(`气泡尺寸错误：${source}`);
    const rgba = rendered.pixels;
    for (let y = 0; y < 32; y++) for (let x = 0; x < 32; x++) {
      const from = (y * 32 + x) * 4, to = (y * w + index * 36 + x) * 4;
      // resvg 返回预乘 RGBA；LVGL ARGB8888 需要直通 alpha 的小端 BGRA。
      const alpha = rgba[from + 3];
      for (let channel = 0; channel < 3; channel++)
        pixels[to + channel] = alpha ? Math.min(255, Math.round(rgba[from + 2 - channel] * 255 / alpha)) : 0;
      pixels[to + 3] = alpha;
    }
  });
  return { state, symbols, w, h, offset: append(pixels) };
});
const blob=Buffer.concat(chunks);
const fonts=[16,20,28].map(size=>({size,sha256:hash(fs.readFileSync(path.join(out,`font${size}.bin`)))}));
const sourceFiles=["app/ui/sprites.js","app/ui/pet.css","scripts/gen-mosaico-assets.mjs",...calls.map(c=>`app/src-tauri/calls/${c.name}.wav`),...bubbleFiles];
const sources=Object.fromEntries(sourceFiles.map(p=>[p,hash(read(p))]));
const manifest={sprites,calls,bubbles,fonts,sources,rasterizer:{name:"@resvg/resvg-js",version:require("@resvg/resvg-js/package.json").version},blob_sha256:hash(blob)};
manifest.version=hash(JSON.stringify(manifest));
fs.writeFileSync(path.join(out,"assets.bin"),blob);
fs.writeFileSync(path.join(out,"manifest.json"),JSON.stringify(manifest,null,2)+"\n");
fs.writeFileSync(path.join(out,"version.txt"),manifest.version);
fs.writeFileSync(path.join(out,"version.h"),`#pragma once\n#define RG_RESOURCES "${manifest.version}"\n`);
const cnum = n => Number.isInteger(n) ? `${n}.0f` : `${n}f`;
const code=[`// 由 scripts/gen-mosaico-assets.mjs 生成。\n#include "assets.h"\nextern const uint8_t rg_asset_blob[] asm("_binary_assets_bin_start");`];
sprites.forEach((s,i)=>{
  code.push(`static const rg_frame frames_${i}[]={${s.frames.map(f=>`{${f.w},${f.h},rg_asset_blob+${f.offset}}`).join(",")}};`);
  if(s.motion.length) code.push(`static const rg_motion motion_${i}[]={${s.motion.map(m=>`{${m.map(cnum).join(",")}}`).join(",")}};`);
});
code.push(`const rg_sprite rg_sprites[]={${sprites.map((s,i)=>`{"${s.skin}","${s.state}",${s.frames.length},${s.interval},frames_${i},${s.period},${s.motion.length},${s.motion.length?`motion_${i}`:"NULL"}}`).join(",\n")}};`);
code.push(`const rg_sound rg_sounds[]={${calls.map(c=>`{"${c.name}",rg_asset_blob+${c.offset},${c.length}}`).join(",\n")}};`);
code.push(`const rg_bubble rg_bubbles[]={${bubbles.map(b=>`{"${b.state}",{${b.w},${b.h},rg_asset_blob+${b.offset}}}`).join(",\n")}};`);
fs.writeFileSync(path.join(out,"assets.c"),code.join("\n")+"\n");
console.log(`资源 ${manifest.version}：18 个状态、${sprites.reduce((n,s)=>n+s.frames.length,0)} 帧、12 段声音、5 组气泡；${blob.length} bytes`);
