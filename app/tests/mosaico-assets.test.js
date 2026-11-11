import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";
import crypto from "node:crypto";
import os from "node:os";
import path from "node:path";
import {execFileSync} from "node:child_process";
const root = new URL("../../", import.meta.url);
test("设备资源保留所有皮肤状态帧、动作和叫声，哈希绑定字体", () => {
  const manifest = JSON.parse(fs.readFileSync(new URL("firmware/mosaico/main/generated/manifest.json", root)));
  const context = { window: {} };
  vm.runInNewContext(fs.readFileSync(new URL("app/ui/sprites.js", root), "utf8"), context);
  for (const skin of ["robot", "crab", "cat"]) {
    for (const state of ["idle", "running", "awaiting", "your_turn", "completed", "aborted"]) {
      const actual = manifest.sprites.find(s => s.skin === skin && s.state === state);
      const source = context.window.SPRITES.spec(skin, state);
      assert.equal(actual.frames.length, source.frames.length);
      assert.equal(actual.interval, source.interval || 0);
      if (state !== "aborted") assert.ok(actual.motion.length >= 3);
    }
  }
  assert.equal(manifest.calls.length, 12);
  for (const c of manifest.calls) {
    assert.equal(c.rate, 48000); assert.equal(c.channels, 2);
    assert.ok(c.length > 0 && c.length <= 48000 * 4);
  }
  assert.equal(manifest.fonts.length, 2);
  assert.match(manifest.version, /^[a-f0-9]{64}$/);
});
test("资源导出可重跑，源文件与字体均参与握手哈希", () => {
  const dir=fs.mkdtempSync(path.join(os.tmpdir(),"rg-assets-"));
  const generated=new URL("firmware/mosaico/main/generated/",root);
  const digest=b=>crypto.createHash("sha256").update(b).digest("hex");
  try {
    for(const size of [20,28]) fs.copyFileSync(new URL(`font${size}.bin`,generated),path.join(dir,`font${size}.bin`));
    execFileSync(process.execPath,[new URL("scripts/gen-mosaico-assets.mjs",root).pathname,dir]);
    for(const name of ["assets.bin","assets.c","manifest.json","version.txt","version.h"])
      assert.equal(digest(fs.readFileSync(new URL(name,generated))),digest(fs.readFileSync(path.join(dir,name))),`${name} 需要重新导出`);
  } finally {fs.rmSync(dir,{recursive:true,force:true});}
});
test("彩色气泡的半透明边缘不会因重复预乘 alpha 变暗", () => {
  const generated = new URL("firmware/mosaico/main/generated/", root);
  const manifest = JSON.parse(fs.readFileSync(new URL("manifest.json", generated)));
  const sparkle = manifest.bubbles.find(b => b.state === "completed");
  const blob = fs.readFileSync(new URL("assets.bin", generated));
  let edges = 0;
  // ✨ 原 SVG 的两种填充都是 R=255；中等透明度的边缘仍应保留原色。
  for (let i = sparkle.offset; i < sparkle.offset + sparkle.w * sparkle.h * 4; i += 4) {
    if (blob[i + 3] >= 64 && blob[i + 3] <= 192) {
      edges++;
      assert.ok(blob[i + 2] >= 250, `透明度 ${blob[i + 3]} 的红色通道被预乘成 ${blob[i + 2]}`);
    }
  }
  assert.ok(edges > 10, "必须覆盖实际栅格化的抗锯齿边缘");
});
