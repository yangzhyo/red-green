import { ReaderState } from './reader-model.js';

const state = new ReaderState();
const el = id => document.getElementById(id);
const view = el('view');
const footer = document.querySelector('footer');
const brandFrame = window.SPRITES.spec('crab', 'completed').frames[0];
window.SPRITES.draw(el('brand-crab'), brandFrame, 4);
window.SPRITES.draw(el('back-crab'), brandFrame, 3);
const nodes = new Map();
const sessionRows = new Map();
let before = null;
let since = null;
let following = true;
let busy = false;
let queued = false;
let timer;
let wantEarlier = false;
let stopped = false;
let interrupted = false;
let homeNotice = '';
let currentEntry = null;
let replyTicket = null;
let historyList = null;
let readingHistory = false;
let resumeTicket = null;
let activation = null;
let resumeNotice = '';
const historyUnsupported = '请更新 Mac 上的 Codex CLI 后再查看历史';
const needsReplyPermission = '请在 Mac 上开启回复后再继续对话';
let sending = false;
let replyError = '';
const replyErrors = {
  '401':'请重新配对', '403':'请在 Mac 上开启回复', '409':'请等待当前回合结束',
  '410':'会话已结束', '412':'发送凭据已更新，请核对正文后再点发送',
  '422':'请输入 1 至 8192 个字', '503':'会话暂不可用，可稍后重试',
  '424':'Codex 未受理，可稍后重试',
  'unknown':'发送结果待核对，请先查看正文。修改文字后可再发送。',
};
const drafts = new Map();
const composer = el('composer');
const draft = el('reply-text');
const projectFilter = el('project-filter');

function updateReply() {
  const active = !stopped && !!replyTicket;
  el('resume').hidden = !readingHistory;
  el('resume').disabled = !resumeTicket || !!activation || stopped;
  el('open-reply').hidden = !active || !composer.hidden;
  const unavailable = !active || ['running', 'awaiting'].includes(currentEntry?.state);
  el('send-reply').disabled = sending || replyError === 'unknown' || unavailable || !draft.value.trim();
  draft.readOnly = sending;
  el('back').disabled = sending;
  el('cancel-reply').disabled = sending;
  if (!composer.hidden && !sending) {
    let hint = replyErrors[replyError] || '';
    if (replyError !== 'unknown') {
      if (!active && !hint) hint = '回复暂不可用';
      else if (active && currentEntry?.state === 'awaiting') hint = '请在 Mac 终端处理审批';
      else if (active && unavailable) hint = '请等待当前回合结束';
      else if (active && ['401','403','409','503'].includes(replyError)) hint = '上次未发送，可重试';
    }
    if (el('reply-info').textContent !== hint) el('reply-info').textContent = hint;
  }
}

const stateNames = new Map([
  ['running', '运行中'], ['awaiting', '待确认'], ['your_turn', '轮到你'],
  ['completed', '已完成'], ['aborted', '异常中止'], ['idle', '空闲'],
]);

function portrait(entry) {
  const canvas = document.createElement('canvas');
  const skin = window.SKINS.pick(entry.project);
  const pose = stateNames.has(entry.state) ? entry.state : 'idle';
  // Each view takes one frame so idle pages remain still on e-ink.
  const frame = window.SPRITES.spec(skin, pose).frames[0];
  window.SPRITES.draw(canvas, frame, 6);
  return canvas;
}

function status(text) {
  if (el('status').textContent !== text) el('status').textContent = text;
  if (el('status').hidden !== !text) el('status').hidden = !text;
}

function updateNavigation() {
  if (footer.hidden || view.clientHeight === 0) return;
  const end = view.scrollHeight - view.clientHeight;
  const atEnd = end - view.scrollTop < 2;
  const step = view.clientHeight * 0.9;
  const pages = 1 + Math.ceil(Math.max(0, end - 1) / step);
  const page = atEnd ? pages : Math.min(pages, 1 + Math.floor((view.scrollTop + 1) / step));
  const position = nodes.size ? (before ? '本段 ' : '') + page + ' / ' + pages : '';
  if (el('position').textContent !== position) el('position').textContent = position;
  const atStart = view.scrollTop <= 1 && (!before || stopped);
  if (el('prev').disabled !== atStart) el('prev').disabled = atStart;
  if (el('next').disabled !== atEnd) el('next').disabled = atEnd;
}

