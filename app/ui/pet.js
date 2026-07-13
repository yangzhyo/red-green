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
  tag.textContent = m.project || "?";
  tag.title = m.detail || "";
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
