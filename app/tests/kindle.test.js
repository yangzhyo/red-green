import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {runInNewContext} from 'node:vm';

const script = readFileSync(new URL('../ui/kindle.js', import.meta.url), 'utf8');

function openPage(tail, search = '?latest=1') {
  let scheduled, request, destination;
  const latest = {textContent: '查看最新'};
  const notice = {textContent: ''};
  const reader = {
    innerHTML: '已读正文',
    getBoundingClientRect() { return {bottom: 900 - window.pageYOffset}; },
    getAttribute(name) { return name === 'data-tail' ? String(tail) : '版本一'; },
  };
  const document = {
    body: {get scrollHeight() { return notice.textContent ? 950 : 900; }},
    documentElement: {scrollHeight: 900},
    getElementById(id) { return {reader, notice, latest}[id]; },
  };
  class XHR {
    open(method, path) { assert.equal(method, 'GET'); assert.equal(path, '/kindle/read/demo'); }
    setRequestHeader(name, value) {
      if (name === 'X-Kindle-Status-Only') this.statusOnly = value;
      else { assert.equal(name, 'X-Kindle-Since'); this.revision = value; }
    }
    send() { request = this; }
  }
  const window = {
    innerHeight: 500, pageYOffset: 0, JSON, XMLHttpRequest: XHR,
    scrollTo() { this.pageYOffset = 400; },
  };
  runInNewContext(script, {
    window, document, XMLHttpRequest: XHR, location: {pathname: '/kindle/read/demo', search, replace(path) { destination = path; }},
    clearTimeout() {}, setTimeout(fn, delay) { if (delay === 3000) scheduled = fn; },
  });
  return {reader, latest, notice, window, destination: () => destination,
    statusOnly: () => request.statusOnly,
    respond(status, revision, html, turn = '版本一') {
      scheduled();
      Object.assign(request, {status, responseText: JSON.stringify({revision, html, turn}), readyState: 4});
      request.onreadystatechange();
      return request.revision;
    },
  };
}

test('Kindle 只在末尾替换正文，断网后接续更新，撤销后回到配对入口', () => {
  const page = openPage(true);
  assert.equal(page.respond(200, '版本二', '新增正文'), '版本一');
  assert.equal(page.reader.innerHTML, '新增正文');
  page.respond(0);
  assert.equal(page.reader.innerHTML, '新增正文');
  assert.match(page.notice.textContent, /重试/);
  assert.equal(page.respond(200, '版本三', '重连后追加'), '版本二');
  assert.equal(page.reader.innerHTML, '重连后追加');
  assert.equal(page.notice.textContent, '');
  page.respond(401);
  assert.equal(page.destination(), '/kindle');
});

test('Kindle 回翻到旧页或在末页向上滚动时提示新消息并保留位置', () => {
  for (const tail of [true, false]) {
    const page = openPage(tail);
    page.window.pageYOffset = tail ? 100 : 400;
    page.respond(200, '版本二', '下一段');
    assert.equal(page.reader.innerHTML, '已读正文');
    assert.equal(page.latest.textContent, '有新消息');
    assert.equal(page.window.pageYOffset, tail ? 100 : 400);
  }
});

test('阅读长轮时仅查询更新状态，滚至末尾再取整轮正文', () => {
  const p = openPage(true, '');
  p.respond(200, '版本二');
  assert.equal(p.statusOnly(), '1');
  assert.equal(p.reader.innerHTML, '已读正文');
  assert.equal(p.respond(204), '版本二');
  p.window.pageYOffset = 400;
  assert.equal(p.respond(200, '版本二', '完整的新正文'), '版本一');
  assert.equal(p.statusOnly(), undefined);
  assert.equal(p.reader.innerHTML, '完整的新正文');
});

function touchPage(links = {prev: '/older', next: '/newer'}, window = {ontouchstart: null}) {
  const events = {};
  const root = {className: '', clientWidth: 400, addEventListener(type, callback) { events[type] = callback; }};
  const reader = {querySelector(selector) {
    const href = links[selector.includes('prev') ? 'prev' : 'next'];
    return href ? {href} : null;
  }};
  const location = {href: '/current'};
  runInNewContext(script, {document: {documentElement: root, getElementById() { return reader; }},
    window, location});
  const touch = (type, x, y, count = 1, target = root, timeStamp = 100) => {
    const point = {clientX: x, clientY: y};
    events[type]?.({target, timeStamp, touches: Array(count).fill(point), changedTouches: [point], preventDefault() {}});
  };
  return {root, location, touch};
}

test('Paperwhite 未声明触摸标记也能翻页，轻触不会切换导航样式', () => {
  const p = touchPage(undefined, {});
  p.touch('touchstart', 90, 100);
  p.touch('touchend', 92, 102, 0);
  assert.equal(p.location.href, '/older');
  assert.equal(p.root.className, '');
});

test('从列表打开长对话和手动换轮都从页首读起，查看最新跳到末尾', () => {
  assert.equal(openPage(true, '').window.pageYOffset, 0);
  assert.equal(openPage(true, '?id=q&step=next').window.pageYOffset, 0);
  assert.equal(openPage(true, '?latest=1').window.pageYOffset, 400);
});

test('Kindle 左半屏点按回看，右半屏点按向后读，首尾没有相邻页就停留', () => {
  for (const [x, links, url] of [[199, {prev:'/older'}, '/older'], [200, {next:'/newer'}, '/newer'],
    [40, {next:'/newer'}, '/current'], [360, {prev:'/older'}, '/current']]) {
    const p = touchPage(links);
    p.touch('touchstart', x, 100);
    p.touch('touchend', x, 104, 0);
    assert.equal(p.location.href, url);
  }
});

test('Kindle 拖动、长按、链接、多指和取消触摸都不翻页', () => {
  for (const kind of ['horizontal', 'vertical', 'drag-back', 'long', 'link', 'multi', 'cancel']) {
    const p = touchPage();
    const target = kind === 'link' ? {nodeName: 'SPAN', parentNode: {nodeName: 'A', parentNode: p.root}} : p.root;
    p.touch('touchstart', 100, 100, 1, target);
    if (kind === 'multi') p.touch('touchstart', 105, 105, 2);
    if (kind === 'cancel') p.touch('touchcancel', 100, 100, 0);
    if (kind === 'drag-back') p.touch('touchmove', 200, 110);
    p.touch('touchend', kind === 'horizontal' ? 160 : 100, kind === 'vertical' ? 160 : 100, 0, target,
      kind === 'long' ? 1100 : 200);
    assert.equal(p.location.href, '/current', kind);
  }
});