async function request(path, options) {
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), 8000);
  try {
    const response = await fetch(path, {...options, cache: 'no-store', signal: controller.signal});
    if (!response.ok) throw new Error(String(response.status));
    return await response.json();
  } finally { clearTimeout(timeout); }
}

function display(entry, {fromHistory = false, readOnly = fromHistory && !!entry} = {}) {
  if (currentEntry && draft.value) drafts.set(currentEntry.key, {text: draft.value, replyError});
  else if (currentEntry) drafts.delete(currentEntry.key);
  const savedDraft = entry && drafts.get(entry.key);
  el('messages').replaceChildren();
  el('list').replaceChildren();
  sessionRows.clear();
  nodes.clear();
  before = since = null;
  wantEarlier = false;
  following = true;
  stopped = false;
  interrupted = false;
  homeNotice = '';
  status('');
  el('latest').hidden = true;
  if (!fromHistory && historyList) {
    historyList = null;
    el('history-rows').replaceChildren();
    projectFilter.value = '';
  }
  const listingHistory = fromHistory && !entry;
  el('back').hidden = !entry && !listingHistory;
  el('back-label').textContent = listingHistory ? '首页' : fromHistory ? '历史' : '列表';
  el('back').setAttribute('aria-label', fromHistory && entry ? '返回历史列表' : '返回会话列表');
  el('open-history').hidden = true;
  projectFilter.hidden = el('history-list').hidden = !listingHistory;
  footer.hidden = !entry;
  readingHistory = readOnly;
  resumeTicket = activation = null;
  resumeNotice = '';
  currentEntry = entry;
  composer.hidden = true;
  replyTicket = null;
  sending = false;
  replyError = savedDraft?.replyError || '';
  el('open-reply').textContent = replyError === 'unknown' ? '核对回复' : '回复';
  draft.value = savedDraft?.text || '';
  el('reply-info').textContent = replyErrors[replyError] || '';
  updateReply();
  document.body.dataset.page = entry ? 'thread' : listingHistory ? 'history' : 'list';
  el('count').hidden = true;
  el('source').hidden = !entry;
  el('title').textContent = entry ? entry.title || entry.project : listingHistory ? '历史' : '桌面伙伴';
  el('title').title = entry ? el('title').textContent : '';
  el('state').textContent = readOnly ? '只读' : stateNames.get(entry?.state) || '';
  if (entry) {
    el('source').textContent = entry.source + ' / ' + (fromHistory ? entry.project : entry.key.slice(-8));
    state.open(entry.key);
    updateNavigation();
  } else state.back();
  if (listingHistory) renderHistoryList();
  view.scrollTop = listingHistory ? historyList.scroll : 0;
}

function sessionRow(entry) {
  const button = document.createElement('button');
  const avatar = document.createElement('span');
  avatar.className = 'portrait';
  avatar.setAttribute('aria-hidden', 'true');
  const name = document.createElement('strong');
  name.className = 'session-project';
  const detail = document.createElement('span');
  detail.className = 'session-detail';
  const shortId = document.createElement('span');
  shortId.className = 'session-id';
  shortId.textContent = entry.key.slice(-8);
  const identity = document.createElement('span');
  identity.className = 'identity';
  identity.append(name, detail, shortId);
  const badge = document.createElement('span');
  badge.className = 'session-state';
  button.append(badge, avatar, identity);
  const row = { button, name, detail, badge, avatar, entry: null };
  button.onclick = () => { display(row.entry); poll(); };
  sessionRows.set(entry.key, row);
  return row;
}

function showList(entries) {
  el('open-history').hidden = false;
  status(homeNotice || (entries.length ? '' : '等待可读会话'));
  const key = state.autoOpen(entries);
  if (key) {
    display(entries[0]);
    queued = true;
    return;
  }
  if (el('title').textContent !== '桌面伙伴') el('title').textContent = '桌面伙伴';
  const count = entries.length + ' 个会话';
  if (el('count').textContent !== count) el('count').textContent = count;
  if (el('count').hidden) el('count').hidden = false;
  const keys = new Set(entries.map(entry => entry.key));
  const list = el('list');
  for (const [key, row] of sessionRows) {
    if (keys.has(key)) continue;
    row.button.remove();
    sessionRows.delete(key);
  }
  entries.forEach((entry, i) => {
    const row = sessionRows.get(entry.key) || sessionRow(entry);
    const previous = row.entry;
    if (!previous || previous.project !== entry.project || previous.state !== entry.state) {
      row.avatar.replaceChildren(portrait(entry));
    }
    if (!previous || previous.project !== entry.project) row.name.textContent = entry.project;
    if (!previous || previous.source !== entry.source) row.detail.textContent = entry.source;
    if (!previous || previous.state !== entry.state) {
      row.badge.textContent = stateNames.get(entry.state) || '';
    }
    row.entry = entry;
    if (list.children[i] !== row.button) list.insertBefore(row.button, list.children[i] || null);
  });
}

