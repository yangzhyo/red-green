import test from 'node:test';
import { hasChrome, runReaderBrowser } from './reader-browser.js';

const current = {key: 'codex-home', title: '正在处理的任务', project: '当前项目', source: 'Codex CLI', state: 'idle'};
const rows = Array.from({length: 24}, (_, i) => ({
  key: 'codex-' + i, title: '会话标题 ' + i, project: i < 8 ? '项目甲' : 'ProjectB',
  updatedAt: 1700000000 - i, source: 'Codex CLI',
}));

function setup({current, rows, paired}, sleep) {
  const mac = {generation: 'one', granted: true, paired, live: false, writes: [], reads: [], state: 'idle', fail: 0, sessions: [current]};
  const response = data => ({ok: true, json: async () => structuredClone(data)});
  const body = (id, text) => ({id, order: Number(id), startsTurn: true, role: 'assistant', text, html: '<p><strong>' + text + '</strong></p>'});
  mac.fetch = async (url, opts) => {
    if (url === '/pair' && opts.method === 'POST') {
      mac.writes.push({url, payload: JSON.parse(opts.body)});
      mac.paired = true;
      return response({});
    }
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
    if (url === '/read/current/codex-home' && opts.headers?.Accept === 'application/json') {
      return mac.metadataReady ? response(current) : {ok: false, status: 503};
    }
    if (url === '/sessions') return response(mac.sessions);
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
        since: 'history-1', live: mac.live, state: mac.live ? mac.state : null};
      if (!url.includes('since=')) page.before = older ? null : 'history-0';
      if (mac.granted) page.resumeTicket = 'resume-only';
      return response(page);
    }
    if (url.startsWith('/sessions/')) {
      if (mac.holdLive) await new Promise(resolve => { mac.releaseLive = resolve; });
      if (url.startsWith('/sessions/codex-0/') && mac.liveError) return {ok: false, status: mac.liveError};
      const page = {messages: url.includes('since=') ? [] : [body('2', '当前正文')], since: 'live-1', state: mac.state};
      if (url.startsWith('/sessions/codex-home/')) page.title = mac.sessions[0].title;
      if (mac.granted) page.replyTicket = 'send-only';
      return response(page);
    }
    return {ok: false, status: 404};
  };
  return mac;
}

const documents = Object.fromEntries(rows.map(entry => ['/read/history/' + entry.key, {
  entry: {...entry, state: null}, page: 'thread', title: 'Codex CLI · 只读 · ' + entry.project + ' · ' + entry.title,
}]));
documents['/read/current/codex-home'] = {entry: current, page: 'thread', title: 'Codex CLI · 空闲 · 当前项目 · 正在处理的任务'};

async function enterHistory({el, until, navigate}) {
  await until(() => !el('open-reply').hidden, '首页未自动打开唯一当前会话');
  await navigate(() => el('back').click(), '/#list');
  await until(() => !el('open-history').hidden, '返回首页后没有历史入口');
  await navigate(() => el('open-history').click(), '/archive');
  await until(() => el('history-rows').children.length === 4, '历史未按页加载');
}

async function runReader(scenario, {viewportWidth, entryPath, startPath = '/read/current/codex-home', prepare = enterHistory, pages = documents, paired = true} = {}) {
  await runReaderBrowser({setup, prepare, scenario, data: {rows, current, paired}, documents: pages, entryPath, startPath, viewportWidth,
    viewportHeight: viewportWidth ? 600 : 440});
}

