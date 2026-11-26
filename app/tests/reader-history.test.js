import test from 'node:test';
import assert from 'node:assert/strict';
import { hasChrome, renderReader } from './reader-browser.js';

// Run the real reader against a three-page directory and a controllable Mac terminal.
function fixture(scenario) {
  const el = id => document.getElementById(id);
  const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
  const check = (condition, message) => { if (!condition) throw Error(message); };
  async function until(condition, message) {
    for (let i = 0; i < 400 && !condition(); i++) await sleep(20);
    check(condition(), message);
  }
  const mac = {generation: 'one', granted: true, paired: true, live: false, writes: [], reads: [], state: 'idle', fail: 0};
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
    if (url.startsWith('/history/status?generation=')) {
      return url.endsWith('=' + mac.generation) ? response({}) : {ok: false, status: 409};
    }
    if (url === '/history') return response({generation: mac.generation, entries: rows.slice(0, 4), nextCursor: 'A+B/='});
    if (url.startsWith('/history?cursor=') && mac.directoryError) return {ok: false, status: 503};
    if (url === '/history?cursor=A%2BB%2F%3D') return response({generation: mac.generation, entries: rows.slice(4, 8), nextCursor: 'last'});
    if (url === '/history?cursor=last') return response({generation: mac.generation, entries: rows.slice(8), nextCursor: null});
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
      if (mac.holdLive) await new Promise(resolve => { mac.releaseLive = resolve; });
      if (url.startsWith('/sessions/codex-0/') && mac.liveError) return {ok: false, status: mac.liveError};
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
  const stdout = await renderReader('<script>(' + fixture + ')(' + scenario + ')</script>', 50000);
  assert.equal(stdout.match(/<body[^>]*data-test-result="([^"]+)"/)?.[1], 'pass');
}

test('首页、历史页与详情工具条使用各自的导航链接', {skip: !hasChrome}, async () => {
  await runReader(async ({el, until, check}) => {
    const masthead = document.querySelector('.masthead');
    document.body.style.width = '420px';
    function link(container, label) {
      const target = [...container.querySelectorAll('a[href]')].find(a => a.innerText === label && a.getClientRects().length);
      check(target && getComputedStyle(target).textDecorationLine.includes('underline'), '缺少可见的超链接：' + label);
      return target;
    }
    function checkDetail(label) {
      const bar = document.querySelector('footer');
      const back = link(bar, label);
      check(!document.querySelector('.session-head a, .session-head button'), '详情页顶部仍有导航控件');
      for (const width of [320, 420]) {
        document.body.style.width = width + 'px';
        const rect = back.getBoundingClientRect();
        const bounds = bar.getBoundingClientRect();
        check(rect.width >= 44 && rect.height >= 44 && bounds.top >= el('view').getBoundingClientRect().bottom - 1, '返回链接未放入底部触控区');
        const center = r => r.left + r.width / 2;
        const prev = el('prev').getBoundingClientRect(), next = el('next').getBoundingClientRect();
        const action = (el('resume').hidden ? el('open-reply') : el('resume')).getBoundingClientRect();
        check(prev.left === bounds.left && next.right === bounds.right, '翻页按钮没有分居两端');
        check(Math.abs(center(prev) + center(next) - 2 * center(bounds)) < 1, '上一屏和下一屏不对称');
        check(action.width >= 44 && action.height >= 44 && Math.abs(center(rect) + center(action) - 2 * center(bounds)) < 1, '返回和回复操作不对称或触控区域过小');
        const title = el('title').getBoundingClientRect(), state = el('state').getBoundingClientRect();
        check(title.right <= state.left && Math.abs(title.bottom - state.bottom) < 3, '状态未对齐首行标题');
        for (const item of bar.children) {
          if (!item.getClientRects().length) continue;
          const r = item.getBoundingClientRect();
          check(r.left >= bounds.left - 1 && r.right <= bounds.right + 1, '工具条超出窄屏');
          if (item.matches('button, a')) {
            check(item.contains(document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2)), '工具条控件被遮挡：' + item.id);
          }
        }
      }
      return back;
    }
    const home = link(masthead, '返回首页');
    const homeBox = home.getBoundingClientRect();
    const divider = masthead.getBoundingClientRect().bottom;
    const subtitle = el('history-heading');
    check(subtitle?.innerText === '历史' && subtitle.parentElement.classList.contains('wordmark'), '历史没有并入品牌标题');
    check(home.getBoundingClientRect().left >= subtitle.getBoundingClientRect().right, '返回首页未放在标题右侧');
    check(!document.querySelector('.session-head').getClientRects().length, '历史页仍占用第二行标题');
    const filter = el('project-filter');
    const filterTop = filter.getBoundingClientRect().top;
    filter.value = '没有这个项目';
    filter.dispatchEvent(new Event('input'));
    await until(() => el('status').textContent.includes('没有匹配的项目'), '缺少筛选结果提示');
    check(filter.getBoundingClientRect().top === filterTop, '提示挤开了品牌栏下面的筛选框');
    filter.value = '';
    filter.dispatchEvent(new Event('input'));
    const browserEntries = window.history.length;
    el('history-rows').firstChild.click();
    await until(() => el('state').textContent === '只读', '未进入历史详情');
    checkDetail('返回历史').click();
    check(document.body.dataset.page === 'history', '没有返回历史列表');
    link(masthead, '返回首页').click();
    await until(() => !!el('list').firstElementChild, '没有回到首页');
    const historyBox = link(masthead, '历史会话').getBoundingClientRect();
    check(masthead.querySelector('.wordmark').innerText.trim() === 'red-green' && !/桌面伙伴|\d+\s*个会话/.test(document.body.innerText), '首页仍显示多余标题或会话计数');
    check(historyBox.right === homeBox.right && historyBox.top === homeBox.top && masthead.getBoundingClientRect().bottom === divider, '两页的导航或虚线不在同一位置');
    check(el('list').firstElementChild.getBoundingClientRect().top === filterTop && filterTop > divider, '虚线下方留白不同');
    el('list').firstElementChild.click();
    await until(() => el('state').textContent === '空闲' && !el('open-reply').hidden, '未进入可回复的当前会话');
    checkDetail('返回列表').click();
    check(document.body.dataset.page === 'list', '没有返回当前列表');
    check(location.hash === '#list' && window.history.length === browserEntries, '应用内导航增加了浏览器返回步骤');
  });
});