const localDate = new Intl.DateTimeFormat('zh-CN', {
  year: 'numeric', month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit', hour12: false,
});

function addHistoryRow(entry) {
  if (historyList.rows.has(entry.key)) return;
  const button = document.createElement('button');
  const title = document.createElement('strong');
  title.textContent = entry.title || '未命名会话';
  const detail = document.createElement('small');
  detail.textContent = entry.project + ' · ' + localDate.format(new Date(entry.updatedAt * 1000));
  button.append(title, detail);
  button.onclick = () => {
    historyList.scroll = view.scrollTop;
    display(entry, {fromHistory: true});
    poll();
  };
  const project = entry.project.toLocaleLowerCase();
  button.hidden = !project.includes(historyList.filter);
  if (!button.hidden) historyList.matches++;
  historyList.rows.set(entry.key, {button, project});
  el('history-rows').append(button);
}

function renderHistoryList() {
  const {load, cursor, error, matches, filter} = historyList;
  el('history-more').hidden = !cursor && !error;
  el('history-more').disabled = load;
  el('history-more').textContent = error ? '重试' : '加载更多';
  status(error || (load ? '正在查找历史会话…' : matches ? '' : filter ? '没有匹配的项目' : '暂无历史会话'));
}

async function tickHistoryList(generation) {
  if (!historyList.load) {
    // A stationary list still needs to notice when the Mac revokes this device.
    await request('/sessions');
    if (generation === state.generation && interrupted) {
      interrupted = false;
      renderHistoryList();
    }
    return;
  }
  const cursor = historyList.cursor;
  const page = await request('/history' + (cursor ? '?cursor=' + encodeURIComponent(cursor) : ''));
  if (generation !== state.generation) return;
  if (page.nextCursor && historyList.cursors.has(page.nextCursor)) throw new Error('503');
  if (page.nextCursor) historyList.cursors.add(page.nextCursor);
  const previousCount = historyList.matches;
  page.entries.forEach(addHistoryRow);
  historyList.cursor = page.nextCursor;
  historyList.load = !!historyList.cursor && historyList.matches === previousCount;
  if (historyList.load) queued = true;
  historyList.error = '';
  interrupted = false;
  renderHistoryList();
}

const readingBlocks = 'p, li, tr, pre, h1, h2, h3, h4, h5, h6, hr';
function scrolledTables(body) {
  const positions = [];
  const tables = body.getElementsByTagName('table');
  for (let i = 0; i < tables.length; i++) {
    const table = tables[i];
    if (table.scrollLeft) positions.push({index: i, heading: table.tHead?.textContent, left: table.scrollLeft});
  }
  return positions;
}

function restoreColumns(body, positions) {
  const tables = body.getElementsByTagName('table');
  for (const {index, heading, left} of positions) {
    let table = tables[index];
    if (table?.tHead?.textContent !== heading) table = [...tables].find(t => t.tHead?.textContent === heading);
    if (table && table.scrollLeft !== left) table.scrollLeft = left;
  }
}

function anchor() {
  const top = view.getBoundingClientRect().top;
  for (const article of el('messages').children) {
    if (article.getBoundingClientRect().bottom <= top) continue;
    const blocks = article.lastElementChild.querySelectorAll(readingBlocks);
    let index = -1;
    for (let i = 0; i < blocks.length; i++) {
      const rect = blocks[i].getBoundingClientRect();
      if (rect.bottom <= top) continue;
      if (index !== -1 && rect.top > top) break;
      index = i;
      if (rect.top > top) break;
    }
    const node = blocks[index] || article;
    return {id: article.dataset.id, node, index, offset: node.getBoundingClientRect().top - top, tables: scrolledTables(article)};
  }
  return null;
}

function restore(saved) {
  if (!saved) return;
  const article = nodes.get(saved.id);
  if (!article) return;
  restoreColumns(article, saved.tables);
  let node = saved.node;
  if (!article.contains(node)) {
    const blocks = [...article.lastElementChild.querySelectorAll(readingBlocks)];
    const nearest = blocks[Math.min(saved.index, blocks.length - 1)];
    node = nearest?.isEqualNode(node) ? nearest : blocks.find(block => block.isEqualNode(node)) || nearest || article;
  }
  view.scrollTop += node.getBoundingClientRect().top - view.getBoundingClientRect().top - saved.offset;
}

