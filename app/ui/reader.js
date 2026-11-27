import { ReaderState } from './reader-model.js';

const state = new ReaderState();
const el = id => document.getElementById(id);
const view = el('view');
const footer = document.querySelector('footer');
const fromHistoryThread = location.pathname.startsWith('/read/history/');
const threadDocument = fromHistoryThread || location.pathname.startsWith('/read/current/');
const pairingDocument = location.pathname === '/pair';
const brandFrame = window.SPRITES.spec('crab', 'completed').frames[0];
window.SPRITES.draw(el('brand-crab'), brandFrame, 4);
const nodes = new Map();
const sessionRows = new Map();
let before = null;
let since = null;
let following = true;
let busy = false;
let queued = false;
let timer;
let wantEarlier = null;
let turns = [];
let turnIndex = -1;
let stopped = false;
let interrupted = false;
const homeNotice = takeTabValue('notice') || '';
let currentEntry = null;
let replyTicket = null;
let historyList = null;
let readingHistory = false;
let resumeTicket = null;
let activation = null;
let resumeNotice = '';
const historyUnsupported = '请更新 Mac 上的 Codex CLI 后再查看历史';
const needsReplyPermission = '请在 Mac 上开启回复后再继续对话';
const liveUnavailableNotice = '实况暂不可用，可继续回看';
let sending = false;
let replyError = '';
const replyErrors = {
  '401':'请重新配对', '403':'请在 Mac 上开启回复', '409':'请等待当前回合结束',
  '410':'会话已结束', '412':'发送凭据已更新，请核对正文后再点发送',
  '422':'请输入 1 至 8192 个字', '503':'会话暂不可用，可稍后重试',
  '424':'Codex 未受理，可稍后重试',
  'unknown':'发送结果待核对，请先查看正文。修改文字后可再发送。',
};
const drafts = new Map(takeTabValue('drafts') || []);
const composer = el('composer');
const draft = el('reply-text');
const projectFilter = el('project-filter');

// Transfer UI state only within this tab. Each destination consumes its handoff.
function takeTabValue(key) {
  try {
    const value = sessionStorage.getItem('reader.' + key);
    sessionStorage.removeItem('reader.' + key);
    return JSON.parse(value);
  } catch { return null; }
}

function putTabValue(key, value) {
  try { sessionStorage.setItem('reader.' + key, JSON.stringify(value)); } catch { /* Storage is optional. */ }
}

function rememberDraft() {
  if (!currentEntry) return;
  if (draft.value) drafts.set(currentEntry.key, {text: draft.value, replyError});
  else drafts.delete(currentEntry.key);
}

function navigate(url) {
  rememberDraft();
  if (drafts.size) putTabValue('drafts', [...drafts]);
  // Replace the current document so the browser's Back button still returns to the note.
  state.back();
  stopped = true;
  clearTimeout(timer);
  if (location.hash && new URL(url, location.href).href === location.href) location.reload();
  else location.replace(url);
}

