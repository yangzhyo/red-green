import test from 'node:test';
import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import { existsSync } from 'node:fs';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { promisify } from 'node:util';

const chrome = process.env.CHROME_BIN || '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
const ui = new URL('../ui/', import.meta.url);

// Run the real reader against a three-page directory and a controllable Mac terminal.
function fixture(scenario) {
  const el = id => document.getElementById(id);
  const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
  const check = (condition, message) => { if (!condition) throw Error(message); };
  async function until(condition, message) {
    for (let i = 0; i < 400 && !condition(); i++) await sleep(20);
    check(condition(), message);
  }
  const mac = {granted: true, paired: true, live: false, writes: [], reads: [], state: 'idle', fail: 0};
  const response = data => ({ok: true, json: async () => data});
  const rows = Array.from({length: 24}, (_, i) => ({
    key: 'codex-' + i, title: '会话标题 ' + i, project: i < 8 ? '项目甲' : 'ProjectB',
    updatedAt: 1700000000 - i, source: 'Codex CLI',
  }));
  const body = (id, text) => ({id, order: Number(id), role: 'assistant', text, html: '<p><strong>' + text + '</strong></p>'});
  window.fetch = async (url, opts) => {
    if (!mac.paired) return {ok: false, status: 401};
    if (opts.method === 'POST') {
      mac.writes.push({url, payload: JSON.parse(opts.body)});
      if (mac.delay) await sleep(mac.delay);
      if (mac.fail) return {ok: false, status: mac.fail};
      if (url.endsWith('/resume')) return response({ready: mac.live});
      if (mac.unknown) throw new Error('连接中断');
      return response({});
    }
    mac.reads.push(url);
    if (url === '/sessions') return response([{key: 'codex-home', project: '当前项目', source: 'Codex CLI', state: 'idle'}]);
    if (url === '/history') return response({entries: rows.slice(0, 4), nextCursor: 'A+B/='});
    if (url.startsWith('/history?cursor=') && mac.directoryError) return {ok: false, status: 503};
    if (url === '/history?cursor=A%2BB%2F%3D') return response({entries: rows.slice(4, 8), nextCursor: 'last'});
    if (url === '/history?cursor=last') return response({entries: rows.slice(8), nextCursor: null});
    if (url.startsWith('/history/')) {
      if (mac.removed) return {ok: false, status: 404};
      const older = url.includes('before=');
      const page = {messages: url.includes('since=') ? [] : [older ? body('0', '较早一页') : body('1', '历史正文')],
        since: 'history-1', live: mac.live};
      if (!url.includes('since=')) page.before = older ? null : 'history-0';
      if (mac.granted) page.resumeTicket = 'resume-only';
      return response(page);
    }
    if (url.startsWith('/sessions/')) {
      const page = {messages: [body('2', '当前正文')], since: 'live-1', state: mac.state};
      if (mac.granted) page.replyTicket = 'send-only';
      return response(page);
    }
    return {ok: false, status: 404};
  };
  const refresh = () => window.dispatchEvent(new Event('online'));
  window.addEventListener('load', async () => {
    try {
      await until(() => el('title').textContent === '当前项目', '首页未自动打开唯一当前会话');
      el('back').click();
      await until(() => !el('open-history').hidden, '返回首页后没有历史入口');
      el('open-history').click();
      await until(() => el('history-rows').children.length === 4, '历史未按页加载');
      await scenario({el, mac, check, until, refresh, sleep});
      document.body.dataset.testResult = 'pass';
    } catch (error) { document.body.dataset.testResult = error.message; }
  });
}

async function runReader(scenario) {
  const dir = await mkdtemp(join(tmpdir(), 'nomad-history-'));
  try {
    const html = await readFile(new URL('reader.html', ui), 'utf8');
    const script = '<base href="' + ui.href + '"><script>(' + fixture + ')(' + scenario + ')</script>';
    const file = join(dir, 'reader.html');
    await writeFile(file, html.replaceAll('src="/', 'src="').replaceAll('href="/', 'href="').replace('<head>', '<head>' + script));
    const {stdout} = await promisify(execFile)(chrome, [
      '--headless', '--disable-gpu', '--no-first-run', '--allow-file-access-from-files', '--window-size=620,440',
      '--virtual-time-budget=50000', '--dump-dom', '--user-data-dir=' + join(dir, 'profile'), pathToFileURL(file).href,
    ], {timeout: 20000});
    assert.equal(stdout.match(/<body[^>]*data-test-result="([^"]+)"/)?.[1], 'pass');
  } finally {
    await rm(dir, {recursive: true, force: true, maxRetries: 3});
  }
}

