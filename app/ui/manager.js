// 逻辑中枢：监听状态快照，管理宠物窗口的生死、叫声和已阅。
// 宠物窗口是哑渲染器，一切决策都在这里。
const { invoke } = window.__TAURI__.core;
const { listen, emitTo } = window.__TAURI__.event;

// 叫声：转移进入这些状态时发声，音色属物种、节奏属状态（前台静默规则见 reconcile）
// 状态集与文件名契约在 calls.js，与 gen-sounds.mjs 共享同一处定义
const CALL_STATES = new Set(CALLS.STATES);
const ACK_STATES = new Set(["completed", "aborted"]);

// sid -> { slot, prevState, since, acked }
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

async function reconcile(sessions) {
  const seen = new Set();
  const front = lastFront;

  for (const s of sessions) {
    seen.add(s.session_id);
    let pet = pets.get(s.session_id);
    if (!pet) {
      pet = { slot: allocSlot(), prevState: null, since: null, acked: false };
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

    // 叫声：仅在真正发生转移、且该会话的终端标签页不在前台时；被静默即消失，不补发
    if (effective !== pet.prevState && CALL_STATES.has(effective)) {
      if (!front || front !== s.tty) {
        invoke("play_sound", { name: CALLS.name(SKINS.pick(s.project), effective) });
      }
    }
    pet.prevState = effective;

    emitTo(`pet-${s.session_id}`, "pet-update", { ...s, state: effective });
  }

  for (const [sid, pet] of pets) {
    if (!seen.has(sid)) {
      pets.delete(sid);
      invoke("remove_pet", { sid }).catch(console.error);
    }
  }
}

async function tick() {
  const sessions = await invoke("get_sessions").catch(() => []);
  await reconcile(sessions);
}

listen("sessions-changed", (e) => reconcile(e.payload));
// Rust 每 1.5s 推一次前台 tty：驱动已阅检测（聚焦终端不产生文件事件），
// 也兼作兜底轮询修补丢失的窗口事件
listen("front-tick", (e) => {
  lastFront = e.payload ?? null;
  tick();
});
tick();