function updateReply() {
  const active = !stopped && !!replyTicket;
  el('resume').disabled = !resumeTicket || !!activation || stopped;
  el('open-reply').hidden = !active || !composer.hidden;
  const unavailable = !active || ['running', 'awaiting'].includes(currentEntry?.state);
  el('send-reply').disabled = sending || replyError === 'unknown' || unavailable || !draft.value.trim();
  draft.readOnly = sending;
  el('back').setAttribute('aria-disabled', String(sending));
  el('cancel-reply').disabled = sending;
  if (!composer.hidden && !sending) {
    let hint = replyErrors[replyError] || '';
    if (replyError !== 'unknown') {
      if (!active && !hint) hint = '回复暂不可用';
      else if (active && currentEntry?.state === 'awaiting') hint = '请在 Mac 终端处理提示';
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

function setPageTitle(...parts) {
  const next = parts.filter(Boolean).join(' · ').replace(/\s+/g, ' ').trim();
  if (document.title === next) return false;
  document.title = next;
  return true;
}

function updateThreadTitle(stateLabel) {
  const {title, project, source} = currentEntry;
  const name = title?.replace(/\s+/g, ' ').trim() || '未命名会话';
  const projectName = project?.replace(/\s+/g, ' ').trim();
  if (setPageTitle(source, stateLabel, projectName, name === projectName ? '' : name)) {
    // WebView hosts can sample the title on navigation. Neither fragment has a scroll target.
    location.replace(location.hash === '#status-1' ? '#status-2' : '#status-1');
  }
}

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
  const position = turns.length ? (before ? '本段 ' : '') + (turnIndex + 1) + ' / ' + turns.length : '';
  if (el('position').textContent !== position) el('position').textContent = position;
  const atStart = turnIndex <= 0 && (!before || stopped);
  const atEnd = turnIndex + 1 >= turns.length;
  if (el('prev').disabled !== atStart) el('prev').disabled = atStart;
  if (el('next').disabled !== atEnd) el('next').disabled = atEnd;
}

function atLatestEnd() {
  return turnIndex === turns.length - 1 && view.scrollHeight - view.scrollTop - view.clientHeight < 3;
}

function showTurn(index, changes = []) {
  const turn = turns[index] || [];
  const visible = new Set(turn.map(message => message.id));
  for (const [id, article] of nodes) {
    if (!visible.has(id)) {
      article.remove();
      nodes.delete(id);
    }
  }
  const changed = new Set(changes.map(message => message.id));
  for (const message of turn) {
    if (!nodes.has(message.id) || changed.has(message.id)) render(message);
  }
  turnIndex = index;
  updateNavigation();
}

function goToTurn(index) {
  if (index < 0 || index >= turns.length) return;
  wantEarlier = null;
  showTurn(index);
  view.scrollTop = 0;
  following = atLatestEnd();
  if (following) el('latest').hidden = true;
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
  rememberDraft();
  const savedDraft = entry && drafts.get(entry.key);
  el('messages').replaceChildren();
  el('list').replaceChildren();
  sessionRows.clear();
  nodes.clear();
  before = since = null;
  wantEarlier = null;
  turns = [];
  turnIndex = -1;
  following = true;
  stopped = false;
  interrupted = false;
  status('');
  el('latest').hidden = true;
  if (!fromHistory && historyList) {
    historyList = null;
    el('history-rows').replaceChildren();
    projectFilter.value = '';
  }
  const listingHistory = fromHistory && !entry;
  el('history-home').hidden = !listingHistory;
  el('back').textContent = fromHistory ? '返回历史' : '返回列表';
  el('back').setAttribute('href', fromHistory ? '/archive' : '/#list');
  el('open-history').hidden = true;
  projectFilter.hidden = el('history-list').hidden = !listingHistory;
  footer.hidden = !entry;
  readingHistory = readOnly;
  resumeTicket = activation = null;
  resumeNotice = '';
  currentEntry = entry;
  composer.hidden = true;
  el('resume').hidden = true;
  replyTicket = null;
  sending = false;
  replyError = savedDraft?.replyError || '';
  el('open-reply').textContent = replyError === 'unknown' ? '核对回复' : '回复';
  draft.value = savedDraft?.text || '';
  el('reply-info').textContent = replyErrors[replyError] || '';
  updateReply();
  document.body.dataset.page = entry ? 'thread' : listingHistory ? 'history' : 'list';
  if (entry) {
    state.open(entry.key);
    updateNavigation();
  } else {
    setPageTitle('red-green', listingHistory ? '历史会话' : '当前会话');
    state.back();
  }
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
  const title = document.createElement('span');
  title.className = 'session-title';
  const identity = document.createElement('span');
  identity.className = 'identity';
  identity.append(name, detail, title);
  const badge = document.createElement('span');
  badge.className = 'session-state';
  button.append(badge, avatar, identity);
  const row = { button, name, detail, title, badge, avatar, entry: null };
  button.onclick = () => navigate('/read/current/' + encodeURIComponent(entry.key));
  sessionRows.set(entry.key, row);
  return row;
}

function showList(entries) {
  el('open-history').hidden = false;
  status(homeNotice || (entries.length ? '' : '等待可读会话'));
  const key = state.autoOpen(entries);
  if (key) {
    navigate('/read/current/' + encodeURIComponent(key));
    return;
  }
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
    if (!previous || previous.title !== entry.title) row.title.textContent = entry.title || '未命名会话';
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
    putTabValue('history', {filter: projectFilter.value, scroll: view.scrollTop,
      count: historyList.rows.size, generation: historyList.generation});
    navigate('/read/history/' + encodeURIComponent(entry.key));
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
    // Check the account and device grant without reloading rows or moving the page.
    const token = historyList.generation;
    await request(token ? '/history/status?generation=' + encodeURIComponent(token) : '/sessions');
    if (generation === state.generation && interrupted) {
      interrupted = false;
      renderHistoryList();
    }
    return;
  }
  const cursor = historyList.cursor;
  const page = await request('/history' + (cursor ? '?cursor=' + encodeURIComponent(cursor) : ''));
  if (generation !== state.generation) return;
  if (!page.generation || (historyList.generation && page.generation !== historyList.generation)) {
    throw new Error('409');
  }
  historyList.generation = page.generation;
  if (page.nextCursor && historyList.cursors.has(page.nextCursor)) throw new Error('503');
  if (page.nextCursor) historyList.cursors.add(page.nextCursor);
  const previousCount = historyList.matches;
  page.entries.forEach(addHistoryRow);
  historyList.cursor = page.nextCursor;
  const restoring = historyList.restoreCount > historyList.rows.size;
  historyList.load = !!historyList.cursor && (restoring || historyList.matches === previousCount);
  if (historyList.load) queued = true;
  historyList.error = '';
  interrupted = false;
  renderHistoryList();
  if (!historyList.load && historyList.restoreCount) {
    view.scrollTop = historyList.scroll;
    historyList.restoreCount = 0;
  }
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

function canSwitchToLive() {
  // Explicit activation can leave an older page. Browsing follows live only at the end.
  return activation ? activation.stage === 'waiting' : following;
}

async function tick() {
  const generation = state.generation;
  if (!state.key) {
    if (threadDocument) {
      // An initially unavailable document regains its native title on a fresh document load.
      await request(location.pathname, {headers: {Accept: 'application/json'}});
      if (generation === state.generation) {
        if (document.body.dataset.readerError === '401') drafts.clear();
        navigate(location.pathname);
      }
      return;
    }
    if (historyList) return tickHistoryList(generation);
    const entries = await request('/sessions');
    if (generation === state.generation) showList(entries);
    return;
  }
  const earlier = before && wantEarlier;
  wantEarlier = null;
  const query = earlier ? '?before=' + encodeURIComponent(before) : since ? '?since=' + encodeURIComponent(since) : '';
  const key = encodeURIComponent(state.key);
  const endpoint = (readingHistory ? '/history/' : '/sessions/') + key + '/messages';
  const page = await request(endpoint + query);
  if (generation !== state.generation) return;
  if (readingHistory) {
    if ('title' in page) currentEntry.title = page.title;
    let bound = page.live;
    resumeTicket = page.resumeTicket || null;
    if (resumeTicket && resumeNotice === needsReplyPermission) resumeNotice = '';
    if (activation?.stage === 'waiting' && !resumeTicket) finishActivation(needsReplyPermission);
    if (bound && canSwitchToLive()) {
      // The live reader owns its cursors, terminal state and reply permission.
      const liveEndpoint = '/sessions/' + key + '/messages';
      let livePage;
      try {
        livePage = await request(liveEndpoint);
      } catch (error) {
        // A terminal can close between the history lookup and the live read.
        if (generation !== state.generation) return;
        if (error.message === '503') resumeNotice = liveUnavailableNotice;
        else if (['404', '410'].includes(error.message)) bound = false;
        else throw error;
      }
      if (generation !== state.generation) return;
      if (livePage && canSwitchToLive()) {
        const continued = activation?.stage === 'waiting';
        display(currentEntry, {fromHistory: true, readOnly: false});
        const liveGeneration = state.generation;
        await showMessages(livePage, false, liveEndpoint, liveGeneration);
        if (continued && liveGeneration === state.generation) openComposer();
        return;
      }
    }
    if (!bound && resumeNotice === liveUnavailableNotice) resumeNotice = '';
    currentEntry.state = bound ? page.state : null;
    updateThreadTitle(stateNames.get(currentEntry.state) || '只读');
    el('resume').hidden = bound;
  }
  await showMessages(page, earlier, endpoint, generation);
}

async function showMessages(page, earlier, endpoint, generation) {
  if (!readingHistory && ('state' in page || 'title' in page)) {
    if ('state' in page) currentEntry.state = page.state;
    if ('title' in page) currentEntry.title = page.title;
    updateThreadTitle(stateNames.get(currentEntry.state));
  }
  replyTicket = readingHistory ? null : page.replyTicket || null;
  if (!replyTicket && !composer.hidden) {
    draft.blur();
    composer.hidden = true;
  }
  updateReply();
  let saved = !following && (page.reset || page.messages.length) ? anchor() : null;
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
      saved = following ? null : anchor();
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
  if (changes.length || page.reset) {
    const oldId = turns[turnIndex]?.[0].id;
    const wasFollowing = following;
    turns = state.turns();
    const anchorId = saved?.id ?? oldId;
    const previous = turns.findIndex(turn => turn.some(message => message.id === anchorId));
    const tail = turns.length - 1;
    let index = wasFollowing || previous < 0 ? tail : previous;
    if (!wasFollowing && earlier && earlier === oldId && previous > 0) index = previous - 1;
    showTurn(index, changes);
    if (oldId !== turns[index]?.[0].id) view.scrollTop = 0;
    else if (wasFollowing) view.scrollTop = view.scrollHeight;
    else restore(saved);
    following = atLatestEnd();
    if (following) el('latest').hidden = true;
    else if (!earlier && oldId != null && !wasFollowing) el('latest').hidden = false;
  }
  interrupted = false;
}

function requirePairing() {
  display(null);
  drafts.clear();
  takeTabValue('history');
  delete document.body.dataset.entry;
  state.manualList = false;
  if (!threadDocument && !pairingDocument) {
    const destination = location.pathname === '/archive' ? '#archive' : location.hash;
    return navigate('/pair' + destination);
  }
  el('pair').hidden = false;
  document.body.dataset.page = 'pair';
  setPageTitle('red-green', '设备配对');
  status('');
  if (threadDocument && document.body.dataset.readerError !== '401') location.reload();
}

function sourceChanged(notice = '会话来源已变更，请重新选择') {
  display(null);
  drafts.clear();
  delete document.body.dataset.entry;
  takeTabValue('history');
  putTabValue('notice', notice);
  navigate('/#list');
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
      } else if (error.message === '501' && (historyList || fromHistoryThread)) {
        sourceChanged(historyUnsupported);
        queued = true;
      } else if (error.message === '409' || ((readingHistory || (threadDocument && !state.key)) && ['404', '410'].includes(error.message))) {
        sourceChanged();
        queued = true;
      } else if (historyList && !state.key && historyList.load) {
        historyList.load = false;
        historyList.error = '未能加载历史会话，请重试';
        renderHistoryList();
      } else if (error.message === '410') {
        stopped = true;
        if (currentEntry) updateThreadTitle('已结束');
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
      '424': '请在 Mac 检查终端和自动化权限后重试',
      '428': '请在 Mac 检查新终端。若已打开，请完成终端提示；若没有，请关闭并重新开启局域网会话共享，再允许设备回复。',
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

for (const link of document.querySelectorAll('.nav-link')) {
  link.onclick = event => {
    if (sending) { event.preventDefault(); return; }
    if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
    event.preventDefault();
    if (link.id === 'open-history' || link.id === 'history-home') takeTabValue('history');
    navigate(link.href);
  };
}

function startReader() {
  if (pairingDocument) return requirePairing();
  if (threadDocument) {
    const {entry, readerError} = document.body.dataset;
    delete document.body.dataset.entry;
    if (entry) {
      display(JSON.parse(entry), {fromHistory: fromHistoryThread});
    } else {
      el('back').href = fromHistoryThread ? '/archive' : '/#list';
      el('back').textContent = fromHistoryThread ? '返回历史' : '返回列表';
      footer.hidden = false;
      // A link from a note can omit a Strict cookie on its first navigation.
      status(readerError === '401' ? '正在核对配对状态…' : '正在重新打开会话…');
    }
  } else if (location.pathname === '/archive') {
    const saved = takeTabValue('history');
    projectFilter.value = saved?.filter || '';
    historyList = {generation: saved?.generation || null, rows: new Map(), cursors: new Set(), cursor: null,
      scroll: saved?.scroll || 0, restoreCount: saved?.count || 0, load: true, error: '', matches: 0,
      filter: projectFilter.value.trim().toLocaleLowerCase()};
    display(null, {fromHistory: true});
  } else if (location.hash === '#history-list') {
    return navigate('/archive');
  } else if (location.hash === '#list') {
    display(null);
  }
  return poll();
}

el('history-more').onclick = () => {
  historyList.load = true;
  historyList.error = '';
  renderHistoryList();
  poll();
};
projectFilter.oninput = () => {
  view.scrollTop = 0;
  historyList.restoreCount = 0;
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
    setPageTitle('red-green', '当前会话');
    el('code').value = '';
    if (pairingDocument) navigate(location.hash === '#archive' ? '/archive' : '/' + location.hash);
    else location.reload();
  } catch (error) {
    status(error.message === '403' ? '配对码无效或已过期，请查看 Mac 上的配对码。' : '连接失败，请重试。');
  }
};
el('prev').onclick = () => {
  if (turnIndex > 0) goToTurn(turnIndex - 1);
  else if (before && !stopped) {
    following = false;
    wantEarlier = turns[turnIndex]?.[0].id;
    poll();
  }
};
el('next').onclick = () => goToTurn(turnIndex + 1);
el('latest').onclick = () => {
  goToTurn(turns.length - 1);
  view.scrollTop = view.scrollHeight;
  following = true;
  el('latest').hidden = true;
};
view.onscroll = () => {
  if (!currentEntry) return;
  following = atLatestEnd();
  if (following) el('latest').hidden = true;
};

if (threadDocument) {
  window.enableReadingGestures(view, x => {
    const bounds = view.getBoundingClientRect();
    const line = parseFloat(getComputedStyle(view.querySelector('.markdown') || view).lineHeight);
    const distance = Math.max(1, view.clientHeight - line);
    view.scrollTop += x < bounds.left + bounds.width / 2 ? -distance : distance;
  }, next => el(next ? 'next' : 'prev').click());
}
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
startReader();
