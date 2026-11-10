import fs from "node:fs";
import test from "node:test";
import vm from "node:vm";
import assert from "node:assert/strict";

for (const source of ["codex", "claude"]) {
  test(`${source} 首见静默、前台已阅与退出清理`, async () => {
    const handlers = new Map();
    const calls = [];
    let sessions = [];
    let front = null;
    const context = vm.createContext({
      console, setTimeout, Date, Set, Map, Promise,
      window: { __TAURI__: {
        core: { invoke: async (name, args) => {
          calls.push([name, args]);
          if (name === "get_sessions") return sessions;
          if (name === "frontmost_tty") return front;
          if (name === "get_usage") return null;
        } },
        event: {
          listen: (name, fn) => { handlers.set(name, fn); return Promise.resolve(() => {}); },
          emitTo: () => {},
        },
      } },
    });
    for (const file of ["skins", "calls", "usage-model", "manager"]) {
      vm.runInContext(fs.readFileSync(new URL(`../ui/${file}.js`, import.meta.url), "utf8"), context);
    }
    const settle = async () => {
      for (let i = 0; i < 20; i++) await new Promise((resolve) => setImmediate(resolve));
    };
    const update = async (state, since) => {
      sessions = [{session_id: "test", source, state, since, project: "delta", tty: "/dev/test"}];
      await handlers.get("sessions-changed")({payload: sessions});
      await settle();
    };
    const submissions = () => calls.filter(([name]) => name === "submit_output").map(([,args]) => args);
    const callCount = () => submissions().flatMap((args) => args.calls).length;
    const model = () => submissions().at(-1)?.snapshot.pets[0]?.model;
    await settle();
    await update("completed", "1");
    assert.equal(callCount(), 0);
    await update("running", "2");
    await update("awaiting", "3");
    assert.equal(callCount(), 1);
    // 桌面回退仍使用来源隔离后的窗口身份，真实 pet.js 必须能接受快照。
    const elements = new Map(["sprite", "bubble", "tag"].map((id) => [id, {
      textContent: "", classList: { add() {}, remove() {} },
    }]));
    let onUpdate;
    let drawn = 0;
    const renderer = vm.createContext({
      URLSearchParams, location: { search: `?sid=${source}-test` },
      document: { body: { dataset: {} }, getElementById: (id) => elements.get(id) },
      setInterval: () => 1, clearInterval() {}, installDrag() {},
      window: {
        SPRITES: { SCALE: 1, spec: () => ({ frames: [0] }), draw: () => drawn++ },
        __TAURI__: { core: { invoke() {} }, event: {
          listen: (_, fn) => { onUpdate = fn; return Promise.resolve(); }, emit() {},
        } },
      },
    });
    vm.runInContext(fs.readFileSync(new URL("../ui/pet.js", import.meta.url), "utf8"), renderer);
    onUpdate({ payload: model() });
    assert.equal(elements.get("tag").textContent, "delta");
    assert.equal(drawn, 1);
    onUpdate({ payload: { ...model(), session_id: `${source === "codex" ? "claude" : "codex"}-test`, project: "other" } });
    assert.equal(elements.get("tag").textContent, "delta");
    await update("awaiting", "3");
    assert.equal(callCount(), 1);

    front = "/dev/test";
    await update("running", "4");
    await update("completed", "5");
    assert.equal(model().state, "idle");
    assert.equal(callCount(), 1);
    await update("your_turn", "6");
    assert.equal(model().state, "your_turn");
    assert.equal(callCount(), 1);
    front = null;
    await update("your_turn", "6");
    assert.equal(callCount(), 1);

    sessions = [];
    handlers.get("sessions-changed")({payload: []});
    await settle();
    assert.equal(submissions().at(-1).snapshot.pets.length, 0);
    assert.equal(calls.filter(([name]) => ["play_call","ensure_pet","remove_pet","ensure_gauge","remove_gauge"].includes(name)).length, 0);
  });
}