test('历史跨页查项目，正文只读可翻页，继续原会话后可返回原列表位置', {skip: !hasChrome}, async () => {
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
    await until(() => !el('open-reply').hidden, '从历史重进已激活会话时缺少回复入口');
    check(el('resume').hidden && el('composer').hidden && mac.writes.length === 2, '重进会话时重复恢复或自动弹出键盘');
    el('open-reply').click();
    check(field.value === '继续测试' && el('send-reply').disabled, '待核对的草稿可被重复发送或已丢失');
  });
});

test('从历史查看已激活会话时沿用实时状态和设备回复权限', {skip: !hasChrome}, async () => {
  await runReader(async ({el, mac, check, until, refresh}) => {
    mac.live = true;
    mac.granted = false;
    mac.state = 'running';
    el('history-rows').firstChild.click();
    check(el('resume').hidden, '确认终端状态前闪现了继续对话');
    await until(() => el('state').textContent === '运行中', '只读设备未看到实时状态');
    check(el('messages').textContent.includes('当前正文') && el('resume').hidden && el('open-reply').hidden, '只读设备的正文或按钮有误');
    mac.granted = true;
    refresh();
    await until(() => !el('open-reply').hidden, '授权后没有显示回复');
    check(el('open-reply').textContent === '回复' && el('composer').hidden && mac.writes.length === 0, '浏览已激活会话有副作用');
    el('open-reply').click();
    el('reply-text').value = '来自历史入口';
    el('reply-text').dispatchEvent(new Event('input'));
    check(el('send-reply').disabled, '运行中允许重复开始回合');
    mac.granted = false;
    refresh();
    await until(() => el('composer').hidden && el('open-reply').hidden, '撤销授权后输入框仍然打开');
    mac.granted = true;
    mac.state = 'idle';
    refresh();
    await until(() => !el('open-reply').hidden, '重新授权后不能回复');
    el('open-reply').click();
    check(el('reply-text').value === '来自历史入口' && !el('send-reply').disabled, '重新授权后草稿无法发送');
    mac.delay = 100;
    el('send-reply').click();
    el('back').click();
    check(document.body.dataset.page === 'thread' && el('back').getAttribute('aria-disabled') === 'true', '发送中误触返回离开了详情');
    await until(() => mac.writes.length === 1, '点击发送无效');
    check(mac.writes[0].url === '/sessions/codex-0/messages' && mac.writes[0].payload.ticket === 'send-only', '没有向原会话发送');
    await until(() => el('back').getAttribute('aria-disabled') !== 'true', '发送完成后仍不能返回');
  });
});

test('打开历史时终端恰好退出，仍可阅读原对话并按需恢复', {skip: !hasChrome}, async () => {
  await runReader(async ({el, mac, check, until}) => {
    mac.live = true;
    mac.liveError = 410;
    el('history-rows').firstChild.click();
    await until(() => el('messages').textContent.includes('历史正文'), '退出终端后丢失历史正文');
    check(mac.reads.includes('/sessions/codex-0/messages') && !el('resume').hidden && !el('resume').disabled
      && el('open-reply').hidden && mac.writes.length === 0, '终端退出后没有回到可恢复的历史状态');
  });
});

