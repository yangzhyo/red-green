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
