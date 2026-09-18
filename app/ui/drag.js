// 拖动手势：整窗既可点也可拖——按下后移动 >4px 进入窗口拖拽，原地松手视为点击。
// 宠物与用量表共用。data-tauri-drag-region 会吞掉 click，无法两者兼得，故手动区分手势。
window.installDrag = function (onClick) {
  const appWindow = window.__TAURI__.window.getCurrentWindow();
  let pressAt = null;

  document.addEventListener("mousedown", (e) => {
    if (e.button !== 0) return;
    pressAt = { x: e.screenX, y: e.screenY };
  });

  document.addEventListener("mousemove", (e) => {
    if (!pressAt) return;
    if (Math.abs(e.screenX - pressAt.x) + Math.abs(e.screenY - pressAt.y) > 4) {
      pressAt = null;
      appWindow.startDragging();
    }
  });

  document.addEventListener("mouseup", () => {
    if (!pressAt) return;
    pressAt = null;
    if (onClick) onClick();
  });
};
