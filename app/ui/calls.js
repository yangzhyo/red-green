// 叫声契约：哪些状态有叫声、资产文件名如何拼（皮肤-状态，见 CONTEXT.md「叫声」）。
// 发声决策（manager.js）与资产生成（scripts/gen-sounds.mjs）共用这一处定义；
// 浏览器 <script> 与 Node import 都要加载，故挂 globalThis 而非 window。
globalThis.CALLS = (function () {
  var STATES = ["awaiting", "aborted", "your_turn", "completed"];

  function name(skin, state) {
    return skin + "-" + state;
  }

  return { STATES: STATES, name: name };
})();
