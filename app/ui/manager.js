// 逻辑中枢：监听状态快照，管理宠物窗口的生死、声音和已阅。
// 宠物窗口是哑渲染器，一切决策都在这里。
const { invoke } = window.__TAURI__.core;
const { listen, emitTo } = window.__TAURI__.event;

// 状态转移进入这两个状态时发声（前台静默规则见 reconcile）
const SOUND = { awaiting: "Glass", aborted: "Basso" };
const ACK_STATES = new Set(["completed", "aborted"]);

// sid -> { slot, prevState, since, acked }
const pets = new Map();

function allocSlot() {
  const used = new Set([...pets.values()].map((p) => p.slot));
  let i = 0;
  while (used.has(i)) i++;
  return i;
}

async function reconcile(sessions) {
  const seen = new Set();
  const front = await invoke("frontmost_tty").catch(() => null);

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

    // 声音：仅在真正发生转移、且该会话的终端标签页不在前台时
    if (effective !== pet.prevState && SOUND[effective]) {
      if (!front || front !== s.tty) {
        invoke("play_sound", { name: SOUND[effective] });
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
// 轮询兜底：驱动已阅检测（聚焦终端不产生文件事件），也修补丢失的窗口事件
setInterval(tick, 1500);
tick();