test('首页、历史页与详情工具条使用各自的导航链接', {skip: !hasChrome}, async () => {
  const scenario = async ({el, reader, until, check, navigate}) => {
    check(reader.innerWidth === Number(reader.frameElement.width), '测试未使用指定视口');
    const masthead = () => reader.document.querySelector('.masthead');
    function link(container, label) {
      const target = [...container.querySelectorAll('a[href]')].find(a => a.innerText === label && a.getClientRects().length);
      check(target, '缺少可见的超链接：' + label);
      if (container === masthead()) check(reader.getComputedStyle(target).textDecorationLine === 'underline', '目录导航缺少下划线');
      return target;
    }
    function checkDetail(label) {
      const bar = reader.document.querySelector('footer');
      const back = link(bar, label);
      const rect = back.getBoundingClientRect();
      const bounds = bar.getBoundingClientRect();
      check(rect.width >= 44 && rect.height >= 44 && bounds.top >= el('view').getBoundingClientRect().bottom - 1, '返回链接未放入底部触控区');
      const center = r => r.left + r.width / 2;
      const prev = el('prev').getBoundingClientRect(), next = el('next').getBoundingClientRect();
      const reply = el('resume').hidden ? el('open-reply') : el('resume');
      const action = reply.getBoundingClientRect();
      const reference = reader.getComputedStyle(el('prev'));
      for (const control of [back, reply, el('next')]) {
        const style = reader.getComputedStyle(control);
        check(style.textDecorationLine === 'none', control.id + ' 不应显示下划线');
        for (const property of ['fontFamily', 'fontSize', 'fontWeight', 'lineHeight']) {
          check(style[property] === reference[property], control.id + ' 的 ' + property + ' 与上一屏不同');
        }
      }
      check(prev.left === bounds.left && next.right === bounds.right, '翻页按钮没有分居两端');
      check(Math.abs(center(prev) + center(next) - 2 * center(bounds)) < 1, '上一屏和下一屏不对称');
      check(action.width >= 44 && action.height >= 44 && Math.abs(center(rect) + center(action) - 2 * center(bounds)) < 1, '返回和回复操作不对称或触控区域过小');
      check(el('view').getBoundingClientRect().top === 0, '详情栏仍占用正文上方的空间');
      for (const item of bar.children) {
        if (!item.getClientRects().length) continue;
        const r = item.getBoundingClientRect();
        check(r.left >= bounds.left - 1 && r.right <= bounds.right + 1, '工具条超出窄屏');
        if (item.matches('button, a')) {
          check(item.contains(reader.document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2)), '工具条控件被遮挡：' + item.id);
        }
      }
      return back;
    }
    const home = link(masthead(), '返回首页');
    const homeBox = home.getBoundingClientRect();
    const divider = masthead().getBoundingClientRect().bottom;
    const brand = masthead().querySelector('.wordmark');
    check(reader.document.title === 'red-green · 历史会话' && brand.innerText.trim() === 'red-green', '历史页标题或品牌文案错误');
    check(homeBox.left >= brand.getBoundingClientRect().right, '返回首页未放在品牌右侧');
    const filter = el('project-filter');
    const filterTop = filter.getBoundingClientRect().top;
    const ruleTop = divider - parseFloat(reader.getComputedStyle(masthead()).borderBottomWidth);
    const upperGap = ruleTop - brand.getBoundingClientRect().bottom;
    check(filterTop - divider > 8 && Math.abs(filterTop - divider - upperGap) < 1, '虚线下方留白未增大或上下不一致');
    filter.value = '没有这个项目';
    filter.dispatchEvent(new Event('input'));
    await until(() => el('status').textContent.includes('没有匹配的项目'), '缺少筛选结果提示');
    check(filter.getBoundingClientRect().top === filterTop, '提示挤开了品牌栏下面的筛选框');
    filter.value = '';
    filter.dispatchEvent(new Event('input'));
    const browserEntries = reader.history.length;
    await navigate(() => el('history-rows').firstChild.click(), '/read/history/codex-0');
    await until(() => el('messages').textContent.includes('历史正文'), '未进入历史详情');
    check(reader.document.title === 'Codex CLI · 只读 · 项目甲 · 会话标题 0', '历史详情标题字段或顺序错误');
    await navigate(() => checkDetail('返回历史').click(), '/archive');
    check(reader.document.title === 'red-green · 历史会话', '返回历史时仍显示详情标题');
    check(reader.document.body.dataset.page === 'history', '没有返回历史列表');
    await navigate(() => link(masthead(), '返回首页').click(), '/#list');
    await until(() => !!el('list').firstElementChild, '没有回到首页');
    const historyBox = link(masthead(), '历史会话').getBoundingClientRect();
    check(reader.document.title === 'red-green · 当前会话', '首页浏览器标题错误');
    check(masthead().querySelector('.wordmark').innerText.trim() === 'red-green' && !/桌面伙伴|\d+\s*个会话/.test(reader.document.body.innerText), '首页仍显示多余标题或会话计数');
    check(historyBox.right === homeBox.right && historyBox.top === homeBox.top && masthead().getBoundingClientRect().bottom === divider, '两页的导航或虚线不在同一位置');
    check(el('list').firstElementChild.getBoundingClientRect().top === filterTop && filterTop > divider, '虚线下方留白不同');
    await navigate(() => el('list').firstElementChild.click(), '/read/current/codex-home');
    await until(() => !el('open-reply').hidden, '未进入可回复的当前会话');
    check(reader.document.title === 'Codex CLI · 空闲 · 当前项目 · 正在处理的任务', '当前详情标题字段错误或项目名重复');
    await navigate(() => checkDetail('返回列表').click(), '/#list');
    check(reader.document.body.dataset.page === 'list', '没有返回当前列表');
    check(reader.location.hash === '#list' && reader.history.length === browserEntries, '应用内导航增加了浏览器返回步骤');
    await navigate(() => reader.history.back(), '/test-note');
    check(reader.document.title === '测试笔记', '阅读器后退未回到笔记');
  };
  for (const viewportWidth of [320, 420, 620]) await runReader(scenario, {viewportWidth});
});

