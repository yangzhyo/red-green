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

test("设备 Claude 数值长期不变仍显示，重置或缺失时保持未知", () => {
  const raw={claude:{updated_at:"2000-01-01T00:00:00Z",five_hour:{used_percentage:47,resets_at:1200}}};
  assert.equal(USAGE.deviceGroups(raw,new Set(["claude"]),now)[0].windows[0].used_percentage,47);
  for(const data of [null,{five_hour:{used_percentage:47}},{five_hour:{used_percentage:47,resets_at:999}}]) {
    const g=USAGE.deviceGroups({claude:data},new Set(["claude"]),now)[0];
    assert.equal(g.windows[0].used_percentage,null);
  }
  assert.deepEqual(USAGE.deviceGroups(raw,new Set(),now),[]);
});

test("设备 Codex 陈旧数值不伪装成零，窗口保留实际时长与重置文字", () => {
  const fresh=USAGE.deviceGroups({codex:codex()},new Set(["codex"]),now)[0];
  assert.equal(fresh.windows[0].duration_label,"5h");
  assert.ok(fresh.windows[0].reset_label.length>0);
  const stale=USAGE.deviceGroups({codex:codex({updated_at:800})},new Set(["codex"]),now)[0];
  assert.equal(stale.windows[0].used_percentage,null);
});
test("设备保留 Claude 数值最后变化的显示时间", () => {
  const g = USAGE.deviceGroups({claude:{updated_at: 1700000000, five_hour:{used_percentage:10,resets_at:1800000000}}}, new Set(["claude"]), 1750000000000)[0];
  assert.match(g.updated_label, /^数值更新于 \d+\/\d+ \d\d:\d\d$/);
});

const antigravity = (patch = {}) => ({
  updated_at: 990,
  account_id: "user@example.com",
  groups: [
    {
      id: "antigravity:user@example.com:gemini",
      source: "antigravity",
      account_id: "user@example.com",
      label: "Gemini",
      windows: [
        { key: "gemini-5h", used_percentage: 15.13, duration_minutes: 300, resets_at: 1100 },
        { key: "gemini-weekly", used_percentage: 11.07, duration_minutes: 10080, resets_at: 1500 },
      ],
    },
    {
      id: "antigravity:user@example.com:3p",
      source: "antigravity",
      account_id: "user@example.com",
      label: "Claude / GPT",
      windows: [
        { key: "3p-5h", used_percentage: 0, duration_minutes: 300, resets_at: null },
        { key: "3p-weekly", used_percentage: 0, duration_minutes: 10080, resets_at: null },
      ],
    },
  ],
  ...patch,
});

test("三来源并存互不覆盖，Antigravity 保留真实零值、过期或缺失显示未知", () => {
  const raw = {
    codex: codex(),
    antigravity: antigravity(),
    claude: { five_hour: { used_percentage: 47, resets_at: 1200 } },
  };
  const groups = USAGE.groups(raw, new Set(["codex", "antigravity", "claude"]), now);
  assert.deepEqual(groups.map((g) => g.id), [
    "codex:a:codex",
    "antigravity:user@example.com:gemini",
    "antigravity:user@example.com:3p",
    "claude",
  ]);
  assert.equal(groups[1].windows[0].used_percentage, 15.13);
  assert.equal(groups[2].windows[0].used_percentage, 0);
  assert.equal(groups[2].windows[0].resets_at, null);

  // 过期窗口显示未知而非 0%，缺失或空分组显示未知视图
  const expired = USAGE.groups(
    {
      antigravity: antigravity({
        groups: [
          {
            id: "antigravity:u:gemini",
            source: "antigravity",
            label: "Gemini",
            windows: [{ key: "gemini-5h", used_percentage: 12, duration_minutes: 300, resets_at: 999 }],
          },
        ],
      }),
    },
    new Set(["antigravity"]),
    now
  );
  assert.equal(expired[0].windows[0].used_percentage, null);
  assert.equal(expired[0].windows[0].resets_at, null);

  for (const data of [null, { updated_at: 990, groups: [] }]) {
    const unknown = USAGE.groups({ antigravity: data }, new Set(["antigravity"]), now);
    assert.equal(unknown.length, 1);
    assert.equal(unknown[0].id, "antigravity:unknown");
    assert.equal(unknown[0].windows[0].used_percentage, null);
  }
  // 无 Antigravity 宠物在场时不展示其用量
  assert.equal(USAGE.groups(raw, new Set(["claude"]), now).some((g) => g.source === "antigravity"), false);
});

test("设备投影保留 Antigravity 过期状态与数值更新时间", () => {
  const raw = {
    antigravity: antigravity({
      updated_at: 1700000000,
      groups: [
        {
          id: "antigravity:u:gemini",
          source: "antigravity",
          label: "Gemini",
          windows: [
            { key: "gemini-5h", used_percentage: 15, duration_minutes: 300, resets_at: 1749999999 },
            { key: "gemini-weekly", used_percentage: 22, duration_minutes: 10080, resets_at: 1760000000 },
          ],
        },
      ],
    }),
  };
  const g = USAGE.deviceGroups(raw, new Set(["antigravity"]), 1750000000000)[0];
  assert.equal(g.source, "antigravity");
  assert.match(g.updated_label, /^数值更新于 \d+\/\d+ \d\d:\d\d$/);
  assert.equal(g.windows[0].status, "expired");
  assert.equal(g.windows[0].used_percentage, null);
  assert.equal(g.windows[1].status, "known");
  assert.equal(g.windows[1].used_percentage, 22);
});

test("多额度组自动轮播按 8 秒切换，悬停停在当前组，手动切换重置计时且快照刷新不打断周期", () => {
  const r = USAGE.createRotator(8000);
  const g1 = { id: "codex:a:codex", source: "codex" };
  const g2 = { id: "antigravity:u:gemini", source: "antigravity" };
  const g3 = { id: "claude", source: "claude" };

  // 单组不轮播
  r.update([g1], 1000);
  assert.equal(r.current().id, g1.id);
  assert.equal(r.tick(20_000), false);

  // 多组启动后：快照频繁刷新不重置 8 秒周期
  r.update([g1, g2, g3], 20_000);
  assert.equal(r.current().id, g1.id);
  assert.equal(r.tick(27_999), false);
  r.update([g1, g2, g3], 27_999);
  assert.equal(r.tick(28_000), true);
  assert.equal(r.current().id, g2.id);

  // 手动点击切到下一组并重置计时
  r.next(30_000);
  assert.equal(r.current().id, g3.id);
  assert.equal(r.tick(37_999), false);
  assert.equal(r.tick(38_000), true);
  assert.equal(r.current().id, g1.id);

  // 悬停期间不切换，离开悬停后重新计 8 秒
  r.setPaused(true, 40_000);
  assert.equal(r.tick(60_000), false);
  assert.equal(r.current().id, g1.id);
  r.setPaused(false, 60_000);
  assert.equal(r.tick(67_999), false);
  assert.equal(r.tick(68_000), true);
  assert.equal(r.current().id, g2.id);

  // 当前选中组消失时回退首组并重新计时
  r.update([g1, g3], 70_000);
  assert.equal(r.current().id, g1.id);
  assert.equal(r.tick(77_999), false);
  assert.equal(r.tick(78_000), true);
  assert.equal(r.current().id, g3.id);

  // 时钟回拨时不卡死，重锚后仍正常轮播
  assert.equal(r.tick(50_000), false);
  assert.equal(r.tick(58_000), true);
  assert.equal(r.current().id, g1.id);
});
