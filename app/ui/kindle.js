// Loaded only on Kindle thread documents. All navigation also works as HTML links.
(function () {
  var reader = document.getElementById('reader');
  if (!reader) return;
  if (window.scrollBy) window.enableReadingGestures(document.documentElement, function (x) {
    var width = document.documentElement.clientWidth || window.innerWidth;
    var style = window.getComputedStyle && window.getComputedStyle(reader);
    var lineHeight = parseFloat(style && style.lineHeight) || 27;
    var distance = Math.max(1, readingHeight() - lineHeight);
    window.scrollBy(0, x < width / 2 ? -distance : distance);
  }, function (next) {
    var link = reader.querySelector(next ? 'nav a[rel="next"][href]' : 'nav a[rel="prev"][href]');
    if (link) location.href = link.href;
  });
  if (!window.XMLHttpRequest || !window.JSON) return;

  function readingHeight() {
    var height = window.innerHeight || document.documentElement.clientHeight;
    var nav = document.getElementById('reader-nav');
    return height - (nav ? nav.offsetHeight : 0);
  }

  var notice = document.getElementById('notice');
  var revision = reader.getAttribute('data-revision');
  var observedRevision = revision;
  var latestRound = reader.getAttribute('data-tail') === 'true';
  var currentTurn = reader.getAttribute('data-turn');
  var path = location.pathname;

  function atBottom() {
    return reader.getBoundingClientRect().bottom <= readingHeight() + 4;
  }

  function show(text) {
    if (notice.textContent !== text) notice.textContent = text;
  }

  function syncState(state, label) {
    var badge = document.getElementById('session-state');
    if (badge && label && badge.getAttribute('data-state') !== state) {
      badge.setAttribute('data-state', state);
      badge.textContent = label;
    }
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
      var previous = nav.querySelector('a[rel="prev"]');
      previous.parentNode.insertBefore(link, previous);
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
      if (xhr.status === 410) { syncState('ended', '已结束'); syncReply(null); show('会话已结束'); return; }
      try {
        if (xhr.status === 200) {
          var page = JSON.parse(xhr.responseText);
          syncReply(page.reply);
          syncState(page.state, page.stateLabel);
          if (!page.revision) {
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
      var badge = document.getElementById('session-state');
      if (badge) xhr.setRequestHeader('X-Kindle-State', badge.getAttribute('data-state'));
      if (document.getElementById('kindle-reply')) xhr.setRequestHeader('X-Kindle-Reply', '1');
      if (!wantBody) xhr.setRequestHeader('X-Kindle-Status-Only', '1');
      xhr.onreadystatechange = function () { if (xhr.readyState === 4) finish(); };
      xhr.send(null);
    } catch (error) { finish(); }
  }
  if (latestRound && location.search === '?latest=1') window.scrollTo(0, document.body.scrollHeight);
  setTimeout(poll, 3000);
}());