test('历史跨页查项目，正文只读可翻页，继续原会话后可返回原列表位置', {skip: !hasChrome}, async () => {
  await runReader(async ({el, mac, check, until, refresh, reader, navigate}) => {
    const query = el('project-filter');
    query.value = 'projectb';
    query.dispatchEvent(new Event('input'));
    await until(() => el('history-rows').children.length === 24, '未跨过没有匹配项的一页');
    const matches = [...el('history-rows').children].filter(row => !row.hidden);
    check(matches.length === 16 && matches[0].firstChild.textContent === '会话标题 8', '筛选结果或顺序错误');
    check(matches[0].lastChild.textContent.includes('ProjectB · 2023'), '缺少项目或本地时间');
    el('view').scrollTop = 190;
    const top = el('view').scrollTop;
    await navigate(() => el('history-rows').children[12].click(), '/read/history/codex-12');
    await until(() => !!reader.document.querySelector('#messages strong'), '缺少 Markdown 正文');
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
    await navigate(() => el('back').click(), '/archive');
    await until(() => reader.document.body.dataset.page === 'history' && el('project-filter').value === 'projectb' && el('view').scrollTop === top && top > 0,
      '返回后筛选或滚动位置丢失');
    await navigate(() => el('history-rows').children[12].click(), '/read/history/codex-12');
    await until(() => !el('open-reply').hidden, '从历史重进已激活会话时缺少回复入口');
    check(el('resume').hidden && el('composer').hidden && mac.writes.length === 2, '重进会话时重复恢复或自动弹出键盘');
    el('open-reply').click();
    check(el('reply-text').value === '继续测试' && el('send-reply').disabled, '待核对的草稿可被重复发送或已丢失');
  });
});

test('从历史查看已激活会话时沿用实时状态和设备回复权限', {skip: !hasChrome}, async () => {
  await runReader(async ({el, mac, check, until, refresh, reader, navigate}) => {
    mac.live = true;
    mac.granted = false;
    mac.state = 'running';
    await navigate(() => el('history-rows').firstChild.click(), '/read/history/codex-0');
    check(el('resume').hidden, '确认终端状态前闪现了继续对话');
    await until(() => reader.document.title === 'Codex CLI · 运行中 · 项目甲 · 会话标题 0', '只读设备未看到实时状态');
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
    check(reader.document.body.dataset.page === 'thread' && el('back').getAttribute('aria-disabled') === 'true', '发送中误触返回离开了详情');
    await until(() => mac.writes.length === 1, '点击发送无效');
    check(mac.writes[0].url === '/sessions/codex-0/messages' && mac.writes[0].payload.ticket === 'send-only', '没有向原会话发送');
    await until(() => el('back').getAttribute('aria-disabled') !== 'true', '发送完成后仍不能返回');
  });
});

