// 用量表：按来源和额度组切换，外圈/圆心为首窗口、内圈为第二窗口。
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
  0: { ch: "O", cells: clockwise(cells((d) => d >= 5.5 && d < 6.5)) },
  1: { ch: "I", cells: clockwise(cells((d) => d >= 2.5 && d < 3.5)) },
};

let groups = [];
let view = null;
let expanded = false;

// 短窗口显示时分，长窗口显示星期与时分。
function fmtReset(ts, minutes) {
  if (!ts) return "";
  const d = new Date(ts * 1000);
  const hm = `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
  return minutes <= 1440 ? hm : `${WEEKDAY[d.getDay()]} ${hm}`;
}

// 颜色随用量升温：60% 起琥珀，85% 起红
function level(pct) {
  if (pct >= 85) return "hot";
  if (pct >= 60) return "warn";
  return "ok";
}

function pctOf(key) {
  const w = view?.windows[key];
  return typeof w?.used_percentage === "number" ? Math.round(w.used_percentage) : null;
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
  const five = pctOf(0);
  const num = document.getElementById("num");
  num.textContent = five === null ? "?" : `${five}%`;
  num.classList.toggle("wide", five !== null && five >= 100);
}

function renderCard() {
  for (const row of document.querySelectorAll("#card .row")) {
    const key = Number(row.dataset.window);
    const w = view?.windows[key];
    const pct = pctOf(key);
    row.hidden = !w;
    if (!w) continue;
    row.style.setProperty("--fill", `color-mix(in srgb, ${LAMP[level(pct ?? 0)]} 85%, transparent)`);
    row.style.setProperty("--pct", `${pct ?? 0}%`);
    row.querySelector(".k").textContent = USAGE.duration(w.duration_minutes);
    row.querySelector(".pct").textContent = pct === null ? "未知" : `${pct}%`;
    row.querySelector(".reset").textContent = fmtReset(w.resets_at, w.duration_minutes);
  }
}

const stage = document.getElementById("stage");
document.getElementById("num").style.color = C.white;

function render() {
  const source = document.getElementById("source");
  const label = view?.source === "codex" ? "Codex" : "Claude";
  source.textContent = label + (groups.length > 1 ? " ▸" : "");
  const bucket = view?.label && view.label.toLowerCase() !== label.toLowerCase() ? view.label : "";
  document.getElementById("bucket").textContent = bucket;
  const updated = view?.updated_at ? `；更新于 ${new Date(view.updated_at * 1000).toLocaleTimeString()}` : "";
  stage.title = `${label}${bucket ? ` · ${bucket}` : ""}${updated}${groups.length > 1 ? "；点击切换账号用量" : ""}`;
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
  groups = e.payload ?? [];
  view = USAGE.select(groups, view?.id);
  render();
}).then(() => {
  // 首帧握手：窗口刚创建时 manager 的 emitTo 可能先于监听器就绪，就绪后请 manager 补发
  emit("gauge-ready");
});

// 整窗可拖；原地点击在来源/额度组之间轮换，选择在后续快照中保持。
function nextGroup() {
  if (groups.length < 2) return;
  view = groups[(groups.findIndex((g) => g.id === view?.id) + 1) % groups.length];
  render();
}
installDrag(nextGroup);
// 原生 button 自己处理点击/键盘，避免与整窗拖动手势重复触发。
const sourceButton = document.getElementById("source");
sourceButton.addEventListener("mousedown", (e) => e.stopPropagation());
sourceButton.addEventListener("click", nextGroup);
