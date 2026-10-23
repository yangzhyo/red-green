import test from "node:test";
import assert from "node:assert/strict";
import "../ui/usage-model.js";

const now = 1000_000;
const codex = (patch = {}) => ({ updated_at: 990, groups: [{ id: "codex:a:codex", source: "codex", label: "Codex", windows: [{ key: "primary", used_percentage: 23, duration_minutes: 300, resets_at: 1100 }] }], ...patch });

test("两来源分组，多个会话不累加，默认优先 Codex", () => {
  const raw = { codex: codex(), claude: { five_hour: { used_percentage: 47, resets_at: 1200 } } };
  const groups = USAGE.groups(raw, new Set(["codex", "claude"]), now);
  assert.equal(groups.length, 2);
  assert.equal(groups[0].windows[0].used_percentage, 23);
  assert.equal(groups[1].windows[0].used_percentage, 47);
  assert.equal(USAGE.select(groups, null).source, "codex");
  assert.equal(USAGE.select(groups, "claude").source, "claude");
  assert.equal(USAGE.groups(raw, new Set(["claude"]), now).length, 1);
});

test("Codex 过期、陈旧、缺失都未知，真实零值仍是零", () => {
  for (const data of [null, codex({updated_at: 800}), codex({groups: [{id: "c", source: "codex", windows: [{key: "primary", used_percentage: 9, resets_at: 999}]}]})]) {
    const group = USAGE.groups({codex: data}, new Set(["codex"]), now)[0];
    assert.equal(group.windows[0].used_percentage, null);
  }
  const data = codex();
  data.groups[0].windows[0].used_percentage = 0;
  assert.equal(USAGE.groups({codex: data}, new Set(["codex"]), now)[0].windows[0].used_percentage, 0);
  const empty = USAGE.groups({codex: codex({groups: []})}, new Set(["codex"]), now)[0];
  assert.equal(empty.updated_at, 990);
});

test("额度组按原始时长展示，账号切换不会保留旧选择", () => {
  assert.equal(USAGE.duration(300), "5h");
  assert.equal(USAGE.duration(10080), "7d");
  assert.equal(USAGE.duration(90), "90m");
  assert.equal(USAGE.duration(null), "窗口");
  const groups = USAGE.groups({codex: codex()}, new Set(["codex"]), now);
  assert.equal(USAGE.select(groups, "codex:old:codex").id, "codex:a:codex");
  assert.deepEqual(USAGE.groups({codex: codex()}, new Set(), now), []);
});

test("Claude 保持原有重置规则，没有有效窗口则不出现", () => {
  assert.deepEqual(USAGE.groups({claude: {five_hour: {used_percentage: 40, resets_at: 999}}}, new Set(["claude"]), now), []);
  const raw = {claude: {five_hour: {used_percentage: 40, resets_at: 999}, seven_day: {used_percentage: 20, resets_at: 1200}}};
  const group = USAGE.groups(raw, new Set(["claude"]), now)[0];
  assert.equal(group.windows[0].used_percentage, 0);
  assert.equal(group.windows[0].resets_at, null);
});

test("Claude 缺失五小时窗口时，七天用量仍留在第二圈", () => {
  const group = USAGE.groups({claude: {seven_day: {used_percentage: 45, resets_at: 1200}}}, new Set(["claude"]), now)[0];
  assert.equal(group.windows[0].used_percentage, null);
  assert.equal(group.windows[1].duration_minutes, 10080);
  assert.equal(group.windows[1].used_percentage, 45);
});
