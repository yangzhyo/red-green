// 三套像素皮肤：灯灯（红绿灯头机器人）/ 钳钳（螃蟹）/ 灰灰（猫）。
// 纯渲染器：皮肤分配（项目名 → 物种）在 skins.js。
// 静态姿态（坐/睡/举手/倒地）全部画在网格里；pet.css 只做周期动作（跳/探头/摇摆）。
// 契约：每个状态网格的首行必须含可见像素——布局把 canvas 顶当作宠物头顶，
// 气泡以恒定间距贴随，顶部空行会让气泡飘离头顶。
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
  // 顺时针旋转 90°：竖姿变横躺（头朝右），与旧 CSS rotate(90deg) 同视觉
  function rot(g) {
    var h = g.length,
      w = 0;
    g.forEach(function (r) {
      w = Math.max(w, r.length);
    });
    var out = [];
    for (var r = 0; r < w; r++) {
      var row = [];
      for (var c = 0; c < h; c++) {
        row.push((g[h - 1 - c] || [])[r] || ".");
      }
      out.push(row);
    }
    return out;
  }
  // 去掉首尾全空行：rot 产物必须重新满足「首行含可见像素」契约
  function trim(g) {
    function blank(r) {
      return r.every(function (ch) {
        return ch === ".";
      });
    }
    var a = 0,
      b = g.length;
    while (a < b && blank(g[a])) a++;
    while (b > a && blank(g[b - 1])) b--;
    return g.slice(a, b);
  }
  function flipV(g) {
    return g.slice().reverse();
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
  var ROBOT_FALLEN = trim(rot(stamp(clone(ROBOT_BASE), ARMS.down, "A")));
  function robotPal(lampChar, lampColor, tip, blinkOff) {
    var p = {
      T: tip, H: C.housing, N: C.steelDk, L: C.steelDk,
      B: C.steel, E: C.ink, A: C.steelDk,
      1: C.dim, 2: C.dim, 3: C.dim,
    };
    if (!blinkOff) p[lampChar] = lampColor;
    return p;
  }
  function robotFrames(s) {
    function pose(arms) {
      return stamp(clone(ROBOT_BASE), ARMS[arms], "A");
    }
    switch (s) {
      case "idle":
        return [{ g: pose("down"), p: robotPal("3", C.idle, C.idle, true) }];
      case "running":
        return [
          { g: pose("typeL"), p: robotPal("2", C.running, C.running) },
          { g: pose("typeR"), p: robotPal("2", C.running, C.running) },
        ];
      case "awaiting":
        return [{ g: pose("up"), p: robotPal("1", C.awaiting, C.awaiting) }];
      case "your_turn":
        return [
          { g: pose("wave"), p: robotPal("3", C.your_turn, C.your_turn) },
          { g: pose("wave"), p: robotPal("3", C.your_turn, C.your_turn, true) },
        ];
      case "completed":
        return [{ g: pose("up"), p: robotPal("3", C.completed, C.completed) }];
      case "aborted":
        return [
          { g: ROBOT_FALLEN, p: robotPal("1", C.aborted, C.aborted) },
          { g: ROBOT_FALLEN, p: robotPal("1", C.aborted, C.aborted, true) },
        ];
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
  var CRAB_FLIPPED = flipV(crabGrid(false));
  function crabPal(lamp, blinkOff) {
    return {
      E: C.white, I: C.shellDk, C: C.shell, P: C.shellDk,
      S: blinkOff ? C.dim : lamp,
    };
  }
  function crabFrames(s) {
    switch (s) {
      case "idle":
        return [{ g: crabGrid(false), p: crabPal(C.idle, true) }];
      case "running":
        return [
          { g: crabGrid(false), p: crabPal(C.running) },
          { g: crabGrid(true), p: crabPal(C.running) },
        ];
      case "awaiting":
        return [{ g: crabGrid(true), p: crabPal(C.awaiting) }];
      case "your_turn":
        return [
          { g: crabGrid(true), p: crabPal(C.your_turn) },
          { g: crabGrid(true), p: crabPal(C.your_turn, true) },
        ];
      case "completed":
        return [{ g: crabGrid(true), p: crabPal(C.completed) }];
      case "aborted":
        return [
          { g: CRAB_FLIPPED, p: crabPal(C.aborted) },
          { g: CRAB_FLIPPED, p: crabPal(C.aborted, true) },
        ];
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
    "..K.......K...",
    "..KK.....KK...",
    "..KKKKKKKKK...",
    "..KcKKKKKcK...",
    ".KKKKwKwKKKK..",
    "KKKKKKKKKKKKK.",
    "KKKKKKKKKKKKK.",
    ".KKKKKKKKKKK..",
  ]);
  // 四脚朝天：头在右、x 眼、耳朵垂在头下，铃铛在颈侧
  var CAT_FALLEN = parse([
    ".K.K...K.K....",
    ".K.K...K.K....",
    "KKKKKKKKKKKKK.",
    "KwwwwwwwwKKKK.",
    "KwwwwwwwwKEKEK",
    "KKKKKKKKKSKwKK",
    ".KKKKKKKKKKKK.",
    "..........K.K.",
  ]);
  function catPal(bell, closed, blinkOff) {
    return {
      K: C.fur, E: closed ? C.fur : C.ink, c: C.furDk,
      w: C.white, p: C.pink,
      S: blinkOff ? C.furDk : bell,
    };
  }
  function catFrames(s) {
    switch (s) {
      case "idle":
        return [{ g: CAT_SLEEP, p: catPal(C.idle, true, true) }];
      case "running":
        return [{ g: CAT_SIT, p: catPal(C.running) }];
      case "awaiting":
        return [{ g: CAT_SIT, p: catPal(C.awaiting) }];
      case "your_turn":
        return [
          { g: CAT_SIT, p: catPal(C.your_turn) },
          { g: CAT_SIT, p: catPal(C.your_turn, false, true) },
        ];
      case "completed":
        return [{ g: CAT_SIT, p: catPal(C.completed) }];
      case "aborted":
        return [
          { g: CAT_FALLEN, p: catPal(C.aborted) },
          { g: CAT_FALLEN, p: catPal(C.aborted, false, true) },
        ];
    }
  }

  var MAKERS = { robot: robotFrames, crab: crabFrames, cat: catFrames };

  // 双帧动画的节律三物种共享：运行中敲击、轮到你/异常中止灯光明灭。
  // 物种只提供各状态的帧（几帧、什么姿态），快慢是状态的语义，不属物种
  var TEMPO = { running: 210, your_turn: 500, aborted: 420 };

  function spec(skin, state) {
    var frames = (MAKERS[skin] || MAKERS.robot)(state) || MAKERS.robot("idle");
    return frames.length > 1
      ? { interval: TEMPO[state], frames: frames }
      : { frames: frames };
  }

  // scale 为内部像素倍率，CSS 尺寸取一半以适配 Retina
  function draw(canvas, frame, scale) {
    var grid = frame.g,
      pal = frame.p;
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

  return { spec: spec, draw: draw };
})();
