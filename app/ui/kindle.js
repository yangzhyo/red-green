// Loaded only on Kindle thread documents. All navigation also works as HTML links.
(function () {
  var reader = document.getElementById('reader');
  if (!reader) return;
  enableTapPaging();
  if (!window.XMLHttpRequest || !window.JSON) return;

  function enableTapPaging() {
    var root = document.documentElement;
    if (!root.addEventListener || !reader.querySelector) return;
    var start = null;
    root.addEventListener('touchstart', function (event) {
      start = null;
      if (event.touches.length !== 1) return;
      for (var node = event.target; node && node !== root; node = node.parentNode) {
        if (/^(NAV|A|INPUT|BUTTON|SELECT|TEXTAREA)$/i.test(node.nodeName)) return;
      }
      start = {x: event.touches[0].clientX, y: event.touches[0].clientY, time: event.timeStamp};
    }, false);

    function moved(origin, point) {
      return Math.abs(point.clientX - origin.x) > 12 || Math.abs(point.clientY - origin.y) > 12;
    }

    root.addEventListener('touchmove', function (event) {
      if (start && (event.touches.length !== 1 || moved(start, event.touches[0]))) start = null;
    }, false);

    root.addEventListener('touchcancel', function () { start = null; }, false);
    root.addEventListener('touchend', function (event) {
      var origin = start;
      start = null;
      if (!origin || event.touches.length || event.changedTouches.length !== 1) return;
      if (event.timeStamp - origin.time >= 500 || moved(origin, event.changedTouches[0])) return;
      var width = root.clientWidth || window.innerWidth;
      var link = reader.querySelector(origin.x < width / 2 ? 'nav a[rel="prev"][href]' : 'nav a[rel="next"][href]');
      if (link) {
        event.preventDefault();
        location.href = link.href;
      }
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
        } else if (xhr.status === 204) { show(''); }
        else { show('连接中断，正在重试'); }
      } catch (error) { show('未能更新，可点「查看最新」重试'); }
      setTimeout(poll, 3000);
    }
    try {
      xhr.open('GET', path, true);
      xhr.setRequestHeader('X-Kindle-Since', wantBody ? revision : observedRevision);
      if (!wantBody) xhr.setRequestHeader('X-Kindle-Status-Only', '1');
      xhr.onreadystatechange = function () { if (xhr.readyState === 4) finish(); };
      xhr.send(null);
    } catch (error) { finish(); }
  }
  if (latestRound && location.search === '?latest=1') window.scrollTo(0, document.body.scrollHeight);
  setTimeout(poll, 3000);
}());
