import test from 'node:test';
import assert from 'node:assert/strict';
import { hasChrome, renderReader } from './reader-browser.js';

test('输入法覆盖页面底部时，仍能编辑回复并点到发送和收起', {skip: !hasChrome}, async () => {
  const fixture = `<script>
    const messages = Array.from({length: 45}, (_, i) => ({
      id: String(i), order: i, role: 'user', text: '这是一段可回看的消息。'
    }));
    window.fetch = async path => ({
      ok: true, json: async () => path === '/sessions'
        ? [{key:'codex-sample', source:'Codex CLI', project:'示例', state:'idle'}]
        : {messages, replyTicket:'sample'}
    });
    window.addEventListener('load', () => {
      const open = document.getElementById('open-reply');
      const check = () => {
        if (open.hidden) return setTimeout(check, 10);
        open.click();
        const view = document.getElementById('view');
        document.body.dataset.followedTail = view.scrollHeight - view.clientHeight - view.scrollTop < 2;
        const field = document.getElementById('reply-text');
        field.value = '在墨水屏上输入中文';
        field.dispatchEvent(new Event('input'));
        // Some IMEs cover the WebView without changing its reported dimensions.
        const keyboard = document.createElement('aside');
        keyboard.style.cssText = 'position:fixed;top:120px;bottom:0;left:0;right:0;z-index:999;background:white';
        document.body.append(keyboard);
        const visible = [field, document.getElementById('send-reply'), document.getElementById('cancel-reply')].every(el => {
          const r = el.getBoundingClientRect();
          return r.top >= 0 && r.bottom <= keyboard.getBoundingClientRect().top &&
            document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2) === el;
        });
        document.body.dataset.imeResult = visible && document.activeElement === field ? 'pass' : 'covered';
      };
      check();
    });
  </script>`;
  const stdout = await renderReader(fixture, 1500);
  assert.equal(stdout.match(/<body[^>]*data-ime-result="([^"]+)"/)?.[1], 'pass');
  assert.equal(stdout.match(/<body[^>]*data-followed-tail="([^"]+)"/)?.[1], 'true', '展开输入框后仍跟随正文末尾');
});
