// Loaded only on Kindle thread documents. All navigation also works as HTML links.
(function () {
  var reader = document.getElementById('reader');
  if (!reader) return;
  enableReadingGestures();
  if (!window.XMLHttpRequest || !window.JSON) return;

  function enableReadingGestures() {
    var root = document.documentElement;
    if (!root.addEventListener || !window.scrollBy) return;
    var start = null;
    root.addEventListener('touchstart', function (event) {
      start = null;
      if (event.touches.length !== 1) return;
      for (var node = event.target; node && node !== root; node = node.parentNode) {
        if (/^(NAV|A|INPUT|BUTTON|SELECT|TEXTAREA)$/i.test(node.nodeName)) return;
      }
      start = {x: event.touches[0].clientX, y: event.touches[0].clientY, time: event.timeStamp, dragged: false};
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
      if (!horizontal(dx, dy) || (!start.dragged && event.timeStamp - start.time >= 500)) {
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
        if (Math.abs(dx) >= 48 && horizontal(dx, dy) && reader.querySelector) {
          event.preventDefault();
          var link = reader.querySelector(dx < 0 ? 'nav a[rel="next"][href]' : 'nav a[rel="prev"][href]');
          if (link) location.href = link.href;
        }
        return;
      }
      var width = root.clientWidth || window.innerWidth;
      var height = window.innerHeight || root.clientHeight;
      var style = window.getComputedStyle && window.getComputedStyle(reader);
      var lineHeight = parseFloat(style && style.lineHeight) || 27;
      var distance = Math.max(1, height - lineHeight);
      event.preventDefault();
      window.scrollBy(0, origin.x < width / 2 ? -distance : distance);
    }, false);
  }

  var notice = document.getElementById('notice');
  var revision = reader.getAttribute('data-revision');
  var observedRevision = revision;
  var latestRound = reader.getAttribute('data-tail') === 'true';
  var currentTurn = reader.getAttribute('data-turn');
  var path = location.pathname;

  function atBottom() {
    var height = window.innerHeight || document.documentElement.clientHeight;
    return reader.getBoundingClientRect().bottom <= height + 4;
  }

  function show(text) {
    if (notice.textContent !== text) notice.textContent = text;
  }

  function syncReply(url) {
    var link = document.getElementById('kindle-reply');
    if (link && !url) link.parentNode.removeChild(link);
    if (!link && url) {
      var nav = reader.querySelector('nav');
      if (!nav) return;
      link = document.createElement('a');
      link.id = 'kindle-reply';
      link.href = url;
      link.textContent = '回复';
      nav.appendChild(link);
    }
  }

  function poll() {
    if (document.hidden) { setTimeout(poll, 3000); return; }
    var wantBody = latestRound && atBottom();
    var xhr = new XMLHttpRequest();
    var finished = false;
    var timeout = setTimeout(function () { xhr.abort(); finish(); }, 8000);
    function finish() {
      if (finished) return;
      finished = true;
      clearTimeout(timeout);
      if (xhr.status === 401 || xhr.status === 409) { location.replace('/kindle'); return; }
      if (xhr.status === 410) { show('会话已结束'); return; }
      try {
        if (xhr.status === 200) {
          var page = JSON.parse(xhr.responseText);
          syncReply(page.reply);
          if (page.permissionOnly) {
            show('');
          } else {
            if (wantBody && atBottom()) {
              reader.innerHTML = page.html;
              currentTurn = page.turn;
              revision = page.revision;
              show('');
              window.scrollTo(0, document.body.scrollHeight);
            } else {
              latestRound = currentTurn === page.turn;
              if (currentTurn && page.turn && currentTurn !== page.turn) {
                var next = reader.querySelector('nav a[rel="next"][aria-disabled]');
                if (next) {
                  next.href = next.getAttribute('data-href');
                  next.removeAttribute('aria-disabled');
                }
              }
              document.getElementById('latest').textContent = '有新消息';
              show('');
            }
            observedRevision = page.revision;
          }
        } else if (xhr.status === 204) { show(''); }
        else { show('连接中断，正在重试'); }
      } catch (error) { show('未能更新，可点「查看最新」重试'); }
      setTimeout(poll, 3000);
    }
    try {
      xhr.open('GET', path, true);
      xhr.setRequestHeader('X-Kindle-Since', wantBody ? revision : observedRevision);
      if (document.getElementById('kindle-reply')) xhr.setRequestHeader('X-Kindle-Reply', '1');
      if (!wantBody) xhr.setRequestHeader('X-Kindle-Status-Only', '1');
      xhr.onreadystatechange = function () { if (xhr.readyState === 4) finish(); };
      xhr.send(null);
    } catch (error) { finish(); }
  }
  if (latestRound && location.search === '?latest=1') window.scrollTo(0, document.body.scrollHeight);
  setTimeout(poll, 3000);
}());
