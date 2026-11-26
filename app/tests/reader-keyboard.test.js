import test from 'node:test';
import { hasChrome, runReaderBrowser } from './reader-browser.js';

test('输入法覆盖页面底部时，仍能编辑回复并点到发送和收起', {skip: !hasChrome}, async () => {
  const entry = {key: 'codex-sample', source: 'Codex CLI', project: '示例', title: '测试键盘', state: 'idle'};
  const entryPath = '/read/current/codex-sample';
  await runReaderBrowser({
    entryPath,
    documents: {[entryPath]: {entry, page: 'thread', title: 'Codex CLI · 空闲 · 示例 · 测试键盘'}},
    setup: () => {
      const messages = Array.from({length: 45}, (_, i) => ({
        id: String(i), order: i, role: 'user', text: '这是一段可回看的消息。',
      }));
      return {fetch: async () => ({ok: true, json: async () => ({messages, replyTicket: 'sample'})})};
    },
    scenario: async ({reader, el, until, check}) => {
      await until(() => !el('open-reply').hidden, '未显示回复入口');
      el('open-reply').click();
      const view = el('view');
      check(view.scrollHeight - view.clientHeight - view.scrollTop < 2, '展开输入框后没有跟随正文末尾');
      const field = el('reply-text');
      field.value = '在墨水屏上输入中文';
      field.dispatchEvent(new Event('input'));
      // Some IMEs cover the WebView without changing its reported dimensions.
      const doc = reader.document;
      const keyboard = doc.createElement('aside');
      keyboard.style.cssText = 'position:fixed;top:120px;bottom:0;left:0;right:0;z-index:999;background:white';
      doc.body.append(keyboard);
      const visible = [field, el('send-reply'), el('cancel-reply')].every(control => {
        const rect = control.getBoundingClientRect();
        return rect.top >= 0 && rect.bottom <= keyboard.getBoundingClientRect().top &&
          doc.elementFromPoint(rect.x + rect.width / 2, rect.y + rect.height / 2) === control;
      });
      check(visible && doc.activeElement === field, '键盘遮挡输入或操作控件');
    },
  });
});
