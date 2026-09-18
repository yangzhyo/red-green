// 用量表：哑渲染器，把 manager 发来的用量视图画成一枚像素圆表；悬停展开成两行明细；可拖动。
// 视图由 manager 裁决（含"重置已过即 0%"），这里只负责几何、格式与配色。
const { listen, emit } = window.__TAURI__.event;

const WEEKDAY = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

// 颜色全部取自精灵调色板：灯色绿 = 已完成的灯、琥珀 = 运行中的灯、红 = 待确认的灯
const C = window.SPRITES.C;
const LAMP = { ok: C.completed, warn: C.running, hot: C.awaiting };

// 圆表网格：15×15 格，与精灵同一比例。
// 壳体是半径 ~7 格的像素圆盘；外圈灯格半径 6、内圈半径 3，各一格厚，中间留两格壳体分隔；
// 圆心 5×5 格留给数字。网格字符：H 壳体、D 未亮的灯格、O 外圈亮格、I 内圈亮格
const SIZE = 15;
const CENTER = 7;

function cells(pred) {
  const out = [];
  for (let y = 0; y < SIZE; y++) {
    for (let x = 0; x < SIZE; x++) {
      const dx = x - CENTER;
      const dy = y - CENTER;
      if (pred(Math.sqrt(dx * dx + dy * dy))) out.push([y, x]);
    }
  }
  return out;
}

// 灯格从 12 点起顺时针点亮：按与正上方的顺时针夹角 [0, 2π) 排序
function clockwise(list) {
  const angle = ([y, x]) =>
    (Math.atan2(x - CENTER, CENTER - y) + 2 * Math.PI) % (2 * Math.PI);
  return list.slice().sort((a, b) => angle(a) - angle(b));
}

const DISC = cells((d) => d <= 7.3);
const RINGS = {
  five_hour: { ch: "O", cells: clockwise(cells((d) => d >= 5.5 && d < 6.5)) },
  seven_day: { ch: "I", cells: clockwise(cells((d) => d >= 2.5 && d < 3.5)) },
};

let view = null;
let expanded = false;

// 五小时窗口只给时分（5 小时内不会跨到歧义的日期）；七天窗口带星期
function fmtReset(ts, key) {
  if (!ts) return "";
  const d = new Date(ts * 1000);
  const hm = `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
  return key === "five_hour" ? hm : `${WEEKDAY[d.getDay()]} ${hm}`;
}

// 颜色随用量升温：60% 起琥珀，85% 起红
function level(pct) {
  if (pct >= 85) return "hot";
  if (pct >= 60) return "warn";
  return "ok";
}

function pctOf(key) {
  const w = view?.[key];
  return w ? Math.round(w.used_percentage) : null;
}

function renderRing() {
  const grid = Array.from({ length: SIZE }, () => Array(SIZE).fill("."));
  for (const [y, x] of DISC) grid[y][x] = "H";
  const pal = { H: C.housing, D: C.dim };
  for (const [key, ring] of Object.entries(RINGS)) {
    // 缺席的窗口整圈不亮，圆表不因此缺一块
    const pct = pctOf(key) ?? 0;
    const lit = Math.round((ring.cells.length * pct) / 100);
    ring.cells.forEach(([y, x], i) => {
      grid[y][x] = i < lit ? ring.ch : "D";
    });
    pal[ring.ch] = LAMP[level(pct)];
  }
  window.SPRITES.draw(document.getElementById("disc"), { g: grid, p: pal }, window.SPRITES.SCALE);

  // 圆心只放五小时窗口的数字：变得快、和"现在还能不能干活"直接相关
  const five = pctOf("five_hour");
  const num = document.getElementById("num");
  num.textContent = five === null ? "" : `${five}%`;
  num.classList.toggle("wide", five !== null && five >= 100);
}

function renderCard() {
  for (const row of document.querySelectorAll("#card .row")) {
    const key = row.dataset.window;
    const pct = pctOf(key);
    row.hidden = pct === null;
    if (pct === null) continue;
    // 填充色与圆表灯色同源，在这里内联而不在 CSS 里再抄一份
    row.style.setProperty("--fill", `color-mix(in srgb, ${LAMP[level(pct)]} 85%, transparent)`);
    row.style.setProperty("--pct", `${pct}%`);
    row.querySelector(".pct").textContent = `${pct}%`;
    row.querySelector(".reset").textContent = fmtReset(view[key].resets_at, key);
  }
}

const stage = document.getElementById("stage");
document.getElementById("num").style.color = C.white;

function render() {
  renderRing();
  renderCard();
  stage.classList.toggle("expanded", expanded);
}

// 悬停由 Rust 轮询光标推来（非焦点窗口收不到 mousemove）：进入要真的落在圆表 / 卡片上，
// 展开后只要还在窗口内就保持，避免圆表与卡片形状不同导致的反复开合
listen("gauge-hover", (e) => {
  const p = e.payload;
  const hit = p ? stage.contains(document.elementFromPoint(p.x, p.y)) : false;
  const next = !!p && (expanded || hit);
  if (next === expanded) return;
  expanded = next;
  render();
});

listen("usage-update", (e) => {
  view = e.payload ?? null;
  render();
}).then(() => {
  // 首帧握手：窗口刚创建时 manager 的 emitTo 可能先于监听器就绪，就绪后请 manager 补发
  emit("gauge-ready");
});

// 可拖（手势在 drag.js），挡住东西时挪开；原地松手不做任何事
installDrag();
