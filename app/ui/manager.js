// 逻辑中枢：监听状态快照，管理宠物窗口与用量表窗口的生死、叫声和已阅。
// 宠物窗口与用量表都是哑渲染器，一切决策都在这里。
const { invoke } = window.__TAURI__.core;
const { listen, emitTo } = window.__TAURI__.event;

// 叫声：转移进入这些状态时发声，音色属物种、节奏属状态（前台静默规则见 reconcile）
// 状态集与文件名契约在 calls.js，与 gen-calls.mjs 共享同一处定义
const CALL_STATES = new Set(CALLS.STATES);
const ACK_STATES = new Set(["completed", "aborted"]);

// sid -> { slot, prevState, since, acked, model }
const pets = new Map();

function allocSlot() {
  const used = new Set([...pets.values()].map((p) => p.slot));
  let i = 0;
  while (used.has(i)) i++;
  return i;
}

// 由 Rust 心跳（front-tick）更新；隐藏窗口的 JS 定时器会被 WebKit 挂起，
// 所以 manager 只做事件响应，自己不跑 setInterval
let lastFront = null;

// 用量文件的原始内容（usage.json，见 docs/protocol.md）；null = 没有数据
let lastUsage = null;

// 用量属于账号，不属于会话：只有一份视图，交给用量表。
// 重置时刻已过的窗口已清零，下一次响应前没有新值——显示为 0%、重置未知，而不是沿用旧数；
// 所有窗口都已过重置时刻则视同无数据（用量表消失）：文件是旧的，不代表现在还有用量可显示
function usageView(raw, now) {
  if (!raw || typeof raw !== "object") return null;
  const view = {};
  let live = 0;
  for (const key of ["five_hour", "seven_day"]) {
    const w = raw[key];
    if (!w || typeof w.used_percentage !== "number") continue;
    const resetsAt = typeof w.resets_at === "number" ? w.resets_at : null;
    const expired = resetsAt !== null && resetsAt * 1000 <= now;
    if (!expired) live++;
    view[key] = expired
      ? { used_percentage: 0, resets_at: null }
      : { used_percentage: w.used_percentage, resets_at: resetsAt };
  }
  return live ? view : null;
}

// 哑渲染器只认模型：所有推送都经这里，pet-ready 的补发也走同一份
function pushModel(sid, pet, model) {
  pet.model = model;
  emitTo(`pet-${sid}`, "pet-update", model);
}

// 用量表：有宠物在场且有用量数据才出现——它是这群宠物共用的物件，最后一只离场它也走。
// 生死操作串行化：心跳与 usage-changed 可能同时触发，remove 与 ensure 交错会留下"以为在、其实没了"的窗口
let gaugeShown = false;
let gaugeQueue = Promise.resolve();

function syncGauge() {
  gaugeQueue = gaugeQueue.then(async () => {
    const usage = usageView(lastUsage, Date.now());
    const want = pets.size > 0 && usage !== null;
    if (want && !gaugeShown) {
      gaugeShown = true;
      await invoke("ensure_gauge").catch(console.error);
    } else if (!want && gaugeShown) {
      gaugeShown = false;
      await invoke("remove_gauge").catch(console.error);
    }
    if (want) emitTo("usage-gauge", "usage-update", usage);
  });
  return gaugeQueue;
}

async function reconcile(sessions) {
  const seen = new Set();
  const front = lastFront;

  for (const s of sessions) {
    seen.add(s.session_id);
    let pet = pets.get(s.session_id);
    if (!pet) {
      pet = { slot: allocSlot(), prevState: null, since: null, acked: false, model: null };
      pets.set(s.session_id, pet);
      await invoke("ensure_pet", { sid: s.session_id, slot: pet.slot }).catch(
        console.error
      );
    }

    // 新事件（since 变化）重置已阅
    if (s.since !== pet.since) {
      pet.since = s.since;
      pet.acked = false;
    }
    // 已阅：结果被看过的会话显示为空闲
    if (ACK_STATES.has(s.state) && front && front === s.tty) {
      pet.acked = true;
    }
    const effective = pet.acked && ACK_STATES.has(s.state) ? "idle" : s.state;

    // 叫声：仅在真正发生转移、且该会话的终端标签页不在前台时；被静默即消失，不补发。
    // 首见（含 app 启动时既存的会话）只采纳状态不算转移——重启不该把旧状态再叫一遍
    if (pet.prevState !== null && effective !== pet.prevState && CALL_STATES.has(effective)) {
      if (!front || front !== s.tty) {
        invoke("play_call", { name: CALLS.name(SKINS.pick(s.project), effective) });
      }
    }
    pet.prevState = effective;

    // 皮肤在这里定：pet 是哑渲染器，模型给什么画什么
    pushModel(s.session_id, pet, { ...s, state: effective, skin: SKINS.pick(s.project) });
  }

  for (const [sid, pet] of pets) {
    if (!seen.has(sid)) {
      pets.delete(sid);
      invoke("remove_pet", { sid }).catch(console.error);
    }
  }

  await syncGauge();
}

async function tick() {
  const sessions = await invoke("get_sessions").catch(() => []);
  await reconcile(sessions);
}

// 静默/已阅的裁决要用发声那一刻的前台，不能用至多 1.5s 前的心跳旧值
async function refreshFront() {
  lastFront = (await invoke("frontmost_tty").catch(() => null)) ?? null;
}

listen("sessions-changed", async (e) => {
  await refreshFront();
  reconcile(e.payload);
});
// Rust 每 1.5s 推一次前台 tty：驱动已阅检测（聚焦终端不产生文件事件），
// 也兼作兜底轮询修补丢失的窗口事件
listen("front-tick", (e) => {
  lastFront = e.payload ?? null;
  tick();
});
// 目录里任何文件变化都会带来一次 usage-changed（Rust 侧不区分是哪个文件），这里按内容去重
listen("usage-changed", (e) => {
  const raw = e.payload ?? null;
  if (JSON.stringify(raw) === JSON.stringify(lastUsage)) return;
  lastUsage = raw;
  syncGauge();
});
// 用量表就绪后同样自报家门，补发当前视图
listen("gauge-ready", () => {
  syncGauge();
});
// 窗口刚创建时 emitTo 可能先于 pet 的监听器就绪；pet 就绪后自报家门，这里补发最新模型
listen("pet-ready", (e) => {
  const pet = pets.get(e.payload);
  if (pet?.model) pushModel(e.payload, pet, pet.model);
});

(async () => {
  lastUsage = (await invoke("get_usage").catch(() => null)) ?? null;
  await refreshFront();
  await tick();
})();
