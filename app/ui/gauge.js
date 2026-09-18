// 用量表：哑渲染器，把 manager 发来的用量视图画成一个圆；悬停展开成两行明细。
// 视图由 manager 裁决（含"重置已过即 0%"），这里只负责几何、格式与配色。
const { listen, emit } = window.__TAURI__.event;

const WEEKDAY = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const WINDOWS = ["five_hour", "seven_day"];

let view = null;
let expanded = false;

// 五小时窗口只给时分（5 小时内不会跨到歧义的日期）；七天窗口带星期
function fmtReset(ts, key) {
  if (!ts) return "";
  const d = new Date(ts * 1000);
  const hm = `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
  return key === "five_hour" ? hm : `${WEEKDAY[d.getDay()]} ${hm}`;
}

// 颜色随用量升温：平时白色，60% 起琥珀，85% 起红
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
  for (const key of WINDOWS) {
    const arc = document.getElementById(`arc-${key.replace("_", "-")}`);
    const pct = pctOf(key);
    const r = Number(arc.getAttribute("r"));
    const c = 2 * Math.PI * r;
    // 缺席的窗口画成空环，圆不因此缺一块
    arc.setAttribute("stroke-dasharray", `${(c * (pct ?? 0)) / 100} ${c}`);
    arc.dataset.level = level(pct ?? 0);
  }
  // 圆心只放五小时窗口的数字：变得快、和"现在还能不能干活"直接相关
  const five = pctOf("five_hour");
  document.getElementById("num").textContent = five === null ? "" : `${five}%`;
}

function renderCard() {
  for (const row of document.querySelectorAll("#card .row")) {
    const key = row.dataset.window;
    const w = view?.[key];
    row.hidden = !w;
    if (!w) continue;
    const pct = Math.round(w.used_percentage);
    row.dataset.level = level(pct);
    row.style.setProperty("--pct", `${pct}%`);
    row.querySelector(".pct").textContent = `${pct}%`;
    row.querySelector(".reset").textContent = fmtReset(w.resets_at, key);
  }
}

const stage = document.getElementById("stage");

function render() {
  renderRing();
  renderCard();
  stage.classList.toggle("expanded", expanded);
}

// 悬停由 Rust 轮询光标推来（非焦点窗口收不到 mousemove）：进入要真的落在圆 / 卡片上，
// 展开后只要还在窗口内就保持，避免圆与卡片形状不同导致的反复开合
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
