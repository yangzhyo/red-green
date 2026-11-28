// 账号级视图：来源与额度组独立，不随会话数量累加。
globalThis.USAGE = (() => {
  const valid = (n) => typeof n === "number" && Number.isFinite(n) && n >= 0 && n <= 100;
  function antigravityGroups(raw, now, keepReset) {
    const data = raw?.antigravity;
    const added = [];
    for (const group of data?.groups ?? []) {
      added.push({
        ...group,
        source: "antigravity",
        updated_at: data?.updated_at,
        windows: (group.windows ?? []).map((w) => {
          const hasReset = typeof w.resets_at === "number" && Number.isFinite(w.resets_at) && w.resets_at > 0;
          const expired = hasReset && w.resets_at * 1000 <= now;
          const known = valid(w.used_percentage) && !expired;
          return {
            ...w,
            used_percentage: known ? w.used_percentage : null,
            resets_at: hasReset && (known || keepReset) ? w.resets_at : null,
          };
        }),
      });
    }
    if (!added.length) {
      added.push({
        id: "antigravity:unknown",
        source: "antigravity",
        label: "Antigravity",
        updated_at: data?.updated_at,
        windows: [{ key: "primary", used_percentage: null, resets_at: null }],
      });
    }
    return added;
  }
  function groups(raw, sources, now) {
    const result = [];
    if (sources.has("codex")) {
      const data = raw?.codex;
      const fresh = typeof data?.updated_at === "number" && now - data.updated_at * 1000 < 120_000;
      const codexGroups = [];
      for (const group of data?.groups ?? []) {
        codexGroups.push({ ...group, source: "codex", updated_at: data.updated_at, windows: (group.windows ?? []).map((w) => {
          const known = fresh && valid(w.used_percentage) && !(typeof w.resets_at === "number" && w.resets_at * 1000 <= now);
          return { ...w, used_percentage: known ? w.used_percentage : null, resets_at: known ? w.resets_at : null };
        }) });
      }
      if (!codexGroups.length) codexGroups.push({id: "codex:unknown", source: "codex", label: "Codex", updated_at: data?.updated_at, windows: [{key: "primary", used_percentage: null}]});
      result.push(...codexGroups);
    }
    if (sources.has("antigravity")) {
      result.push(...antigravityGroups(raw, now, false));
    }
    if (sources.has("claude")) {
      const windows = [];
      let live = false;
      for (const [key, duration_minutes] of [["five_hour", 300], ["seven_day", 10080]]) {
        const w = raw?.claude?.[key];
        if (!valid(w?.used_percentage)) {
          if (key === "five_hour") windows.push({key, duration_minutes, used_percentage: null, resets_at: null});
          continue;
        }
        const resets_at = typeof w.resets_at === "number" ? w.resets_at : null;
        const expired = resets_at !== null && resets_at * 1000 <= now;
        live ||= !expired;
        windows.push({key, duration_minutes, used_percentage: expired ? 0 : w.used_percentage, resets_at: expired ? null : resets_at});
      }
      if (live) result.push({id: "claude", source: "claude", label: "Claude", windows});
    }
    return result;
  }
  const select = (groups, id) => groups.find((g) => g.id === id) ?? groups[0] ?? null;
  const ROTATE_MS = 8000;
  function createRotator(intervalMs = ROTATE_MS) {
    let groups = [];
    let view = null;
    let since = 0;
    let paused = false;
    return {
      current: () => view,
      groups: () => groups,
      update(nextGroups, now) {
        const prevId = view?.id ?? null;
        const wasSingle = groups.length < 2;
        groups = Array.isArray(nextGroups) ? nextGroups : [];
        view = select(groups, prevId);
        if (wasSingle || groups.length < 2 || view?.id !== prevId) since = now;
        return view;
      },
      next(now) {
        if (groups.length < 2) return view;
        const idx = groups.findIndex((g) => g.id === view?.id);
        view = groups[(idx + 1) % groups.length];
        since = now;
        return view;
      },
      setPaused(nextPaused, now) {
        const flag = Boolean(nextPaused);
        if (flag === paused) return;
        paused = flag;
        if (!paused) since = now;
      },
      tick(now) {
        if (paused || groups.length < 2) return false;
        if (now < since) {
          since = now;
          return false;
        }
        if (now - since < intervalMs) return false;
        this.next(now);
        return true;
      },
    };
  }
  function duration(minutes) {
    if (!(minutes > 0)) return "窗口";
    if (minutes % 1440 === 0) return `${minutes / 1440}d`;
    if (minutes % 60 === 0) return `${minutes / 60}h`;
    return `${minutes}m`;
  }
  // 设备投影不改变桌面兼容行为。Claude / Antigravity 的 updated_at 是数值变化时间，不是心跳。
  function deviceGroups(raw, sources, now) {
    const result = groups(raw, new Set(sources.has("codex") ? ["codex"] : []), now);
    if (sources.has("antigravity")) {
      result.push(...antigravityGroups(raw, now, true));
    }
    if (sources.has("claude")) {
      const windows = [["five_hour",300],["seven_day",10080]].flatMap(([key, duration_minutes], i) => {
        const w=raw?.claude?.[key];
        if (!w && i>0) return [];
        const expiry=typeof w?.resets_at === "number" && Number.isFinite(w.resets_at) ? w.resets_at : null;
        const known=valid(w?.used_percentage) && expiry!==null && expiry*1000>now;
        return [{key,duration_minutes,used_percentage:known?w.used_percentage:null,resets_at:expiry}];
      });
      result.push({id:"claude",source:"claude",label:"Claude",updated_at:raw?.claude?.updated_at,windows});
    }
    const dateLabel=(seconds) => {
      if(typeof seconds!=="number" || !Number.isFinite(seconds)) return "";
      const d=new Date(seconds*1000);
      return Number.isNaN(d.getTime()) ? "" : `${d.getMonth()+1}/${d.getDate()} ${String(d.getHours()).padStart(2,"0")}:${String(d.getMinutes()).padStart(2,"0")}`;
    };
    return result.map(g=>({...g,updated_label:(g.source==="claude"||g.source==="antigravity") ? `数值更新于 ${dateLabel(typeof g.updated_at === "string" ? Date.parse(g.updated_at)/1000 : g.updated_at)||"未知"}` : "",windows:g.windows.map(w=>({...w,
      duration_label:duration(w.duration_minutes),
      reset_label:dateLabel(w.resets_at),
      status:w.used_percentage===null ? (typeof w.resets_at==="number" && w.resets_at>0 && w.resets_at*1000<=now?"expired":"unknown") : "known",
    }))}));
  }
  return {groups, select, duration, deviceGroups, ROTATE_MS, createRotator};
})();
