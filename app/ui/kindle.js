// Loaded only on Kindle thread documents. All navigation also works as HTML links.
(function () {
  var reader = document.getElementById('reader');
  if (!window.XMLHttpRequest || !window.JSON || !reader) return;
  var notice = document.getElementById('notice');
  var revision = reader.getAttribute('data-revision');
  var following = reader.getAttribute('data-tail') === 'true';
  var path = location.pathname;

  function atBottom() {
    var top = window.pageYOffset || document.documentElement.scrollTop || document.body.scrollTop;
    var height = window.innerHeight || document.documentElement.clientHeight;
    return top + height >= Math.max(document.body.scrollHeight, document.documentElement.scrollHeight) - 4;
  }

  function show(text) {
    if (notice.textContent !== text) notice.textContent = text;
  }

  function poll() {
    if (document.hidden) { setTimeout(poll, 3000); return; }
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
          if (following && atBottom()) {
            reader.innerHTML = page.html;
            show('');
            window.scrollTo(0, document.body.scrollHeight);
          } else {
            following = false;
            document.getElementById('latest').textContent = '有新消息';
            show('');
          }
          revision = page.revision;
        } else if (xhr.status === 204) { show(''); }
        else { show('连接中断，正在重试'); }
      } catch (error) { show('未能更新，可点「查看最新」重试'); }
      setTimeout(poll, 3000);
    }
    try {
      xhr.open('GET', path, true);
      xhr.setRequestHeader('X-Kindle-Since', revision);
      xhr.onreadystatechange = function () { if (xhr.readyState === 4) finish(); };
      xhr.send(null);
    } catch (error) { finish(); }
  }
  if (following) window.scrollTo(0, document.body.scrollHeight);
  setTimeout(poll, 3000);
}());