test('历史跨页查项目，正文只读可翻页，继续原会话后可返回原列表位置', {skip: !existsSync(chrome)}, async () => {
  await runReader(async ({el, mac, check, until, refresh}) => {
    const query = el('project-filter');
    query.value = 'projectb';
    query.dispatchEvent(new Event('input'));
    await until(() => el('history-rows').children.length === 24, '未跨过没有匹配项的一页');
    const matches = [...el('history-rows').children].filter(row => !row.hidden);
    check(matches.length === 16 && matches[0].firstChild.textContent === '会话标题 8', '筛选结果或顺序错误');
    check(matches[0].lastChild.textContent.includes('ProjectB · 2023'), '缺少项目或本地时间');
    el('view').scrollTop = 190;
    const top = el('view').scrollTop;
    matches[4].click();
    await until(() => !!document.querySelector('#messages strong'), '缺少 Markdown 正文');
    check(el('open-reply').hidden && el('composer').hidden && !el('resume').disabled, '查看历史时回复入口错误');
    el('prev').click();
    await until(() => el('messages').textContent.includes('较早一页'), '历史正文无法加载前页');
    check(mac.writes.length === 0, '浏览或翻页时启动了终端');
    el('resume').click();
    el('resume').click();
    await until(() => el('status').textContent.includes('等待 Mac'), '没有等待终端就绪');
    check(el('composer').hidden && mac.writes.length === 1, '提前打开输入框或重复启动');
    check(mac.writes[0].url === '/history/codex-12/resume' && mac.writes[0].payload.ticket === 'resume-only', '恢复了错误的会话');
    mac.live = true;
    mac.state = 'awaiting';
    refresh();
    await until(() => !el('composer').hidden, '终端就绪后未打开输入框');
    check(el('messages').textContent.includes('当前正文') && el('reply-info').textContent.includes('Mac 终端'), '未读取实况或缺少审批提示');
    const field = el('reply-text');
    field.value = '继续测试';
    field.dispatchEvent(new Event('input'));
    check(el('send-reply').disabled, '待审批状态仍可发送');
    mac.state = 'idle';
    refresh();
    await until(() => !el('send-reply').disabled, '回合结束后不能发送');
    mac.unknown = true;
    el('send-reply').click();
    await until(() => el('reply-info').textContent.includes('待核对'), '未知发送结果没有保留');
    check(mac.writes[1].url === '/sessions/codex-12/messages' && mac.writes[1].payload.ticket === 'send-only', '未使用当前会话的发送凭据');
    el('back').click();
    check(document.body.dataset.page === 'history' && query.value === 'projectb' && el('view').scrollTop === top && top > 0,
      '返回后筛选或滚动位置丢失');
    matches[4].click();
    await until(() => !el('resume').disabled, '不能再次继续会话');
    el('resume').click();
    await until(() => !el('composer').hidden, '已运行的会话未重新打开');
    check(field.value === '继续测试' && el('send-reply').disabled, '待核对的草稿可被重复发送或已丢失');
  });
});

test('恢复出错保留正文，离开后忽略迟到结果，撤销配对清除列表', {skip: !existsSync(chrome)}, async () => {
  await runReader(async ({el, mac, check, until, refresh, sleep}) => {
    mac.granted = false;
    el('history-rows').firstChild.click();
    await until(() => el('status').textContent.includes('开启回复'), '只读设备缺少权限提示');
    check(el('resume').disabled, '只读设备可以恢复');
    mac.granted = true;
    refresh();
    await until(() => !el('resume').disabled, '授权没有生效');
    mac.fail = 422;
    el('resume').click();
    await until(() => el('status').textContent.includes('项目目录'), '目录丢失没有提示');
    check(el('messages').textContent.includes('历史正文') && el('composer').hidden, '失败后没有保留正文');
    await until(() => !el('resume').disabled, '找不到重试入口');
    mac.fail = 0;
    mac.live = true;
    mac.delay = 250;
    el('resume').click();
    el('back').click();
    await sleep(500);
    check(document.body.dataset.page === 'history' && el('composer').hidden && mac.writes.length === 2, '离开后迟到结果仍然打开输入框');
    mac.paired = false;
    refresh();
    await until(() => !el('pair').hidden, '撤销配对后没有清除页面');
    check(!el('history-rows').children.length && !el('messages').children.length, '配对撤销后缓存仍可读取');
  });
});

test('更多历史可重试，等待超时不会重启，历史不再可见会清除缓存', {skip: !existsSync(chrome)}, async () => {
  await runReader(async ({el, mac, check, until, refresh, sleep}) => {
    mac.directoryError = true;
    el('history-more').click();
    await until(() => el('history-more').textContent === '重试', '未提示目录读取错误');
    check(el('history-rows').children.length === 4, '读取失败丢弃了已读目录');
    mac.directoryError = false;
    el('history-more').click();
    await until(() => el('history-rows').children.length === 8, '重试没有使用原游标');
    el('history-rows').firstChild.click();
    await until(() => !el('resume').disabled, '历史页缺少恢复凭据');
    el('resume').click();
    await sleep(33000);
    check(!el('resume').disabled && el('status').textContent.includes('尚未连上终端'), '没有在有限等待后回到可重试状态');
    check(mac.writes.length === 1 && el('composer').hidden, '超时后重复启动或打开了输入框');
    mac.removed = true;
    refresh();
    await until(() => !el('open-history').hidden && document.body.dataset.page === 'list', '没有退出失效的会话');
    check(!el('messages').children.length && !el('history-rows').children.length, '已失效的会话仍然可见');
  });
});
