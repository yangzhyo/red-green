// 皮肤即会话身份：按项目名哈希确定性分配——同一项目永远同一只。
// 形象（sprites.js）与音色（scripts/gen-calls.mjs）共用这一条轴；
// 浏览器 <script> 与 Node import 都要加载，故挂 globalThis 而非 window。
globalThis.SKINS = (function () {
  var LIST = ["robot", "crab", "cat"];

  function pick(project) {
    var h = 0;
    project = project || "";
    for (var i = 0; i < project.length; i++) {
      h = (h * 31 + project.charCodeAt(i)) >>> 0;
    }
    return LIST[h % LIST.length];
  }

  return { LIST: LIST, pick: pick };
})();
