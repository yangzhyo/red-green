// 哑渲染器：只负责把 manager 发来的宠物模型画出来，点击时请求聚焦终端。
const { invoke } = window.__TAURI__.core;
const { listen, emit } = window.__TAURI__.event;

const sid = new URLSearchParams(location.search).get("sid");

const BUBBLE = {
  idle: "💤",
  awaiting: "🚩❗",
  your_turn: "👋💬",
  completed: "✨",
  aborted: "💥",
};

// Claude Code 风格的 spinner verbs：运行中随机换着显示
const VERBS = [
  "Thinking", "Pondering", "Sauteing", "Simmering", "Brewing",
  "Percolating", "Marinating", "Reticulating", "Noodling", "Herding",
  "Wrangling", "Conjuring", "Scheming", "Crunching", "Hatching",
  "Incubating", "Whirring", "Tinkering", "Mulling", "Churning",
  "Distilling", "Kneading", "Sculpting", "Vibing",
];

let model = null;
let verbTimer = null;
let spriteTimer = null;
let skin = null;

// 用量：名牌下方两行——五小时窗口 / 七天窗口。视图由 manager 裁决（含"重置已过即 0%"），
// 这里只负责格式与配色；没有数据时不显示用量，而不是画成 0%
const WINDOW_ZH = { five_hour: "五小时窗口", seven_day: "七天窗口" };
const WEEKDAY = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const WEEKDAY_ZH = ["周日", "周一", "周二", "周三", "周四", "周五", "周六"];

// 重置时刻的两种说法：胶囊里的缩写与 tooltip 里的中文。
// 五小时窗口只给时分（5 小时内不会跨到歧义的日期）；七天窗口带星期
function resetLabels(ts, key) {
  if (!ts) return { short: "", zh: "重置时刻未知" };
  const d = new Date(ts * 1000);
  const hm = `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
  if (key === "five_hour") return { short: hm, zh: `${hm} 重置` };
  return { short: `${WEEKDAY[d.getDay()]} ${hm}`, zh: `${WEEKDAY_ZH[d.getDay()]} ${hm} 重置` };
}

// 配色随用量升温：绿 → 琥珀 → 红（red-green 本色）
function usageLevel(pct) {
  if (pct >= 85) return "hot";
  if (pct >= 60) return "warn";
  return "ok";
}

function renderUsage(usage) {
  const box = document.getElementById("usage");
  if (!usage) {
    box.hidden = true;
    return;
  }
  const tips = [];
  for (const row of box.querySelectorAll(".row")) {
    const key = row.dataset.window;
    const w = usage[key];
    row.hidden = !w;
    if (!w) continue;
    const pct = Math.round(w.used_percentage);
    row.dataset.level = usageLevel(pct);
    row.style.setProperty("--pct", `${pct}%`);
    row.querySelector(".pct").textContent = `${pct}%`;
    const reset = resetLabels(w.resets_at, key);
    row.querySelector(".reset").textContent = reset.short;
    tips.push(`${WINDOW_ZH[key]}已用 ${pct}% · ${reset.zh}`);
  }
  box.hidden = false;
  // 胶囊里只放得下缩写；全称与重置时刻的完整说法在 tooltip
  box.title = tips.join("\n");
}

function pickVerb() {
  return VERBS[Math.floor(Math.random() * VERBS.length)] + "…";
}

// 只在状态或皮肤变化时重画，避免 1.5s 轮询不断重置动画帧相位
function renderSprite(state) {
  const spec = window.SPRITES.spec(skin, state);
  const cv = document.getElementById("sprite");
  clearInterval(spriteTimer);
  spriteTimer = null;
  // 内部 8px/格、CSS 4px/格（Retina 2x 整数倍，像素不糊）
  const SCALE = 8;
  let i = 0;
  window.SPRITES.draw(cv, spec.frames[0], SCALE);
  if (spec.frames.length > 1) {
    spriteTimer = setInterval(() => {
      i = (i + 1) % spec.frames.length;
      window.SPRITES.draw(cv, spec.frames[i], SCALE);
    }, spec.interval || 400);
  }
}

function render(m) {
  const prev = model?.state;
  model = m;
  document.body.dataset.state = m.state;

  if (m.skin !== skin || m.state !== prev) {
    skin = m.skin;
    document.body.dataset.skin = skin;
    renderSprite(m.state);
  }

  const bubble = document.getElementById("bubble");
  if (m.state === "running") {
    bubble.classList.add("verb");
    if (prev !== "running") {
      bubble.textContent = pickVerb();
      clearInterval(verbTimer);
      verbTimer = setInterval(() => {
        bubble.textContent = pickVerb();
      }, 15000);
    }
  } else {
    bubble.classList.remove("verb");
    clearInterval(verbTimer);
    verbTimer = null;
    bubble.textContent = BUBBLE[m.state] ?? "";
  }

  const tag = document.getElementById("tag");
  const project = m.project || "?";
  tag.textContent = project;
  // 名字截断后窗口内唯一能看全名的地方是这里：全名与近况合并进 tooltip
  tag.title = m.detail ? `${project} — ${m.detail}` : project;

  renderUsage(m.usage ?? null);
}

listen("pet-update", (e) => {
  // Tauri 的 plain listen 是 Any-target：收得到发给所有窗口的事件，必须自筛
  if (e.payload.session_id !== sid) return;
  render(e.payload);
}).then(() => {
  // 首帧握手：窗口刚创建时 manager 的 emitTo 可能先于监听器就绪，
  // 就绪后自报家门请 manager 补发模型——不自取原始文件，已阅/皮肤的裁决只在 manager
  emit("pet-ready", sid);
});

// 整只宠物既可点也可拖：按下后移动 >4px 进入窗口拖拽，原地松手视为点击聚焦。
// （data-tauri-drag-region 会吞掉 click，无法两者兼得，故手动区分手势）
const appWindow = window.__TAURI__.window.getCurrentWindow();
let pressAt = null;

document.addEventListener("mousedown", (e) => {
  if (e.button !== 0) return;
  pressAt = { x: e.screenX, y: e.screenY };
});

document.addEventListener("mousemove", (e) => {
  if (!pressAt) return;
  if (
    Math.abs(e.screenX - pressAt.x) + Math.abs(e.screenY - pressAt.y) > 4
  ) {
    pressAt = null;
    appWindow.startDragging();
  }
});

document.addEventListener("mouseup", () => {
  if (!pressAt) return;
  pressAt = null;
  if (model?.tty) invoke("focus_terminal", { tty: model.tty });
});
