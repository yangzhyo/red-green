// 一次性视觉原型。A 五行等高条块；B 当前主角大块＋其余小块。只演示外观，不连接设备。
const $ = (id) => document.getElementById(id);
const variants = {
  rows: { title: "A · 等高条块（已选）", description: "五个会话各占一条。当前主角色块整体消失、再现，淡出与淡入柔和过渡，其余色块保持常亮。" },
  tiles: { title: "B · 大小磁贴", description: "当前主角占一块大磁贴，其余会话排成两列。Metro 特征更强，长名称需要滚动。" },
};
const states = {
  awaiting: { label: "待确认", symbol: "!" }, running: { label: "运行中", symbol: "…" },
  your_turn: { label: "轮到你", symbol: "?" }, completed: { label: "已完成", symbol: "✓" },
  aborted: { label: "异常中止", symbol: "×" }, idle: { label: "空闲", symbol: "z z" },
};
const sessions = [
  { name: "red-green", source: "Codex", state: "awaiting" },
  { name: "portal", source: "Claude", state: "running" },
  { name: "工具箱", source: "Codex", state: "your_turn" },
  { name: "docs", source: "Claude", state: "completed" },
  { name: "notes", source: "Codex", state: "idle" },
].map((s,id) => ({ ...s, id, skin: SKINS.pick(s.name) }));
const groups = [
  { source: "Codex", values: [43,68], reset: ["16:30", "11/15 09:00"] },
  { source: "Claude", values: [32,57], reset: ["17:45", "11/16 10:00"] },
];
let variant = new URLSearchParams(location.search).get("variant") || "rows";
if (!variants[variant]) variant = "rows";
let count = 5, focus = 0, source = 0, usageMode = "normal", paused = false;
let elapsed = 0, focusClock = 0, usageClock = 0, previousTime = performance.now(), drawing;
const reducedMotion = matchMedia("(prefers-reduced-motion: reduce)");
const shown = () => sessions.slice(0,count);
const nameOf = (s) => $("long-name").checked && s.id === 2 ? "中文长名称项目自动滚动展示" : s.name;
const nameMarkup = (s,cls) => `<div class="name-clip ${cls}"><span>${nameOf(s)}</span></div>`;
function applyScreenGeometry() {
  const radius = Number($("corner-mode").value);
  $("device").style.setProperty("--screen-radius",`${radius}px`);
  $("screen").classList.toggle("show-safe",$("safe-area").checked);
  $("corner-caption").textContent = radius === 80 ? "圆角近似 R80" : "较大圆角 R96";
  // 默认布局也按较大的 R96 校核。圆心保持原位，内缩 8px 的圆弧界定文字安全区。
  const safetyRadius = Math.max(96,radius), clearance = 8;
  document.querySelectorAll(".device-header,.device-footer").forEach((el) => {
    // 系统字体的字形包围盒可能越过行框，另留 2px 余量。
    const edgeY = Math.min(el.offsetTop,480 - el.offsetTop - el.offsetHeight) - 2;
    const dy = Math.max(0,safetyRadius - edgeY);
    const inset = Math.ceil(safetyRadius - Math.sqrt(Math.max(0,(safetyRadius - clearance) ** 2 - dy ** 2)));
    el.style.marginInline = `${Math.max(0,inset - 16)}px`;
  });
}
function usageMarkup() {
  const g = groups[source];
  const values = usageMode === "high" ? [92,86] : g.values;
  const unavailable = ["unknown","stale"].includes(usageMode);
  return `<section class="usage"><div class="usage-heading"><strong>${g.source} <span>账号用量</span></strong><span>${source + 1} / ${groups.length}</span></div>${values.map((value,i) => {
    const text = unavailable ? (usageMode === "stale" ? "已过期" : "未知") : `${value}%`;
    const color = value >= 85 ? "#ed526c" : value >= 60 ? "#e7b42b" : "#35b982";
    return `<div class="usage-row ${usageMode}"><div class="usage-label"><span>${i ? "7 天" : "5 小时"}</span><b>${text}</b><small>${unavailable ? "等待更新" : `重置 ${g.reset[i]}`}</small></div><div class="progress-track" role="progressbar" aria-label="${g.source} ${i ? "7 天" : "5 小时"}已用比例" ${unavailable ? `aria-valuetext="${text}"` : `aria-valuemin="0" aria-valuemax="100" aria-valuenow="${value}"`} >${unavailable ? "" : `<div class="progress-fill" style="--value:${value}%;--bar:${color}"></div>`}</div></div>`;
  }).join("")}</section>`;
}
function tileMarkup(s) {
  const current = s.id === focus;
  const meta = states[s.state];
  return `<div class="tile${current ? " current" : ""}" data-state="${s.state}" ${current ? 'aria-current="true"' : ""}>${nameMarkup(s,"tile-name")}${variant === "tiles" ? `<div class="tile-source">${s.source}</div><div class="tile-status-line"><span class="tile-state">${meta.label}</span>${current ? `<span class="tile-symbol">${meta.symbol}</span>` : ""}</div>` : `<div class="tile-state">${meta.label}</div>`}</div>`;
}
function refreshMarquees() {
  document.querySelectorAll(".name-clip").forEach((el) => {
    const distance = el.firstElementChild.scrollWidth - el.clientWidth;
    if (distance > 1) {
      el.classList.add("scrolling");
      el.style.setProperty("--overflow", `${-distance}px`);
      el.style.setProperty("--duration", `${Math.max(7,distance / 22 + 3)}s`);
    }
  });
}
function render() {
  if (focus >= count) focus = 0;
  const s = sessions[focus], state = states[s.state];
  const list = variant === "rows" ? shown() : [s,...shown().filter((item) => item !== s)];
  const attention = shown().filter((item) => ["awaiting","your_turn","aborted"].includes(item.state)).length;
  $("screen").innerHTML = `<header class="device-header"><strong>red-green</strong><span>${count} 个会话</span></header>${usageMarkup()}<section class="device-main"><div class="hero" data-state="${s.state}"><div class="pet-area"><div class="pet" data-state="${s.state}"><span class="bubble">${state.symbol}</span><canvas id="pet" role="img" aria-label="${nameOf(s)}，${state.label}"></canvas></div></div>${nameMarkup(s,"hero-name")}<div class="hero-state">${state.label}</div></div><div class="overview ${variant}">${list.map(tileMarkup).join("")}</div></section><footer class="device-footer"><span>${attention ? `${attention} 个待处理` : "所有会话正常"}</span><span><strong>${s.source}</strong>　${focus + 1} / ${count}</span></footer>`;
  $("variant-title").textContent = variants[variant].title;
  $("variant-description").textContent = variants[variant].description;
  $("focus").innerHTML = shown().map((item) => `<option value="${item.id}" ${item.id === focus ? "selected" : ""}>${nameOf(item)} · ${states[item.state].label}</option>`).join("");
  $("source").value = String(source);
  $("live-state").textContent = `${count} 个会话；主角：${nameOf(s)}（${state.label}）；用量：${groups[source].source}，${$("usage-mode").selectedOptions[0].textContent.split(" · ")[0]}。`;
  document.querySelectorAll("[data-variant]").forEach((b) => b.setAttribute("aria-pressed", String(b.dataset.variant === variant)));
  drawing = { canvas: $("pet"), spec: SPRITES.spec(s.skin,s.state), frame: -1 };
  draw();
  applyScreenGeometry();
  refreshMarquees();
}
function draw() {
  if (!drawing) return;
  const {spec,canvas} = drawing;
  const frame = reducedMotion.matches ? 0 : Math.floor(elapsed / (spec.interval || 1000)) % spec.frames.length;
  if (frame !== drawing.frame) {
    const sprite = spec.frames[frame];
    const scale = Math.min(14,Math.floor(238 / Math.max(...sprite.g.map((row) => row.length))),Math.floor(157 / sprite.g.length));
    SPRITES.draw(canvas,sprite,scale * 2); // 共用渲染器的倍率包含 Retina 2×。
    drawing.frame = frame;
  }
}
function setVariant(next,writeURL = true) {
  variant = variants[next] ? next : "rows";
  if (writeURL) { const url = new URL(location.href); url.searchParams.set("variant",variant); history.replaceState(null,"",url); }
  render();
}
function shiftVariant() { setVariant(variant === "rows" ? "tiles" : "rows"); }
function resize() {
  const width = Math.min(Number($("size").value),document.querySelector(".preview").clientWidth * 480 / 500);
  const scale = width / 480;
  $("device").style.transform = `scale(${scale})`;
  $("device-space").style.width = $("device-space").style.height = `${500 * scale}px`;
  $("size-value").value = `${Math.round(width)} px`;
  $("size-caption").textContent = `${Math.round(scale * 100)}%`;
}
document.querySelectorAll("[data-variant]").forEach((b) => b.addEventListener("click",() => setVariant(b.dataset.variant)));
$("previous").addEventListener("click",shiftVariant);
$("next").addEventListener("click",shiftVariant);
document.addEventListener("keydown",(event) => {
  if (event.target.closest("input,select,textarea,[contenteditable]")) return;
  if (["ArrowLeft","ArrowRight"].includes(event.key)) { event.preventDefault(); shiftVariant(); }
});
window.addEventListener("popstate",() => setVariant(new URLSearchParams(location.search).get("variant"),false));
$("count").addEventListener("change",() => { count = Number($("count").value); render(); });
$("focus").addEventListener("change",() => { focus = Number($("focus").value); focusClock = 0; render(); });
$("source").addEventListener("change",() => { source = Number($("source").value); usageClock = 0; render(); });
$("usage-mode").addEventListener("change",() => { usageMode = $("usage-mode").value; render(); });
$("long-name").addEventListener("change",render);
$("scene").addEventListener("change",() => {
  const scene = $("scene").value;
  const values = scene === "mixed" ? ["awaiting","running","your_turn","completed","idle"] : scene === "quiet" ? ["completed","idle","completed","idle","idle"] : scene === "attention" ? ["awaiting","aborted","your_turn","running","awaiting"] : Array(5).fill("running");
  sessions.forEach((s,i) => { s.state = values[i]; }); render();
});
$("rotate").addEventListener("change",() => { focusClock = usageClock = 0; });
$("pause").addEventListener("click",() => { paused = !paused; document.body.classList.toggle("paused",paused); $("pause").textContent = paused ? "继续动画" : "暂停动画"; $("pause").setAttribute("aria-pressed",String(paused)); });
$("size").addEventListener("input",resize);
$("corner-mode").addEventListener("change",applyScreenGeometry);
$("safe-area").addEventListener("change",applyScreenGeometry);
window.addEventListener("resize",resize);
function tick(now) {
  const dt = Math.min(now - previousTime,100); previousTime = now;
  if (!paused) {
    elapsed += dt;
    if ($("rotate").checked && !reducedMotion.matches) {
      focusClock += dt; usageClock += dt;
      let changed = false;
      if (focusClock >= 4000) { focus = (focus + 1) % count; focusClock = 0; changed = true; }
      if (usageClock >= 8000) { source = (source + 1) % groups.length; usageClock = 0; changed = true; }
      if (changed) render();
    }
    draw();
  }
  requestAnimationFrame(tick);
}
setVariant(variant); resize(); requestAnimationFrame(tick);