function renderBody(body, message) {
  const columns = scrolledTables(body);
  const template = document.createElement('template');
  if (typeof message.html === 'string') {
    // The Mac escapes raw transcript HTML before serializing the rendered Markdown.
    template.innerHTML = message.html;
    for (const cell of template.content.querySelectorAll('th[style], td[style]')) {
      cell.dataset.align = cell.style.textAlign;
      cell.removeAttribute('style');
    }
  } else {
    const plain = document.createElement('p');
    plain.className = 'plain-text';
    plain.textContent = message.text;
    template.content.append(plain);
  }
  // Keep finished blocks attached while a streaming reply changes its last block.
  const blocks = [...template.content.children];
  blocks.forEach((block, i) => {
    const existing = body.children[i];
    if (!existing) body.append(block);
    else if (!existing.isEqualNode(block)) existing.replaceWith(block);
  });
  while (body.children.length > blocks.length) body.lastElementChild.remove();
  restoreColumns(body, columns);
}

function render(message) {
  let article = nodes.get(message.id);
  if (!article) {
    article = document.createElement('article');
    article.dataset.id = message.id;
    article.dataset.order = String(message.order);
    const next = [...el('messages').children].find(node => Number(node.dataset.order) > message.order);
    el('messages').insertBefore(article, next || null);
    nodes.set(message.id, article);
    const body = document.createElement('div');
    body.className = 'markdown';
    article.append(document.createElement('h2'), body);
  }
  const speaker = message.role === 'user' ? '你' : '助手';
  if (article.firstChild.textContent !== speaker) article.firstChild.textContent = speaker;
  article.dataset.role = message.role;
  renderBody(article.lastElementChild, message);
}

async function tick() {
  const generation = state.generation;
  if (!state.key) {
    if (historyList) return tickHistoryList(generation);
    const entries = await request('/sessions');
    if (generation === state.generation) showList(entries);
    return;
  }
  const earlier = wantEarlier && before;
  wantEarlier = false;
  const query = earlier ? '?before=' + encodeURIComponent(before) : since ? '?since=' + encodeURIComponent(since) : '';
  const key = encodeURIComponent(state.key);
  const endpoint = (readingHistory ? '/history/' : '/sessions/') + key + '/messages';
  const page = await request(endpoint + query);
  if (generation !== state.generation) return;
  if (readingHistory) {
    resumeTicket = page.resumeTicket || null;
    if (resumeTicket && resumeNotice === needsReplyPermission) resumeNotice = '';
  }
  await showMessages(page, earlier, endpoint, generation);
  if (generation !== state.generation || activation?.stage !== 'waiting') return;
  if (!resumeTicket) {
    finishActivation(needsReplyPermission);
    return;
  }
  if (!page.live) return;

  // Only the live endpoint can authorize sending. Start with its own latest page and cursors.
  const liveEndpoint = '/sessions/' + key + '/messages';
  let livePage;
  try {
    livePage = await request(liveEndpoint);
  } catch (error) {
    if (['404', '409', '410', '503'].includes(error.message)) return;
    throw error;
  }
  if (generation !== state.generation || !activation) return;
  if (!livePage.replyTicket) {
    resumeTicket = null;
    finishActivation(needsReplyPermission);
    return;
  }
  display(currentEntry, {fromHistory: true, readOnly: false});
  const liveGeneration = state.generation;
  await showMessages(livePage, false, liveEndpoint, liveGeneration);
  if (liveGeneration === state.generation) openComposer();
}

