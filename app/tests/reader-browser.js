import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import { once } from 'node:events';
import { existsSync } from 'node:fs';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { promisify } from 'node:util';

const chrome = process.env.CHROME_BIN || '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
export const hasChrome = existsSync(chrome);
const ui = new URL('../ui/', import.meta.url);

function escapeHtml(value) {
  return value.replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;').replaceAll("'", '&#39;');
}

// This driver stays in the parent while the reader iframe loads fresh same-origin documents.
async function drive(setup, prepare, scenario, data, startPath) {
  const frame = document.querySelector('iframe');
  const reader = frame.contentWindow;
  const el = id => reader.document.getElementById(id);
  const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
  const check = (condition, message) => { if (!condition) throw Error(message); };
  async function until(condition, message) {
    for (let i = 0; i < 400 && !condition(); i++) await sleep(20);
    check(condition(), message);
  }
  const path = () => reader.location.pathname === '/' ? '/' + reader.location.hash : reader.location.pathname;
  async function navigate(action, target) {
    const old = reader.document;
    action();
    await until(() => reader.document !== old && path() === target && reader.document.readyState === 'complete',
      '未加载新文档 ' + target);
  }
  const refresh = () => reader.dispatchEvent(new Event('online'));
  try {
    const mac = setup(data, sleep);
    window.fetchReader = mac.fetch;
    window.loadedTitles = [];
    await until(() => path() === '/test-note' && reader.document.readyState === 'complete', '未打开测试笔记');
    await navigate(() => el('reader-entry').click(), startPath);
    const ctx = {el, reader, mac, check, until, refresh, navigate, sleep, loadedTitles};
    if (prepare) await prepare(ctx);
    await scenario(ctx);
    document.body.dataset.testResult = 'pass';
  } catch (error) { document.body.dataset.testResult = error.message; }
}

export async function runReaderBrowser({
  setup, prepare, scenario, data = {}, documents, entryPath = '/', startPath = entryPath,
  viewportWidth = 620, viewportHeight = 440, virtualTimeMs = 50000,
}) {
  const template = await readFile(new URL('reader.html', ui), 'utf8');
  const hostScript = '(' + drive + ')(' + [setup, prepare || 'null', scenario, JSON.stringify(data), JSON.stringify(startPath)].join(',') + ')';
  const server = createServer(async (req, res) => {
    const path = new URL(req.url, 'http://localhost').pathname;
    let content, type = 'text/html; charset=utf-8';
    if (path === '/test') {
      content = '<body><iframe src="/test-note" width="' + viewportWidth + '" height="' + viewportHeight
        + '"></iframe><script>' + hostScript + '</script></body>';
    } else if (path === '/test-note') {
      content = '<title>测试笔记</title><a id="reader-entry" href="' + escapeHtml(entryPath) + '">打开阅读器</a>';
    } else if (path === '/' || path === '/archive' || path === '/pair' || Object.hasOwn(documents, path)) {
      const descriptor = documents[path];
      const served = typeof descriptor === 'function' ? descriptor() : descriptor;
      const {entry, title, page, error} = served || (path === '/pair'
        ? {page: 'pair', title: 'red-green · 设备配对'} : path === '/archive'
        ? {page: 'history', title: 'red-green · 历史会话'} : {page: 'list', title: 'red-green · 当前会话'});
      const metadata = entry ? " data-entry='" + escapeHtml(JSON.stringify(entry)) + "'" : '';
      const errorAttribute = error ? ' data-reader-error="' + error + '"' : '';
      content = template.replace(/<title>.*?<\/title>/, () => '<title>' + escapeHtml(title) + '</title>'
        + '<script>parent.loadedTitles.push({path:location.pathname,title:document.title});window.fetch=parent.fetchReader;</script>')
        .replace(/<body[^>]*>/, () => '<body data-page="' + page + '"' + metadata + errorAttribute + '>');
    } else if (['/reader.js', '/reader-model.js', '/reader.css', '/skins.js', '/sprites.js'].includes(path)) {
      type = path.endsWith('.css') ? 'text/css' : 'text/javascript';
      content = await readFile(new URL(path.slice(1), ui));
    } else {
      res.writeHead(404).end();
      return;
    }
    res.writeHead(200, {'Content-Type': type, 'Cache-Control': 'no-store'}).end(content);
  });
  const dir = await mkdtemp(join(tmpdir(), 'nomad-reader-'));
  try {
    server.listen(0, '127.0.0.1');
    await once(server, 'listening');
    const {stdout} = await promisify(execFile)(chrome, [
      '--headless', '--disable-gpu', '--no-first-run', '--window-size=620,440',
      '--virtual-time-budget=' + virtualTimeMs, '--dump-dom', '--user-data-dir=' + join(dir, 'profile'),
      'http://127.0.0.1:' + server.address().port + '/test',
    ], {timeout: 20000});
    assert.equal(stdout.match(/<body[^>]*data-test-result="([^"]+)"/)?.[1], 'pass');
  } finally {
    server.close();
    await rm(dir, {recursive: true, force: true, maxRetries: 3});
  }
}
