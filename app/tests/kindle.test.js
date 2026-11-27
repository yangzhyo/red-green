import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {runInNewContext} from 'node:vm';

const script = readFileSync(new URL('../ui/kindle.js', import.meta.url), 'utf8');

function openPage(tail) {
  let scheduled, request, destination;
  const latest = {textContent: '查看最新'};
  const notice = {textContent: ''};
  const reader = {
    innerHTML: '已读正文',
    getAttribute(name) { return name === 'data-tail' ? String(tail) : '版本一'; },
  };
  const document = {
    body: {scrollHeight: 900},
    documentElement: {scrollHeight: 900},
    getElementById(id) { return {reader, notice, latest}[id]; },
  };
  class XHR {
    open(method, path) { assert.equal(method, 'GET'); assert.equal(path, '/kindle/read/demo'); }
    setRequestHeader(name, value) { assert.equal(name, 'X-Kindle-Since'); this.revision = value; }
    send() { request = this; }
  }
  const window = {
    innerHeight: 500, pageYOffset: 0, JSON, XMLHttpRequest: XHR,
    scrollTo() { this.pageYOffset = 400; },
  };
  runInNewContext(script, {
    window, document, XMLHttpRequest: XHR, location: {pathname: '/kindle/read/demo', replace(path) { destination = path; }},
    clearTimeout() {}, setTimeout(fn, delay) { if (delay === 3000) scheduled = fn; },
  });
  return {reader, latest, notice, window, destination: () => destination,
    respond(status, revision, html) {
      scheduled();
      Object.assign(request, {status, responseText: JSON.stringify({revision, html}), readyState: 4});
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
  assert.equal(page.respond(204), '版本二');
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