test('正在阅读的实时会话结束后提示结束，重进历史可继续对话', {skip: !hasChrome}, async () => {
  await runReader(async ({el, mac, check, until, refresh}) => {
    mac.live = true;
    el('history-rows').firstChild.click();
    await until(() => !el('open-reply').hidden, '未进入实时会话');
    mac.live = false;
    mac.liveError = 410;
    refresh();
    await until(() => el('status').textContent.includes('会话已结束'), '没有显示当前会话的结束状态');
    check(el('messages').textContent.includes('当前正文') && el('open-reply').hidden, '结束后正文或回复状态不正确');
    el('back').click();
    el('history-rows').firstChild.click();
    await until(() => !el('resume').hidden && !el('resume').disabled, '从历史重进后无法恢复');
    check(mac.writes.length === 0, '阅读已结束会话时启动了终端');
  });
});

test('已激活会话的实况暂不可用时保留正文，恢复后直接显示回复', {skip: !hasChrome}, async () => {
  await runReader(async ({el, mac, check, until, refresh}) => {
    mac.live = true;
    mac.liveError = 503;
    el('history-rows').firstChild.click();
    await until(() => el('messages').textContent.includes('历史正文'), '实况暂不可用时丢失正文');
    check(el('resume').hidden && el('open-reply').hidden && el('status').textContent.includes('暂不可用'), '把实况中断当成终端退出');
    el('prev').click();
    await until(() => el('messages').textContent.includes('较早一页'), '断连后不能向前翻页');
    check(el('resume').hidden && el('status').textContent.includes('暂不可用'), '翻页后误显示继续对话或丢失中断提示');
    mac.liveError = 0;
    el('latest').click();
    refresh();
    await until(() => !el('open-reply').hidden, '实况恢复后缺少回复入口');
    check(el('composer').hidden && mac.writes.length === 0, '自动重试唤起了键盘或终端');
  });
});

test('离开历史正文后，迟到的实时内容不会盖住新打开的会话', {skip: !hasChrome}, async () => {
  await runReader(async ({el, mac, check, until}) => {
    const rows = el('history-rows').children;
    mac.live = mac.holdLive = true;
    rows[0].click();
    await until(() => !!mac.releaseLive, '未请求实时正文');
    el('back').click();
    mac.live = mac.holdLive = false;
    rows[1].click();
    mac.releaseLive();
    await until(() => el('messages').textContent.includes('历史正文'), '迟到的请求影响了下一条会话');
    check(el('title').textContent === '会话标题 1' && !el('resume').hidden && el('open-reply').hidden, '新会话的状态不正确');
  });
});

test('实况请求期间向前翻页，保留正在回看的历史正文', {skip: !hasChrome}, async () => {
  await runReader(async ({el, mac, check, until, refresh}) => {
    el('history-rows').firstChild.click();
    await until(() => !el('resume').disabled, '历史正文未就绪');
    mac.live = mac.holdLive = true;
    refresh();
    await until(() => !!mac.releaseLive, '未开始读取实况');
    el('prev').click();
    mac.holdLive = false;
    mac.releaseLive();
    await until(() => el('messages').textContent.includes('较早一页'), '实况请求覆盖了翻页');
    check(!el('messages').textContent.includes('当前正文'), '回看中被带回了实况');
  });
});

test('恢复出错保留正文，离开后忽略迟到结果，撤销配对清除列表', {skip: !hasChrome}, async () => {
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
    mac.fail = 428;
    el('resume').click();
    await until(() => el('status').textContent.includes('重新开启局域网会话共享'), '系统未返回标签时缺少安全恢复步骤');
    await until(() => !el('resume').disabled, '完成 Mac 端检查后无法继续');
    mac.fail = 0;
    mac.live = true;
    mac.delay = 250;
    el('resume').click();
    el('back').click();
    await sleep(500);
    check(document.body.dataset.page === 'history' && el('composer').hidden && mac.writes.length === 3, '离开后迟到结果仍然打开输入框');
    mac.paired = false;
    refresh();
    await until(() => !el('pair').hidden, '撤销配对后没有清除页面');
    check(!el('history-rows').children.length && !el('messages').children.length, '配对撤销后缓存仍可读取');
  });
});

test('更多历史可重试，等待超时不会重启，历史不再可见会清除缓存', {skip: !hasChrome}, async () => {
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

test('Mac 切换账号时历史列表和分页一起失效', {skip: !hasChrome}, async () => {
  await runReader(async ({el, mac, check, until, refresh}) => {
    mac.generation = 'two';
    refresh();
    await until(() => document.body.dataset.page === 'list', '空闲列表没有发现账号切换');
    check(!el('history-rows').children.length, '旧账号的标题仍然可见');
    el('open-history').click();
    await until(() => el('history-rows').children.length === 4, '无法进入新账号目录');
    mac.generation = 'three';
    el('history-more').click();
    await until(() => document.body.dataset.page === 'list', '下一页来自另一个账号时未清空目录');
    check(!el('history-rows').children.length, '列表混入了其他账号的会话');
  });
});
