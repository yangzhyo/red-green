// 状态中枢：已阅、前台静默与皮肤仍在这里裁决；输出由 Rust 模块独占管理。
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const CALL_STATES = new Set(CALLS.STATES);
const ACK_STATES = new Set(["completed", "aborted"]);
const pets = new Map();
let lastFront = null;
let lastUsage = null;
let reconcileQueue = Promise.resolve();

function allocSlot() {
  const used = new Set([...pets.values()].map((p) => p.slot));
  let i = 0;
  while (used.has(i)) i++;
  return i;
}

async function publish(calls = []) {
  const sources = new Set([...pets.values()].map((p) => p.model.source ?? "claude"));
  const now = Date.now();
  await invoke("submit_output", {
    snapshot: {
      pets: [...pets].map(([sid, p]) => ({ sid, slot: p.slot, model: p.model })),
      desktop_usage: USAGE.groups(lastUsage, sources, now),
      device_usage: USAGE.deviceGroups(lastUsage, sources, now),
    },
    calls,
  });
}

function reconcile(sessions) {
  reconcileQueue = reconcileQueue.then(() => applySessions(sessions)).catch(console.error);
  return reconcileQueue;
}

async function applySessions(sessions) {
  const seen = new Set();
  const calls = [];
  const front = lastFront;
  for (const s of sessions) {
    const sid = `${s.source ?? "claude"}-${s.session_id}`;
    seen.add(sid);
    let pet = pets.get(sid);
    if (!pet) {
      pet = { slot: allocSlot(), prevState: null, since: null, acked: false, model: null };
      pets.set(sid, pet);
    }
    if (s.since !== pet.since) { pet.since = s.since; pet.acked = false; }
    if (ACK_STATES.has(s.state) && front && front === s.tty) pet.acked = true;
    const effective = pet.acked && ACK_STATES.has(s.state) ? "idle" : s.state;
    if (pet.prevState !== null && effective !== pet.prevState && CALL_STATES.has(effective) && (!front || front !== s.tty)) {
      calls.push({ sid, name: CALLS.name(SKINS.pick(s.project), effective) });
    }
    pet.prevState = effective;
    pet.model = { ...s, session_id: sid, state: effective, skin: SKINS.pick(s.project) };
  }
  for (const sid of pets.keys()) if (!seen.has(sid)) pets.delete(sid);
  // 一次转移最多提交一次；调用失败也不重投叫声，下一次心跳只补状态。
  await publish(calls);
}

async function tick() {
  try { await reconcile(await invoke("get_sessions")); } catch (e) { console.error(e); }
}
async function refreshFront() {
  lastFront = (await invoke("frontmost_tty").catch(() => null)) ?? null;
}
listen("sessions-changed", async (e) => { await refreshFront(); return reconcile(e.payload); });
// 不在隐藏 WebKit 中运行定时器；Rust front-tick 证明状态中枢仍在处理。
listen("front-tick", (e) => { lastFront = e.payload ?? null; return tick(); });
listen("usage-changed", (e) => {
  const raw = e.payload ?? null;
  if (JSON.stringify(raw) === JSON.stringify(lastUsage)) return;
  lastUsage = raw;
  reconcileQueue = reconcileQueue.then(() => publish()).catch(console.error);
});
for (const event of ["pet-ready", "gauge-ready"]) {
  listen(event, () => invoke("refresh_output").catch(console.error));
}
(async () => {
  lastUsage = (await invoke("get_usage").catch(() => null)) ?? null;
  await refreshFront();
  await tick();
})();
