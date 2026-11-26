#!/usr/bin/env node
// 用桌宠的同一帧生成应用图标。第二个可选输出是 macOS 单色菜单栏模板。
import fs from "node:fs";
import vm from "node:vm";
import { createRequire } from "node:module";

const requireFromApp = createRequire(new URL("../app/package.json", import.meta.url));
const { Resvg } = requireFromApp("@resvg/resvg-js");
const window = {};
vm.runInNewContext(fs.readFileSync(new URL("../app/ui/sprites.js", import.meta.url), "utf8"), { window });
const { g: grid, p: palette } = window.SPRITES.spec("crab", "completed").frames[0];

function renderCrab(unit, monochrome) {
  const cells = [];
  for (const [y, row] of grid.entries()) {
    for (const [x, key] of row.entries()) {
      if (!palette[key] || (monochrome && key === "S")) continue;
      cells.push(`<rect x="${x * unit}" y="${y * unit}" width="${unit}" height="${unit}" fill="${monochrome ? "black" : palette[key]}"/>`);
    }
  }
  return cells.join("");
}
function writeIcon(path, size, artwork) {
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}">${artwork}</svg>`;
  fs.writeFileSync(path, new Resvg(svg).render().asPng());
}

writeIcon(process.argv[2] || "icon.png", 1024,
  '<rect x="80" y="80" width="864" height="864" rx="214" fill="#fffcf1" stroke="#c7caba" stroke-width="4"/>' +
  `<g transform="translate(208 322)">${renderCrab(38, false)}</g>`);
if (process.argv[3]) {
  writeIcon(process.argv[3], 64, `<g transform="translate(0 12)">${renderCrab(4, true)}</g>`);
}
