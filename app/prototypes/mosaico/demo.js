// 一次性原型：状态只在内存中。轮播/抢占规则是供讨论的候选，不是最终固件实现。
const $ = (id) => document.getElementById(id);
const variants = {
  hybrid: { name: "主角＋总览", description: "已选定黄金比例：左侧 61.8% 展示主角，右侧 38.2% 保留全部会话总览。" },
  solo: { name: "单宠轮播", description: "把空间留给一只宠物和大字名牌。每 4 秒轮换，待处理会话优先。" },
  list: { name: "会话列表", description: "五个项目一起看清，宠物缩成小图标。优先比较项目名和状态的可读性。" },
};
const keys = Object.keys(variants);
let variant = new URLSearchParams(location.search).get("variant");
if (!variants[variant]) variant = "hybrid";
function writeViewURL() {
  const url = new URL(location.href);
  url.searchParams.set("variant", variant);
  url.searchParams.delete("split"); // 旧版试调链接不覆盖已确定的黄金比例。
  history.replaceState(null, "", url);
}
const states = {
  running: { label: "运行中", bubble: "工作中", color: "#ffc53d", priority: 0 },
  awaiting: { label: "待确认", bubble: "!", color: "#ff6d6f", priority: 3 },
  your_turn: { label: "轮到你", bubble: "?", color: "#70b7ff", priority: 1 },
  completed: { label: "已完成", bubble: "✓", color: "#8cd463", priority: 0 },
  aborted: { label: "异常中止", bubble: "×", color: "#ff9d82", priority: 2 },
  idle: { label: "空闲", bubble: "z z", color: "#a5b3a8", priority: 0 },
};
const sessions = [
  { name: "red-green", source: "Codex" },
  { name: "portal", source: "Claude" },
  { name: "工具箱", source: "Codex" },
  { name: "docs", source: "Claude" },
  { name: "notes", source: "Codex" },
].map((s, id) => ({ ...s, id, skin: SKINS.pick(s.name), state: "running" }));
const usage = [
  { source: "Codex", windows: [32, 57], reset: "2h 18m 后重置" },
  { source: "Claude", windows: [68, 24], reset: "1h 42m 后重置" },
];
let count = 5, focus = 0, usageIndex = 0;
let paused = false, elapsed = 0, focusElapsed = 0, usageElapsed = 0, storyElapsed = 0, storyStep = 0;
let lastTime = performance.now(), drawings = [], activeAudio;
const visible = () => sessions.slice(0, count);
const availableUsage = () => usage.filter((u) => visible().some((s) => s.source === u.source));
const needsAction = () => visible().filter((s) => states[s.state].priority > 0);
function queue() {
  const highest = Math.max(...visible().map((s) => states[s.state].priority));
  return visible().filter((s) => states[s.state].priority === highest);
}
function normalizeFocus() {
  const candidates = queue();
  if (!candidates.some((s) => s.id === focus)) { focus = candidates[0].id; focusElapsed = 0; }
}
function log(message) { $("event-log").textContent = message; }
function petMarkup(s, scale, bubble = false) {
  return `<div class="pet-unit" data-state="${s.state}">${bubble ? `<span class="bubble">${states[s.state].bubble}</span>` : ""}<canvas data-pet="${s.id}" data-scale="${scale}" aria-label="${s.name}：${states[s.state].label}"></canvas></div>`;
}
function heroMarkup(s) {
  const spriteScale = 18;
  return `<div class="hero" style="--state:${states[s.state].color}"><div class="pet-area">${petMarkup(s, spriteScale, true)}</div><div class="project">${s.name}</div><div class="hero-status">${states[s.state].label}</div><div class="hero-source">${s.source}</div></div>`;
}
function mainMarkup() {
  if (variant === "solo") return heroMarkup(sessions[focus]);
  if (variant === "hybrid") return heroMarkup(sessions[focus]) + `<div class="overview">${visible().map((s) => `<div class="overview-row ${s.id === focus ? "current" : ""}" style="--state:${states[s.state].color}"><div class="overview-name">${s.name}</div><div class="overview-state">${states[s.state].label}</div></div>`).join("")}</div>`;
  return `<div class="session-list">${visible().map((s) => `<div class="session-row" style="--state:${states[s.state].color}"><div class="row-art">${petMarkup(s, 5)}</div><div class="row-name">${s.name}<small>${s.source}</small></div><div class="row-state">${states[s.state].label}</div></div>`).join("")}</div>`;
}
function drawGauge() {
  const canvas = $("gauge");
  canvas.width = canvas.height = 150;
  const ctx = canvas.getContext("2d");
  const rings = [[], []];
  for (let y = 0; y < 15; y++) for (let x = 0; x < 15; x++) {
    const d = Math.hypot(x - 7, y - 7);
    if (d <= 7.3) { ctx.fillStyle = SPRITES.C.housing; ctx.fillRect(x * 10, y * 10, 10, 10); }
    if (d >= 5.5 && d < 6.5) rings[0].push([x, y]);
    if (d >= 2.5 && d < 3.5) rings[1].push([x, y]);
  }
  const angle = ([x,y]) => (Math.atan2(x - 7, 7 - y) + Math.PI * 2) % (Math.PI * 2);
  rings.forEach((ring, i) => {
    const pct = usage[usageIndex].windows[i];
    const color = pct >= 85 ? SPRITES.C.awaiting : pct >= 60 ? SPRITES.C.running : SPRITES.C.completed;
    ring.sort((a,b) => angle(a) - angle(b)).forEach(([x,y], index) => {
      ctx.fillStyle = index < Math.round(ring.length * pct / 100) ? color : SPRITES.C.dim;
      ctx.fillRect(x * 10, y * 10, 10, 10);
    });
  });
}
function render() {
  normalizeFocus();
  $("split-controls").hidden = variant !== "hybrid";
  // 480 px 减去两侧 16 px 内边距和 12 px 栏间距，剩余 436 px。
  $("split-note").textContent = "左主角约 269 px，右总览约 167 px。比例不包含内边距和栏间距；总览字号保持不变。";
  if (!availableUsage().includes(usage[usageIndex])) usageIndex = 0;
  const u = usage[usageIndex], current = sessions[focus], actionCount = needsAction().length;
  $("variant-title").textContent = variants[variant].name;
  $("variant-description").textContent = variants[variant].description;
  document.querySelectorAll("[data-variant]").forEach((button) => button.setAttribute("aria-pressed", String(button.dataset.variant === variant)));
  $("screen").innerHTML = `<header class="device-header"><strong>red-green</strong><span>${count} 个会话</span></header><section class="usage"><div class="dial"><canvas id="gauge"></canvas><span>${u.windows[0]}%</span></div><div class="usage-source">${u.source}<small>账号用量</small></div><div class="usage-windows"><div><span>5 小时</span><b>${u.windows[0]}%</b></div><div><span>7 天</span><b>${u.windows[1]}%</b></div></div></section><section class="device-main ${variant}">${mainMarkup()}</section><footer class="device-footer"><span>${actionCount ? `${actionCount} 个待处理` : "会话正常轮播"}</span><span>${variant === "list" ? "全部会话" : `${focus + 1} / ${count} · ${current.source}`}</span></footer>`;
  drawGauge();
  drawings = [...document.querySelectorAll("canvas[data-pet]")].map((canvas) => {
    const s = sessions[Number(canvas.dataset.pet)];
    return { canvas, spec: SPRITES.spec(s.skin, s.state), scale: Number(canvas.dataset.scale), frame: -1 };
  });
  $("focus-info").textContent = variant === "list" ? "全部同时展示" : `${current.name} · ${states[current.state].label}`;
  $("queue-info").textContent = variant === "list" ? "顺序固定" : queue().map((s) => s.name).join(" → ");
  $("usage-info").textContent = `${u.source} · ${availableUsage().length > 1 ? "每 8 秒轮换" : "仅此来源在场"}`;
  drawFrames();
}
function drawFrames() {
  drawings.forEach((item) => {
    const index = Math.floor(elapsed / (item.spec.interval || 1000)) % item.spec.frames.length;
    if (index !== item.frame) { SPRITES.draw(item.canvas, item.spec.frames[index], item.scale); item.frame = index; }
  });
  $("screen").style.setProperty("--progress", (variant === "list" ? usageElapsed / 8000 : focusElapsed / 4000).toFixed(3));
}
function playCall(s) {
  if (!$("sound").checked || !["awaiting", "your_turn", "aborted", "completed"].includes(s.state)) return;
  if (activeAudio) activeAudio.pause();
  activeAudio = new Audio(`../../src-tauri/calls/${s.skin}-${s.state}.wav`);
  activeAudio.volume = .45;
  activeAudio.play().catch(() => log("叫声未能播放；可以关闭后重新开启叫声试听。"));
}
function transition(id, next) {
  const s = sessions[id];
  if (s.state === next) { log(`${s.name} 已处于${states[next].label}，不会重复发出叫声。`); return; }
  s.state = next;
  // 新的同级或更紧急事件可抢占；持续状态只在最高优先级队列中轮播。
  if (states[next].priority > 0 && states[next].priority >= states[sessions[focus].state].priority) { focus = id; focusElapsed = 0; }
  normalizeFocus();
  log(`${s.name} → ${states[next].label}。${variant === "list" ? "列表原位更新。" : "待处理会话优先；切换画面不代表已阅。"}`);
  playCall(s);
  render();
}
function setScene(scene) {
  $("story").checked = false;
  sessions.forEach((s, i) => { s.state = scene === "attention" ? ["awaiting", "awaiting", "your_turn", "aborted", "running"][i] : scene === "quiet" ? ["completed", "idle", "completed", "idle", "idle"][i] : "running"; });
  focus = 0; focusElapsed = 0;
  log(scene === "attention" ? "两个待确认会话交替展示；总览或列表保留其他会话状态。" : scene === "quiet" ? "完成与空闲：观察不同皮肤的姿态，以及小尺寸下的辨识度。" : "日常轮播：每 4 秒更换主角，用量来源每 8 秒轮换。");
  render();
}
function storyTick() {
  const second = Math.min(1, count - 1);
  const steps = [
    () => transition(0, "awaiting"),
    () => transition(second, "awaiting"),
    () => transition(0, "running"),
    () => transition(second, "your_turn"),
    () => transition(second, "completed"),
    () => { sessions.forEach((s) => { s.state = "running"; }); log("新一轮开始：所有会话恢复运行。"); render(); },
  ];
  steps[storyStep++ % steps.length]();
}
function setVariant(next, replaceURL = true) {
  variant = next;
  if (replaceURL) writeViewURL();
  log(`正在比较${variants[variant].name}；模拟会话状态保持一致。`);
  render();
}
function shiftVariant(delta) { setVariant(keys[(keys.indexOf(variant) + delta + keys.length) % keys.length]); }
function resize() {
  const requested = Number($("size").value);
  const width = Math.min(requested, document.querySelector(".preview").clientWidth * 480 / 500);
  const scale = width / 480;
  $("device").style.transform = `scale(${scale})`;
  $("device-space").style.width = `${500 * scale}px`;
  $("device-space").style.height = `${500 * scale}px`;
  $("size-value").value = `${Math.round(width)} px`;
  $("preview-size").textContent = `${Math.round(scale * 100)}%`;
}
function targets() {
  const old = $("target").value;
  $("target").innerHTML = visible().map((s) => `<option value="${s.id}">${s.name} · ${s.source}</option>`).join("");
  if (Number(old) < count) $("target").value = old || "0";
}
document.querySelectorAll("[data-variant]").forEach((b) => b.addEventListener("click", () => setVariant(b.dataset.variant)));
document.querySelectorAll("[data-scene]").forEach((b) => b.addEventListener("click", () => setScene(b.dataset.scene)));
document.querySelectorAll("[data-event]").forEach((b) => b.addEventListener("click", () => { $("story").checked = false; transition(Number($("target").value), b.dataset.event); }));
$("previous").addEventListener("click", () => shiftVariant(-1));
$("next").addEventListener("click", () => shiftVariant(1));
document.addEventListener("keydown", (e) => {
  if (e.target.closest("input,textarea,select,[contenteditable]")) return;
  if (e.key === "ArrowLeft" || e.key === "ArrowRight") { e.preventDefault(); shiftVariant(e.key === "ArrowLeft" ? -1 : 1); }
});
window.addEventListener("popstate", () => { const next = new URLSearchParams(location.search).get("variant"); setVariant(variants[next] ? next : "hybrid", false); });
$("size").addEventListener("input", resize);
window.addEventListener("resize", resize);
$("count").addEventListener("input", () => { count = Number($("count").value); $("count-value").value = `${count} 只`; focusElapsed = 0; targets(); log(`现在展示 ${count} 个模拟会话。`); render(); });
$("pause").addEventListener("click", () => { paused = !paused; document.body.classList.toggle("paused", paused); $("pause").textContent = paused ? "继续" : "暂停"; $("pause").setAttribute("aria-pressed", String(paused)); log(paused ? "已暂停动画、轮播与自动剧情，可以仔细比较布局。" : "继续播放演示。"); });
$("story").addEventListener("change", () => { storyElapsed = 0; storyStep = 0; log($("story").checked ? "每 6 秒演示一次状态变化；待确认会话优先出现。" : "已关闭自动剧情，仍保留主角和用量轮播。"); });
$("sound").addEventListener("change", () => { if (!$("sound").checked && activeAudio) activeAudio.pause(); log($("sound").checked ? "已开启叫声；下次进入待处理或完成状态时播放。切换布局不会发声。" : "叫声已关闭。"); });
function tick(now) {
  const dt = Math.min(now - lastTime, 100); lastTime = now;
  if (!paused) {
    elapsed += dt; focusElapsed += dt; usageElapsed += dt;
    if (focusElapsed >= 4000) { const q = queue(); focus = q[(q.findIndex((s) => s.id === focus) + 1) % q.length].id; focusElapsed = 0; render(); }
    if (usageElapsed >= 8000) { const groups = availableUsage(); usageIndex = usage.indexOf(groups[(groups.indexOf(usage[usageIndex]) + 1) % groups.length]); usageElapsed = 0; render(); }
    if ($("story").checked) { storyElapsed += dt; if (storyElapsed >= 6000) { storyElapsed = 0; storyTick(); } }
    drawFrames();
  }
  requestAnimationFrame(tick);
}
targets(); render(); resize();
log("演示已开始：每 4 秒轮播；6 秒后出现第一个待确认会话。");
requestAnimationFrame(tick);
