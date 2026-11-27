import test from 'node:test';
import { hasChrome, runReaderBrowser } from './reader-browser.js';

test('Nomad 从最新轮开头读，点按只滚本轮，横滑换轮，回看时保留位置', {skip: !hasChrome}, async () => {
  const entryPath = '/read/current/sample';
  const entry = {key: 'sample', source: 'Codex CLI', project: '示例', title: '按轮阅读'};
  await runReaderBrowser({
    entryPath, documents: {[entryPath]: {entry, page: 'thread', title: '按轮阅读'}},
    setup: () => {
      const row = (id, role, html, startsTurn) => ({id: String(id), order: id, role, text: html, html, startsTurn});
      const rows = [row(1, 'user', '<p>第一轮</p>', true), row(2, 'user', '<p>第二轮</p>', true),
        row(3, 'user', '<p>补充</p>', false), row(4, 'assistant',
          '<p><a href="#note">链接</a></p><table><tr>' + '<td>列</td>'.repeat(80) + '</tr></table>'
          + '<p>长段落</p>'.repeat(80), false)];
      const mac = {next: []};
      mac.fetch = async url => ({ok: true, json: async () => ({
        since: 'rev', messages: url.includes('since=') ? mac.next.splice(0) : rows,
      })});
      return mac;
    },
    scenario: async ({reader, el, mac, until, check, refresh}) => {
      const view = el('view'), messages = el('messages');
      const ids = () => [...messages.children].map(article => article.dataset.id).join(',');
      function gesture(x, endX = x, target = view, dy = 0) {
        let consumed = false;
        for (const [type, cx, cy] of [['touchstart', x, 120], ['touchmove', endX, 120 + dy], ['touchend', endX, 120 + dy]]) {
          const touch = new reader.Touch({identifier: 0, target, clientX: cx, clientY: cy});
          const event = new reader.TouchEvent(type, {bubbles: true, cancelable: true,
            touches: type === 'touchend' ? [] : [touch], changedTouches: [touch]});
          if (!target.dispatchEvent(event)) consumed = true;
        }
        return consumed;
      }
      await until(() => ids() === '2,3,4', '最新轮缺少同轮提问或混入前轮');
      check(view.scrollTop === 0 && el('next').disabled, '进入长轮没有从头开始');
      gesture(550);
      const step = view.scrollTop;
      const line = parseFloat(reader.getComputedStyle(messages.querySelector('.markdown')).lineHeight);
      check(Math.abs(step - (view.clientHeight - line)) < 2, '轻点右侧未保留一行重叠');
      gesture(30);
      check(view.scrollTop === 0, '轻点左侧没有回到页首');
      gesture(30);
      check(ids() === '2,3,4' && view.scrollTop === 0, '页首轻点误翻轮');
      check(!gesture(100, 200, messages.querySelector('table')), '拦截了宽表横滑');
      check(!gesture(100, 100, messages.querySelector('a')), '拦截了链接');
      check(!gesture(100, 103, view, 70), '拦截了纵向拖动');
      gesture(550);
      mac.next.push({id: '5', role: 'assistant', order: 5, text: '第三轮', startsTurn: true});
      refresh();
      await until(() => !el('latest').hidden, '回看长文时未提示新内容');
      check(ids() === '2,3,4' && view.scrollTop === step, '新轮打断阅读');
      gesture(100, 220);
      check(ids() === '1' && el('prev').disabled, '右滑未进入第一轮');
      gesture(220, 100);
      check(ids() === '2,3,4' && view.scrollTop === 0, '左滑未从下一轮开头读起');
      view.scrollTop = view.scrollHeight;
      gesture(550);
      check(ids() === '2,3,4', '页末轻点误翻轮');
      el('next').click();
      check(ids() === '5' && el('next').disabled, '按钮未切到最新轮');
      mac.next.push({id: '6', role: 'user', order: 6, text: '第四轮', startsTurn: true});
      refresh();
      await until(() => ids() === '6', '末尾未跟随新的对话轮次');
    },
  });
});

test('向前翻轮的请求遇到在途更新时仍按一次点击切换', {skip: !hasChrome}, async () => {
  const entryPath = '/read/current/sample';
  const entry = {key: 'sample', source: 'Codex CLI', project: '示例', title: '历史分页'};
  await runReaderBrowser({
    entryPath, documents: {[entryPath]: {entry, page: 'thread', title: '历史分页'}},
    setup: () => {
      const row = (id, text) => ({id, order: Number(id), role: 'assistant', text, startsTurn: true});
      const mac = {hold: false};
      mac.fetch = async url => {
        const page = {since: 'rev', messages: []};
        if (url.includes('before=')) Object.assign(page, {before: null, messages: [row('0', '上一轮')]});
        else if (!url.includes('since=')) Object.assign(page, {before: 'older', messages: [row('1', '最新轮')]});
        else if (mac.hold) {
          mac.hold = false;
          await new Promise(resolve => { mac.release = resolve; });
          page.messages = [row('1', '最新轮更新')];
        }
        return {ok: true, json: async () => page};
      };
      return mac;
    },
    scenario: async ({el, mac, until, refresh}) => {
      await until(() => el('messages').textContent.includes('最新轮'), '未载入最新轮');
      mac.hold = true;
      refresh();
      await until(() => !!mac.release, '没有在途请求');
      el('prev').click();
      mac.release();
      await until(() => el('messages').textContent.includes('上一轮'), '在途更新取消了翻轮');
    },
  });
});
