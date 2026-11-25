import { ReaderState } from './reader-model.js';

const state = new ReaderState();
const el = id => document.getElementById(id);
const view = el('view');
const footer = document.querySelector('footer');
const nodes = new Map();
let before = null;
let since = null;
let following = true;
let directory = '';
let busy = false;
let queued = false;
let timer;
let wantEarlier = false;
let stopped = false;
let interrupted = false;

function status(text) {
  if (el('status').textContent !== text) el('status').textContent = text;
  if (el('status').hidden !== !text) el('status').hidden = !text;
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

function display(entry) {
  el('messages').replaceChildren();
  el('list').replaceChildren();
  directory = '';
  nodes.clear();
  before = since = null;
  wantEarlier = false;
  following = true;
  stopped = false;
  interrupted = false;
  status('');
  el('latest').hidden = true;
  el('back').hidden = footer.hidden = !entry;
  el('title').textContent = entry ? entry.project + ' · ' + entry.source : '会话列表';
  if (entry) state.open(entry.key);
  else state.back();
}

function showList(entries) {
  status(entries.length ? '' : '等待可读会话');
  const key = state.autoOpen(entries);
  if (key) {
    display(entries[0]);
    queued = true;
    return;
  }
  const signature = JSON.stringify(entries);
  if (directory === signature) return;
  directory = signature;
  const buttons = entries.map(entry => {
    const button = document.createElement('button');
    button.textContent = entry.project + ' · ' + entry.source + ' · ' + entry.key.slice(-8);
    button.onclick = () => { display(entry); poll(); };
    return button;
  });
  el('list').replaceChildren(...buttons);
}

function anchor() {
  const top = view.getBoundingClientRect().top;
  for (const article of el('messages').children) {
    if (article.getBoundingClientRect().bottom <= top) continue;
    const line = [...article.children].find(node => node.getBoundingClientRect().bottom > top);
    if (line) return {id: article.dataset.id, index: [...article.children].indexOf(line), offset: line.getBoundingClientRect().top - top};
  }
  return null;
}

function restore(saved) {
  if (!saved) return;
  const article = nodes.get(saved.id);
  if (!article) return;
  const line = article.children[Math.min(saved.index, article.children.length - 1)];
  view.scrollTop += line.getBoundingClientRect().top - view.getBoundingClientRect().top - saved.offset;
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
    article.append(document.createElement('h2'));
  }
  article.firstChild.textContent = message.role === 'user' ? '你' : '助手';
  const lines = message.text.split('\n');
  for (let i = 0; i < lines.length; i++) {
    let line = article.children[i + 1];
    if (!line) { line = document.createElement('p'); article.append(line); }
    if (line.textContent !== lines[i]) line.textContent = lines[i];
  }
  while (article.children.length > lines.length + 1) article.lastChild.remove();
}

async function tick() {
  const generation = state.generation;
  if (!state.key) {
    const entries = await request('/sessions');
    if (generation === state.generation) showList(entries);
    return;
  }
  const earlier = wantEarlier && before;
  wantEarlier = false;
  const query = earlier ? '?before=' + before : since ? '?since=' + since : '';
  const page = await request('/sessions/' + encodeURIComponent(state.key) + '/messages' + query);
  if (generation !== state.generation) return;
  const saved = following ? null : anchor();
  if (page.reset) {
    // Rebuild the visible history before replacing the DOM when the source resets its cursor.
    const cursors = new Set();
    while (saved && page.before && !page.messages.some(m => m.id === saved.id)) {
      if (cursors.has(page.before)) break;
      cursors.add(page.before);
      const older = await request('/sessions/' + encodeURIComponent(state.key) + '/messages?before=' + page.before);
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
  status(state.messages.size ? '' : '等待第一条消息');
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

async function poll() {
  clearTimeout(timer);
  if (document.hidden || !el('pair').hidden || stopped) return;
  if (busy) { queued = true; return; }
  busy = true;
  const generation = state.generation;
  try {
    await tick();
  } catch (error) {
    if (generation === state.generation) {
      if (error.message === '401') {
        display(null);
        state.manualList = false;
        el('pair').hidden = false;
        status('');
      } else if (error.message === '409') {
        display(null);
        status('会话来源已变更，请重新选择');
        queued = true;
      } else if (error.message === '410') {
        stopped = true;
        status(interrupted ? '会话已结束，离线期间可能还有未同步的消息' : '会话已结束');
      } else if (error.message === '404' || error.message === '503') status('会话暂不可用');
      else { interrupted = true; status('正在重连 Mac…'); }
    }
  } finally {
    busy = false;
    const delay = queued ? 0 : 3000;
    queued = false;
    if (!document.hidden && el('pair').hidden && !stopped) timer = setTimeout(poll, delay);
  }
}

el('pair').onsubmit = async event => {
  event.preventDefault();
  try {
    await request('/pair', {method:'POST', headers:{'Content-Type':'application/json'}, body:JSON.stringify({code:el('code').value})});
    el('pair').hidden = true;
    el('code').value = '';
    await poll();
  } catch (error) {
    status(error.message === '403' ? '配对码无效或已过期，请查看 Mac 上的配对码。' : '连接失败，请重试。');
  }
};
el('back').onclick = () => { display(null); poll(); };
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
};
document.addEventListener('visibilitychange', () => {
  if (document.hidden && state.key) interrupted = true;
  poll();
});
window.addEventListener('offline', () => { interrupted = true; });
window.addEventListener('online', poll);
window.addEventListener('pageshow', poll);
poll();