test('打开历史时终端恰好退出，仍可阅读原对话并按需恢复', {skip: !hasChrome}, async () => {
  await runReader(async ({el, mac, check, until, navigate}) => {
    mac.live = true;
    mac.liveError = 410;
    await navigate(() => el('history-rows').firstChild.click(), '/read/history/codex-0');
    await until(() => el('messages').textContent.includes('历史正文'), '退出终端后丢失历史正文');
    check(mac.reads.includes('/sessions/codex-0/messages') && !el('resume').hidden && !el('resume').disabled
      && el('open-reply').hidden && mac.writes.length === 0, '终端退出后没有回到可恢复的历史状态');
  });
});

test('正在阅读的实时会话结束后提示结束，重进历史可继续对话', {skip: !hasChrome}, async () => {
  await runReader(async ({el, mac, check, until, refresh, reader, navigate}) => {
    mac.live = true;
    await navigate(() => el('history-rows').firstChild.click(), '/read/history/codex-0');
    await until(() => !el('open-reply').hidden, '未进入实时会话');
    mac.live = false;
    mac.liveError = 410;
    refresh();
    await until(() => el('status').textContent.includes('会话已结束'), '没有显示当前会话的结束状态');
    check(el('messages').textContent.includes('当前正文') && el('open-reply').hidden, '结束后正文或回复状态不正确');
    check(reader.document.title === 'Codex CLI · 已结束 · 项目甲 · 会话标题 0', '会话结束时浏览器标题未更新');
    await navigate(() => el('back').click(), '/archive');
    await until(() => !!el('history-rows').firstChild, '返回后未加载历史列表');
    await navigate(() => el('history-rows').firstChild.click(), '/read/history/codex-0');
    await until(() => !el('resume').hidden && !el('resume').disabled, '从历史重进后无法恢复');
    check(mac.writes.length === 0, '阅读已结束会话时启动了终端');
  });
});

test('已激活会话的实况暂不可用时保留正文，恢复后直接显示回复', {skip: !hasChrome}, async () => {
  await runReader(async ({el, mac, check, until, refresh, reader, navigate}) => {
    mac.live = true;
    mac.liveError = 503;
    mac.state = 'running';
    await navigate(() => el('history-rows').firstChild.click(), '/read/history/codex-0');
    await until(() => el('messages').textContent.includes('历史正文'), '实况暂不可用时丢失正文');
    check(el('resume').hidden && el('open-reply').hidden && el('status').textContent.includes('暂不可用'), '把实况中断当成终端退出');
    check(reader.document.title === 'Codex CLI · 运行中 · 项目甲 · 会话标题 0', '实况消息中断时丢失已绑定终端的状态');
    el('prev').click();
    await until(() => el('messages').textContent.includes('较早一页'), '断连后不能向前翻页');
    check(el('resume').hidden && el('status').textContent.includes('暂不可用'), '翻页后误显示继续对话或丢失中断提示');
    mac.state = 'awaiting';
    const position = el('view').scrollTop;
    refresh();
    await until(() => reader.document.title === 'Codex CLI · 待确认 · 项目甲 · 会话标题 0', '回看历史时标题没有更新状态');
    check(el('view').scrollTop === position && el('messages').textContent.includes('较早一页'), '标题更新时丢失回看位置');
    mac.liveError = 0;
    el('latest').click();
    refresh();
    await until(() => !el('open-reply').hidden, '实况恢复后缺少回复入口');
    check(el('composer').hidden && mac.writes.length === 0, '自动重试唤起了键盘或终端');
  });
});

test('离开历史正文后，迟到的实时内容不会盖住新打开的会话', {skip: !hasChrome}, async () => {
  await runReader(async ({el, mac, check, until, reader, navigate}) => {
    mac.live = mac.holdLive = true;
    await navigate(() => el('history-rows').children[0].click(), '/read/history/codex-0');
    await until(() => !!mac.releaseLive, '未请求实时正文');
    await navigate(() => el('back').click(), '/archive');
    mac.live = mac.holdLive = false;
    await until(() => !!el('history-rows').children[1], '返回后未加载历史列表');
    await navigate(() => el('history-rows').children[1].click(), '/read/history/codex-1');
    mac.releaseLive();
    await until(() => el('messages').textContent.includes('历史正文'), '迟到的请求影响了下一条会话');
    check(reader.document.title === 'Codex CLI · 只读 · 项目甲 · 会话标题 1' && !el('resume').hidden && el('open-reply').hidden, '新会话的状态不正确');
  });
});