async function showMessages(page, earlier, endpoint, generation) {
  if (!readingHistory && 'state' in page && currentEntry.state !== page.state) {
    currentEntry.state = page.state;
    el('state').textContent = stateNames.get(page.state) || '';
  }
  replyTicket = readingHistory ? null : page.replyTicket || null;
  updateReply();
  const saved = !following && (page.reset || page.messages.length) ? anchor() : null;
  if (page.reset) {
    // Rebuild the visible history before replacing the DOM when the source resets its cursor.
    const cursors = new Set();
    while (saved && page.before && !page.messages.some(m => m.id === saved.id)) {
      if (cursors.has(page.before)) break;
      cursors.add(page.before);
      const older = await request(endpoint + '?before=' + encodeURIComponent(page.before));
      if (generation !== state.generation) return;
      page.before = older.before;
      page.messages = older.messages.concat(page.messages);
    }
    state.messages.clear();
    nodes.clear();
    el('messages').replaceChildren();
  }
  const changes = state.changes(generation, page.messages);
  if (!earlier || page.reset) since = page.since;
  if ('before' in page) before = page.before;
  if (activation) status(activation.notice);
  else if (readingHistory && resumeNotice) status(resumeNotice);
  else if (!state.messages.size) status(readingHistory ? '这段历史会话暂无正文' : '等待第一条消息');
  else status(readingHistory && !resumeTicket ? needsReplyPermission : '');
  if (changes.length) {
    changes.forEach(render);
    if (following) view.scrollTop = view.scrollHeight;
    else {
      restore(saved);
      if (!earlier) el('latest').hidden = false;
    }
  }
  if (earlier && saved) view.scrollTop -= view.clientHeight * 0.9;
  interrupted = false;
}

function requirePairing() {
  display(null);
  drafts.clear();
  state.manualList = false;
  el('pair').hidden = false;
  document.body.dataset.page = 'pair';
  el('title').textContent = '设备配对';
  status('');
}

function sourceChanged(notice = '会话来源已变更，请重新选择') {
  display(null);
  drafts.clear();
  homeNotice = notice;
  status(homeNotice);
}

async function poll() {
  clearTimeout(timer);
  if (document.hidden || !el('pair').hidden || stopped) return;
  if (busy) { queued = true; return; }
  busy = true;
  const generation = state.generation;
  try {
    if (activation?.stage === 'waiting' && Date.now() >= activation.deadline) {
      finishActivation('尚未连上终端，请在 Mac 上查看 Codex 后重试');
    }
    await tick();
  } catch (error) {
    if (generation === state.generation) {
      replyTicket = null;
      resumeTicket = null;
      updateReply();
      if (error.message === '401') {
        requirePairing();
      } else if (error.message === '501' && historyList) {
        sourceChanged(historyUnsupported);
        queued = true;
      } else if (error.message === '409' || (readingHistory && ['404', '410'].includes(error.message))) {
        sourceChanged();
        queued = true;
      } else if (historyList && !state.key && historyList.load) {
        historyList.load = false;
        historyList.error = '未能加载历史会话，请重试';
        renderHistoryList();
      } else if (error.message === '410') {
        stopped = true;
        el('state').textContent = '';
        status(interrupted ? '会话已结束，离线期间可能还有未同步的消息' : '会话已结束');
      } else if (error.message === '404' || error.message === '503') status('会话暂不可用');
      else { interrupted = true; status('正在重连 Mac…'); }
    }
  } finally {
    updateNavigation();
    busy = false;
    const delay = queued ? 0 : activation?.stage === 'waiting' ? 1000 : 3000;
    queued = false;
    if (!document.hidden && el('pair').hidden && !stopped) timer = setTimeout(poll, delay);
  }
}

function openComposer() {
  if (!replyTicket || readingHistory || stopped) return;
  composer.hidden = false;
  el('open-reply').textContent = '回复';
  updateReply();
  if (following) view.scrollTop = view.scrollHeight;
  draft.focus();
}
el('open-reply').onclick = openComposer;

function finishActivation(notice) {
  activation = null;
  resumeNotice = notice;
  status(notice);
  updateReply();
}

el('resume').onclick = async () => {
  if (!readingHistory || !resumeTicket || activation || stopped) return;
  const generation = state.generation;
  const ticket = resumeTicket;
  activation = {stage: 'requesting', notice: '正在连接 Mac 上的 Codex 终端…'};
  resumeNotice = '';
  status(activation.notice);
  updateReply();
  let notice = '正在等待 Mac 上的 Codex 终端…';
  try {
    await request('/history/' + encodeURIComponent(state.key) + '/resume', {
      method: 'POST', headers: {'Content-Type':'application/json'}, body: JSON.stringify({ticket}),
    });
  } catch (error) {
    if (generation !== state.generation) return;
    if (error.message === '401') { requirePairing(); return; }
    if (error.message === '501') { sourceChanged(historyUnsupported); poll(); return; }
    if (['404', '409', '410'].includes(error.message)) { sourceChanged(); poll(); return; }
    const reason = {
      '403': needsReplyPermission,
      '412': '会话凭据已更新，请重新点继续对话',
      '422': '项目目录已移动或不存在，请在 Mac 上检查',
      '424': 'Mac 未能打开 Codex 终端，请在 Mac 上检查后重试',
      '503': '会话暂不可用，请稍后重试',
    }[error.message];
    if (reason) {
      resumeTicket = null;
      finishActivation(reason);
      poll();
      return;
    }
    // The Mac may already have launched the terminal. Only poll after an uncertain POST.
    notice = '启动结果待核对，正在等待 Mac 上的终端…';
  }
  if (generation !== state.generation) return;
  activation = {stage: 'waiting', deadline: Date.now() + 30000, notice};
  status(notice);
  poll();
};

