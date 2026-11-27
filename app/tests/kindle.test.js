import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {runInNewContext} from 'node:vm';

const script = readFileSync(new URL('../ui/kindle.js', import.meta.url), 'utf8');

function openPage(tail, search = '?latest=1', navHeight = 0) {
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
    getElementById(id) { return {reader, notice, latest, 'reader-nav': {offsetHeight: navHeight}}[id]; },
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

test('正文末行滚到固定栏上方时才跟随新内容', () => {
  const p = openPage(true, '', 40);
  p.window.pageYOffset = 420;
  p.respond(200, '版本二');
  assert.equal(p.statusOnly(), '1');
  assert.equal(p.reader.innerHTML, '已读正文');
  p.window.pageYOffset = 440;
  p.respond(200, '版本二', '新增正文');
  assert.equal(p.reader.innerHTML, '新增正文');
});

function touchPage(height = 1500, links = {prev: '/older', next: '/newer'}) {
  const window = {
    innerHeight: 500, pageYOffset: 0,
    getComputedStyle() { return {lineHeight: '26px'}; },
    scrollBy(x, dy) { this.pageYOffset = Math.max(0, Math.min(height - this.innerHeight, this.pageYOffset + dy)); },
  };
  const events = {};
  const root = {className: '', clientWidth: 400, addEventListener(type, callback) { events[type] = callback; }};
  const reader = {querySelector(selector) {
    const href = links[selector === 'nav a[rel="prev"][href]' ? 'prev' :
      selector === 'nav a[rel="next"][href]' ? 'next' : ''];
    return href ? {href} : null;
  }};
  const nav = {offsetHeight: 40};
  const location = {href: '/current'};
  runInNewContext(script, {document: {documentElement: root, getElementById(id) { return id === 'reader-nav' ? nav : reader; }},
    window, location});
  const touch = (type, x, y, count = 1, target = root, timeStamp = 100) => {
    const point = {clientX: x, clientY: y};
    events[type]?.({target, timeStamp, touches: Array(count).fill(point), changedTouches: [point], preventDefault() {}});
  };
  return {root, location, touch, window};
}

test('从列表打开长对话和手动换轮都从页首读起，查看最新跳到末尾', () => {
  assert.equal(openPage(true, '').window.pageYOffset, 0);
  assert.equal(openPage(true, '?id=q&step=next').window.pageYOffset, 0);
  assert.equal(openPage(true, '?latest=1').window.pageYOffset, 400);
});

test('Kindle 正文点右下移、点左上移，重叠一行且页首尾都停在同一轮', () => {
  for (const height of [400, 1500]) {
    const p = touchPage(height);
    for (const [x, expected] of [[199, 0], [200, 434], [300, 868], [300, 1000], [350, 1000], [30, 566], [30, 132], [30, 0]]) {
      p.touch('touchstart', x, 100);
      p.touch('touchend', x, 104, 0);
      assert.equal(p.window.pageYOffset, height < 500 ? 0 : expected);
      assert.equal(p.location.href, '/current');
    }
  }
});

test('Kindle 正文左滑下一轮、右滑上一轮，到首尾保留当前轮和滚动位置', () => {
  for (const [end, links, expected] of [[100, {next: '/newer'}, '/newer'], [300, {prev: '/older'}, '/older'],
    [100, {prev: '/older'}, '/current'], [300, {next: '/newer'}, '/current']]) {
    const p = touchPage(1500, links);
    p.window.pageYOffset = 250;
    p.touch('touchstart', 200, 100);
    p.touch('touchmove', (200 + end) / 2, 104);
    p.touch('touchend', end, 108, 0);
    assert.equal(p.location.href, expected);
    assert.equal(p.window.pageYOffset, 250);
  }
});

test('Kindle 拖动、长按、链接、多指和取消触摸都不翻页', () => {
  for (const kind of ['short', 'vertical', 'diagonal', 'scroll-then-sideways', 'drag-back', 'long', 'link', 'nav', 'multi', 'cancel']) {
    const p = touchPage();
    p.window.pageYOffset = 250;
    const target = kind === 'link' || kind === 'nav'
      ? {nodeName: 'SPAN', parentNode: {nodeName: kind === 'link' ? 'A' : 'NAV', parentNode: p.root}} : p.root;
    p.touch('touchstart', 100, 100, 1, target);
    if (kind === 'multi') p.touch('touchstart', 105, 105, 2);
    if (kind === 'cancel') p.touch('touchcancel', 100, 100, 0);
    if (kind === 'drag-back') p.touch('touchmove', 200, 110);
    if (kind === 'scroll-then-sideways') p.touch('touchmove', 102, 140);
    const endX = kind === 'short' ? 130 : ['link','nav','multi','cancel','long','diagonal','scroll-then-sideways'].includes(kind) ? 200 : 100;
    p.touch('touchend', endX, kind === 'vertical' || kind === 'diagonal' ? 185 : 100, 0, target,
      kind === 'long' ? 1100 : 200);
    assert.equal(p.window.pageYOffset, 250, kind);
    assert.equal(p.location.href, '/current', kind);
  }
});