test('实况请求期间向前翻页，保留正在回看的历史正文', {skip: !hasChrome}, async () => {
  await runReader(async ({el, mac, check, until, refresh, navigate}) => {
    await navigate(() => el('history-rows').firstChild.click(), '/read/history/codex-0');
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
  await runReader(async ({el, mac, check, until, refresh, sleep, reader, navigate, loadedTitles}) => {
    mac.granted = false;
    await navigate(() => el('history-rows').firstChild.click(), '/read/history/codex-0');
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
    await navigate(() => el('back').click(), '/archive');
    await sleep(500);
    check(reader.document.body.dataset.page === 'history' && el('composer').hidden && mac.writes.length === 3, '离开后迟到结果仍然打开输入框');
    mac.paired = false;
    refresh();
    await until(() => reader.location.pathname === '/pair' && el('pair') && !el('pair').hidden, '撤销配对后没有加载配对文档');
    check(loadedTitles.at(-1).title === 'red-green · 设备配对', '配对文档的原始标题错误');
    check(reader.document.title === 'red-green · 设备配对', '配对撤销后标题未清除');
    check(!el('history-rows').children.length && !el('messages').children.length, '配对撤销后缓存仍可读取');
    el('code').value = '123456';
    await navigate(() => el('pair').requestSubmit(), '/archive');
    check(loadedTitles.at(-1).title === 'red-green · 历史会话', '配对成功后未返回原目录文档');
  });
});

test('更多历史可重试，等待超时不会重启，历史不再可见会清除缓存', {skip: !hasChrome}, async () => {
  await runReader(async ({el, mac, check, until, refresh, sleep, reader, navigate}) => {
    mac.directoryError = true;
    el('history-more').click();
    await until(() => el('history-more').textContent === '重试', '未提示目录读取错误');
    check(el('history-rows').children.length === 4, '读取失败丢弃了已读目录');
    mac.directoryError = false;
    el('history-more').click();
    await until(() => el('history-rows').children.length === 8, '重试没有使用原游标');
    await navigate(() => el('history-rows').firstChild.click(), '/read/history/codex-0');
    await until(() => !el('resume').disabled, '历史页缺少恢复凭据');
    el('resume').click();
    await sleep(33000);
    check(!el('resume').disabled && el('status').textContent.includes('尚未连上终端'), '没有在有限等待后回到可重试状态');
    check(mac.writes.length === 1 && el('composer').hidden, '超时后重复启动或打开了输入框');
    mac.removed = true;
    refresh();
    await until(() => !el('open-history').hidden && reader.document.body.dataset.page === 'list', '没有退出失效的会话');
    check(!el('messages').children.length && !el('history-rows').children.length, '已失效的会话仍然可见');
  });
});

test('Mac 切换账号时历史列表和分页一起失效', {skip: !hasChrome}, async () => {
  await runReader(async ({el, mac, check, until, refresh, reader, navigate}) => {
    mac.generation = 'two';
    refresh();
    await until(() => reader.document.body.dataset.page === 'list', '空闲列表没有发现账号切换');
    check(!el('history-rows').children.length, '旧账号的标题仍然可见');
    await navigate(() => el('open-history').click(), '/archive');
    await until(() => el('history-rows').children.length === 4, '无法进入新账号目录');
    mac.generation = 'three';
    el('history-more').click();
    await until(() => reader.document.body.dataset.page === 'list', '下一页来自另一个账号时未清空目录');
    check(!el('history-rows').children.length, '列表混入了其他账号的会话');
  });
});

test('直接打开与刷新当前或历史详情保留 URL、初始标题、正文和返回入口', {skip: !hasChrome}, async () => {
  const scenario = async ({el, mac, reader, until, navigate, check, loadedTitles}) => {
    const path = reader.location.pathname;
    const history = path.includes('/history/');
    const title = history ? 'Codex CLI · 只读 · 项目甲 · 会话标题 1' : 'Codex CLI · 空闲 · 当前项目 · 正在处理的任务';
    const content = history ? '历史正文' : '当前正文';
    async function checkDetail() {
      await until(() => el('messages').textContent.includes(content), '直接访问或刷新后正文丢失');
      const arrival = loadedTitles.at(-1);
      check(arrival.path === path && arrival.title === title && reader.document.title === title, '加载时和轮询后的详情标题不同');
      check(el('back').textContent === (history ? '返回历史' : '返回列表'), '详情返回入口不符');
      check(!mac.reads.includes('/sessions') && !mac.reads.includes('/history'), '打开详情时遍历了目录');
    }
    await checkDetail();
    await navigate(() => reader.location.reload(), path);
    await checkDetail();
    check(loadedTitles.length === 2, '刷新时未重新获取详情文档');
    await navigate(() => el('back').click(), history ? '/archive' : '/#list');
    await navigate(() => reader.history.back(), '/test-note');
  };
  for (const entryPath of ['/read/current/codex-home', '/read/history/codex-1']) {
    await runReader(scenario, {entryPath, startPath: entryPath, prepare: null});
  }
});

test('没有新消息时浏览器可感知标题状态变化，草稿、焦点与返回路径保留', {skip: !hasChrome}, async () => {
  const entryPath = '/read/current/codex-home';
  await runReader(async ({el, reader, mac, until, refresh, check, navigate, sleep}) => {
    await until(() => !el('open-reply').hidden, '当前会话未就绪');
    const doc = reader.document;
    const entries = reader.history.length;
    const message = el('messages').firstElementChild;
    const titles = [];
    reader.addEventListener('hashchange', () => titles.push(doc.title));
    el('open-reply').click();
    const field = el('reply-text');
    field.value = '墨水屏上的草稿';
    field.setSelectionRange(2, 4);
    for (const [state, label] of [['running', '运行中'], ['awaiting', '待确认'], ['your_turn', '轮到你']]) {
      mac.state = state;
      refresh();
      await until(() => titles.at(-1) === 'Codex CLI · ' + label + ' · 当前项目 · 正在处理的任务', '浏览器没有收到' + label + '的标题导航');
      check(reader.document === doc && el('messages').firstElementChild === message, '状态变化重新加载了正文');
      check(doc.activeElement === field && field.value === '墨水屏上的草稿' && field.selectionStart === 2 && field.selectionEnd === 4, '状态变化打断输入');
    }
    const reads = mac.reads.length;
    refresh();
    await until(() => mac.reads.length > reads, '未完成重复轮询');
    await sleep(50);
    check(titles.length === 3 && reader.history.length === entries, '重复轮询触发导航或增加浏览器返回步骤');
    await navigate(() => reader.history.back(), '/test-note');
  }, {entryPath, startPath: entryPath, prepare: null});
});

test('当前会话卡片及打开的详情跟随改名、空名和与项目同名的情况', {skip: !hasChrome}, async () => {
  await runReader(async ({el, reader, mac, until, refresh, check, navigate}) => {
    const session = mac.sessions[0];
    const cardTitle = () => el('list').querySelector('.session-title')?.textContent;
    await until(() => cardTitle() === '正在处理的任务', '当前会话卡片缺少真实会话名');
    session.title = '整理周报';
    refresh();
    await until(() => cardTitle() === '整理周报', '仅会话名改变时卡片未更新');
    session.title = null;
    refresh();
    await until(() => cardTitle() === '未命名会话', '无名卡片缺少未命名提示');
    await navigate(() => el('list').firstElementChild.click(), '/read/current/codex-home');
    await until(() => reader.document.title === 'Codex CLI · 空闲 · 当前项目 · 未命名会话', '详情没有显示未命名会话');
    session.title = session.project;
    refresh();
    await until(() => reader.document.title === 'Codex CLI · 空闲 · 当前项目', '同名的项目和会话重复显示');
    session.title = '新的会话名';
    refresh();
    await until(() => reader.document.title === 'Codex CLI · 空闲 · 当前项目 · 新的会话名', '状态未变时详情没有更新会话名');
    check(el('messages').textContent.includes('当前正文'), '改名丢失了正文');
  }, {entryPath: '/#list', startPath: '/#list', prepare: null});
});

test('历史列表旧书签打开新地址，浏览器后退直接返回笔记', {skip: !hasChrome}, async () => {
  await runReader(async ({el, reader, navigate, until}) => {
    await until(() => el('history-rows').children.length === 4, '旧书签未打开历史目录');
    await navigate(() => reader.history.back(), '/test-note');
  }, {entryPath: '/#history-list', startPath: '/archive', prepare: null});
});


test('未配对或服务暂不可用时，恢复后重新加载带会话标题的详情文档', {skip: !hasChrome}, async () => {
  const path = '/read/current/codex-home';
  for (const error of [401, 503]) {
    const title = error === 401 ? 'red-green · 设备配对' : 'red-green · 会话';
    let first = true;
    const pages = {[path]: () => {
      if (!first) return documents[path];
      first = false;
      return {page: 'thread', error, title};
    }};
    await runReader(async ({el, mac, reader, until, refresh, check, navigate, loadedTitles}) => {
      const path = reader.location.pathname;
      const unauthenticated = reader.document.body.dataset.readerError === '401';
      check(loadedTitles[0].title === (unauthenticated ? 'red-green · 设备配对' : 'red-green · 会话'), '不可用时原始标题包含会话信息');
      check(!el('messages').children.length && el('open-reply').hidden, '详情未就绪时显示了正文或回复入口');
      if (unauthenticated) {
        await until(() => !el('pair').hidden, '打开未配对的详情没有提示配对');
        el('code').value = '123456';
        await navigate(() => el('pair').requestSubmit(), path);
        check(mac.writes.length === 1 && mac.writes[0].url === '/pair', '未向原设备配对');
      } else {
        await until(() => el('status').textContent.includes('暂不可用'), '详情缺少暂不可用的提示');
        check(mac.reads.includes(path), '没有重新请求详情元数据');
        mac.metadataReady = true;
        await navigate(refresh, path);
      }
      await until(() => el('messages').textContent.includes('当前正文'), '恢复后缺少详情正文');
      check(loadedTitles.at(-1).title === 'Codex CLI · 空闲 · 当前项目 · 正在处理的任务', '恢复后未从新文档获得详情标题');
    }, {entryPath: path, startPath: path, prepare: null, pages, paired: error !== 401});
  }
});

test('会话暂不可用后恢复时仍可核对原草稿', {skip: !hasChrome}, async () => {
  const path = '/read/current/codex-home';
  let visits = 0;
  const pages = {[path]: () => ++visits === 2
    ? {page: 'thread', error: 503, title: 'red-green · 会话'}
    : documents[path]};
  await runReader(async ({el, mac, reader, until, check, navigate, refresh}) => {
    const path = reader.location.pathname;
    await until(() => !el('open-reply').hidden, '未进入可回复的会话');
    el('open-reply').click();
    el('reply-text').value = '待核对的回复';
    el('reply-text').dispatchEvent(new Event('input'));
    mac.unknown = true;
    el('send-reply').click();
    await until(() => el('reply-info').textContent.includes('待核对'), '未保存发送结果');
    await navigate(() => el('back').click(), '/#list');
    await until(() => !!el('list').firstChild, '未回到列表');
    await navigate(() => el('list').firstChild.click(), path);
    await until(() => el('status').textContent.includes('暂不可用'), '未进入临时错误页');
    mac.metadataReady = true;
    await navigate(refresh, path);
    await until(() => !el('open-reply').hidden, '恢复后无法打开回复');
    el('open-reply').click();
    check(el('reply-text').value === '待核对的回复' && el('send-reply').disabled, '临时错误丢失草稿或使未知结果可重复发送');
  }, {entryPath: path, startPath: path, prepare: null, pages});
});
