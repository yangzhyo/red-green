// 三套像素皮肤：灯灯（红绿灯头机器人）/ 钳钳（螃蟹）/ 灰灰（猫）。
// 皮肤按项目名哈希确定性分配——同一项目永远同一只。
// 每个状态给出 1-2 帧像素矩阵 + 调色板；动作（跳/探头/摇摆）由 pet.css 负责。
window.SPRITES = (function () {
  var C = {
    running: "#ffc53d",
    awaiting: "#ff4d4f",
    your_turn: "#4096ff",
    completed: "#52c41a",
    aborted: "#8c1d18",
    idle: "#b8b8b8",
    dim: "#3a3e44",
    housing: "#2b2e33",
    steel: "#c9ced6",
    steelDk: "#8a9099",
    ink: "#1c1e22",
    shell: "#e8623d",
    shellDk: "#b34527",
    fur: "#9aa0a8",
    furDk: "#6d737c",
    pink: "#e89aa4",
    white: "#f5f6f8",
  };

  function parse(rows) {
    return rows.map(function (r) {
      return r.split("");
    });
  }
  function clone(g) {
    return g.map(function (r) {
      return r.slice();
    });
  }
  function stamp(g, cells, ch) {
    cells.forEach(function (c) {
      if (g[c[0]]) g[c[0]][c[1]] = ch;
    });
    return g;
  }

  /* ---------- 灯灯 ---------- */
  var ROBOT_BASE = parse([
    "......TT......",
    "....HHHHHH....",
    "....H1111H....",
    "....H1111H....",
    "....H2222H....",
    "....H2222H....",
    "....H3333H....",
    "....H3333H....",
    "....HHHHHH....",
    "......NN......",
    "...BBBBBBBB...",
    "...BEBBBBEB...",
    "...BBBBBBBB...",
    "...BBBBBBBB...",
    "....LL..LL....",
    "....LL..LL....",
  ]);
  var ARMS = {
    down: [[11, 2], [12, 2], [11, 11], [12, 11]],
    up: [[10, 2], [9, 2], [10, 11], [9, 11]],
    typeL: [[11, 2], [12, 2], [10, 11], [11, 11]],
    typeR: [[10, 2], [11, 2], [11, 11], [12, 11]],
    wave: [[11, 2], [12, 2], [9, 12], [10, 11]],
  };
  function robotPal(lampChar, lampColor, tip, blinkOff) {
    var p = {
      T: tip, H: C.housing, N: C.steelDk, L: C.steelDk,
      B: C.steel, E: C.ink, A: C.steelDk,
      1: C.dim, 2: C.dim, 3: C.dim,
    };
    if (!blinkOff) p[lampChar] = lampColor;
    return p;
  }
  function robotState(s) {
    function pose(arms) {
      return stamp(clone(ROBOT_BASE), ARMS[arms], "A");
    }
    switch (s) {
      case "idle":
        return { frames: [{ g: pose("down"), p: robotPal("3", C.idle, C.idle, true) }] };
      case "running":
        return { interval: 210, frames: [
          { g: pose("typeL"), p: robotPal("2", C.running, C.running) },
          { g: pose("typeR"), p: robotPal("2", C.running, C.running) },
        ] };
      case "awaiting":
        return { frames: [{ g: pose("up"), p: robotPal("1", C.awaiting, C.awaiting) }] };
      case "your_turn":
        return { interval: 500, frames: [
          { g: pose("wave"), p: robotPal("3", C.your_turn, C.your_turn) },
          { g: pose("wave"), p: robotPal("3", C.your_turn, C.your_turn, true) },
        ] };
      case "completed":
        return { frames: [{ g: pose("up"), p: robotPal("3", C.completed, C.completed) }] };
      case "aborted":
        return { interval: 420, frames: [
          { g: pose("down"), p: robotPal("1", C.aborted, C.aborted) },
          { g: pose("down"), p: robotPal("1", C.aborted, C.aborted, true) },
        ] };
    }
  }

  /* ---------- 钳钳 ---------- */
  function crabGrid(clawsUp) {
    var g = parse([
      "..E..........E..",
      "..I..........I..",
      "....CCCCCCCC....",
      "...CCCCCCCCCC...",
      "..CCCCCSSCCCCC..",
      "..CCCCCCCCCCCC..",
      "..CCCCCCCCCCCC..",
      "...CCCCCCCCCC...",
      "....C.C..C.C....",
      "...C..C..C..C...",
    ]);
    var claws = clawsUp
      ? [[1, 0], [2, 0], [2, 1], [1, 15], [2, 15], [2, 14]]
      : [[5, 0], [5, 1], [6, 0], [5, 14], [5, 15], [6, 15]];
    return stamp(g, claws, "P");
  }
  function crabPal(lamp, blinkOff) {
    return {
      E: C.white, I: C.shellDk, C: C.shell, P: C.shellDk,
      S: blinkOff ? C.dim : lamp,
    };
  }
  function crabState(s) {
    switch (s) {
      case "idle":
        return { frames: [{ g: crabGrid(false), p: crabPal(C.idle, true) }] };
      case "running":
        return { interval: 210, frames: [
          { g: crabGrid(false), p: crabPal(C.running) },
          { g: crabGrid(true), p: crabPal(C.running) },
        ] };
      case "awaiting":
        return { frames: [{ g: crabGrid(true), p: crabPal(C.awaiting) }] };
      case "your_turn":
        return { interval: 500, frames: [
          { g: crabGrid(true), p: crabPal(C.your_turn) },
          { g: crabGrid(true), p: crabPal(C.your_turn, true) },
        ] };
      case "completed":
        return { frames: [{ g: crabGrid(true), p: crabPal(C.completed) }] };
      case "aborted":
        return { interval: 420, frames: [
          { g: crabGrid(false), p: crabPal(C.aborted) },
          { g: crabGrid(false), p: crabPal(C.aborted, true) },
        ] };
    }
  }

  /* ---------- 灰灰 ---------- */
  var CAT_SIT = parse([
    "..K.......K...",
    "..KK.....KK...",
    "..KKKKKKKKK...",
    "..KpKKKKKpK...",
    "..KEKKKKKEK...",
    "..KKKwKwKKK...",
    "...KKKKKKK....",
    "...KSSSSSK....",
    "...KKKKKKK..K.",
    "...KKKKKKK.KK.",
    "...KKKKKKKKK..",
    "...KKKKKKK....",
    "...KK...KK....",
  ]);
  var CAT_SLEEP = parse([
    "..............",
    "..............",
    "..............",
    "..............",
    "..K.......K...",
    "..KK.....KK...",
    "..KKKKKKKKK...",
    "..KcKKKKKcK...",
    ".KKKKwKwKKKK..",
    "KKKKKKKKKKKKK.",
    "KKKKKKKKKKKKK.",
    ".KKKKKKKKKKK..",
  ]);
  function catPal(bell, closed, blinkOff) {
    return {
      K: C.fur, E: closed ? C.fur : C.ink, c: C.furDk,
      w: C.white, p: C.pink,
      S: blinkOff ? C.furDk : bell,
    };
  }
  function catState(s) {
    switch (s) {
      case "idle":
        return { frames: [{ g: CAT_SLEEP, p: catPal(C.idle, true, true) }] };
      case "running":
        return { frames: [{ g: CAT_SIT, p: catPal(C.running) }] };
      case "awaiting":
        return { frames: [{ g: CAT_SIT, p: catPal(C.awaiting) }] };
      case "your_turn":
        return { interval: 500, frames: [
          { g: CAT_SIT, p: catPal(C.your_turn) },
          { g: CAT_SIT, p: catPal(C.your_turn, false, true) },
        ] };
      case "completed":
        return { frames: [{ g: CAT_SIT, p: catPal(C.completed) }] };
      case "aborted":
        return { interval: 420, frames: [
          { g: CAT_SLEEP, p: catPal(C.aborted, true) },
          { g: CAT_SLEEP, p: catPal(C.aborted, true, true) },
        ] };
    }
  }

  var MAKERS = { robot: robotState, crab: crabState, cat: catState };
  var SKINS = ["robot", "crab", "cat"];

  function pick(project) {
    var h = 0;
    project = project || "";
    for (var i = 0; i < project.length; i++) {
      h = (h * 31 + project.charCodeAt(i)) >>> 0;
    }
    return SKINS[h % SKINS.length];
  }

  function spec(skin, state) {
    return (MAKERS[skin] || MAKERS.robot)(state) || MAKERS.robot("idle");
  }

  // scale 为内部像素倍率，CSS 尺寸取一半以适配 Retina
  function draw(canvas, grid, pal, scale) {
    var h = grid.length,
      w = 0;
    grid.forEach(function (r) {
      w = Math.max(w, r.length);
    });
    canvas.width = w * scale;
    canvas.height = h * scale;
    canvas.style.width = (w * scale) / 2 + "px";
    canvas.style.height = (h * scale) / 2 + "px";
    var ctx = canvas.getContext("2d");
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    for (var y = 0; y < h; y++) {
      for (var x = 0; x < grid[y].length; x++) {
        var col = pal[grid[y][x]];
        if (!col) continue;
        ctx.fillStyle = col;
        ctx.fillRect(x * scale, y * scale, scale, scale);
      }
    }
  }

  return { pick: pick, spec: spec, draw: draw };
})();
