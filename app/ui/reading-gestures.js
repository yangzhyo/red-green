// Shared with the Paperwhite browser, which loads classic JavaScript.
window.enableReadingGestures = function (root, onTap, onSwipe) {
  if (!root.addEventListener) return;
  var start = null;
  root.addEventListener('touchstart', function (event) {
    start = null;
    if (event.touches.length !== 1) return;
    var wideTable = false;
    for (var node = event.target; node && node !== root; node = node.parentNode) {
      if (node.nodeName === 'TABLE' && node.scrollWidth > node.clientWidth) wideTable = true;
      if (/^(NAV|A|INPUT|BUTTON|SELECT|TEXTAREA)$/i.test(node.nodeName)) return;
    }
    start = {x: event.touches[0].clientX, y: event.touches[0].clientY, time: event.timeStamp, dragged: false, wideTable: wideTable};
  }, false);

  function horizontal(dx, dy) {
    return Math.abs(dx) > 1.5 * Math.abs(dy);
  }

  root.addEventListener('touchmove', function (event) {
    if (!start) return;
    if (event.touches.length !== 1) { start = null; return; }
    var dx = event.touches[0].clientX - start.x;
    var dy = event.touches[0].clientY - start.y;
    if (Math.abs(dx) <= 12 && Math.abs(dy) <= 12) return;
    if (start.wideTable || !horizontal(dx, dy) || (!start.dragged && event.timeStamp - start.time >= 500)) {
      start = null;
      return;
    }
    start.dragged = true;
    event.preventDefault();
  }, {passive: false});

  root.addEventListener('touchcancel', function () { start = null; }, false);
  root.addEventListener('touchend', function (event) {
    var origin = start;
    start = null;
    if (!origin || event.touches.length || event.changedTouches.length !== 1) return;
    if (!origin.dragged && event.timeStamp - origin.time >= 500) return;
    var dx = event.changedTouches[0].clientX - origin.x;
    var dy = event.changedTouches[0].clientY - origin.y;
    if (origin.dragged || Math.abs(dx) > 12 || Math.abs(dy) > 12) {
      if (!origin.wideTable && Math.abs(dx) >= 48 && horizontal(dx, dy)) {
        event.preventDefault();
        onSwipe(dx < 0);
      }
      return;
    }
    event.preventDefault();
    onTap(origin.x);
  }, {passive: false});
};