el('open-history').onclick = () => {
  historyList = {rows: new Map(), cursors: new Set(), cursor: null, scroll: 0, load: true, error: '', matches: 0, filter: ''};
  display(null, {fromHistory: true});
  poll();
};
el('history-more').onclick = () => {
  historyList.load = true;
  historyList.error = '';
  renderHistoryList();
  poll();
};
projectFilter.oninput = () => {
  view.scrollTop = 0;
  historyList.filter = projectFilter.value.trim().toLocaleLowerCase();
  historyList.matches = 0;
  for (const {button, project} of historyList.rows.values()) {
    const match = project.includes(historyList.filter);
    if (button.hidden === match) button.hidden = !match;
    if (match) historyList.matches++;
  }
  renderHistoryList();
  if (!historyList.matches && historyList.cursor && !historyList.load && !historyList.error) el('history-more').click();
};
el('cancel-reply').onclick = () => {
  composer.hidden = true;
  updateReply();
  el('open-reply').focus();
};
draft.oninput = () => { replyError = ''; updateReply(); };
composer.onsubmit = async event => {
  event.preventDefault();
  if (el('send-reply').disabled) return;
  const generation = state.generation;
  const ticket = replyTicket;
  replyError = '';
  sending = true;
  el('reply-info').textContent = '正在发送…';
  updateReply();
  try {
    await request('/sessions/' + encodeURIComponent(state.key) + '/messages', {
      method: 'POST', headers: {'Content-Type':'application/json'},
      body: JSON.stringify({ticket, text:draft.value}),
    });
    if (state.generation !== generation) return;
    draft.value = '';
    draft.blur();
    composer.hidden = true;
    el('open-reply').textContent = '已发送 · 回复';
  } catch (error) {
    if (state.generation !== generation) return;
    replyError = Object.prototype.hasOwnProperty.call(replyErrors, error.message) ? error.message : 'unknown';
  } finally {
    if (state.generation === generation) {
      sending = false;
      replyTicket = null;
      updateReply();
      poll();
    }
  }
};
el('pair').onsubmit = async event => {
  event.preventDefault();
  try {
    await request('/pair', {method:'POST', headers:{'Content-Type':'application/json'}, body:JSON.stringify({code:el('code').value})});
    el('pair').hidden = true;
    document.body.dataset.page = 'list';
    el('title').textContent = '桌面伙伴';
    el('code').value = '';
    await poll();
  } catch (error) {
    status(error.message === '403' ? '配对码无效或已过期，请查看 Mac 上的配对码。' : '连接失败，请重试。');
  }
};
el('back').onclick = () => { display(null, {fromHistory: !!currentEntry && !!historyList}); poll(); };
el('prev').onclick = () => {
  following = false;
  if (view.scrollTop <= 1 && before) { wantEarlier = true; poll(); }
  else view.scrollTop -= view.clientHeight * 0.9;
};
el('next').onclick = () => { view.scrollTop += view.clientHeight * 0.9; };
el('latest').onclick = () => { following = true; el('latest').hidden = true; view.scrollTop = view.scrollHeight; };
view.onscroll = () => {
  following = view.scrollHeight - view.scrollTop - view.clientHeight < 3;
  if (following) el('latest').hidden = true;
  updateNavigation();
};
window.addEventListener('resize', () => {
  if (!composer.hidden && following) view.scrollTop = view.scrollHeight;
  updateNavigation();
});
document.addEventListener('visibilitychange', () => {
  if (document.hidden && state.key) interrupted = true;
  poll();
});
window.addEventListener('offline', () => {
  interrupted = true;
  replyTicket = resumeTicket = null;
  updateReply();
  if (el('pair').hidden && !stopped) status('正在重连 Mac…');
});
window.addEventListener('online', poll);
window.addEventListener('pageshow', poll);
poll();
