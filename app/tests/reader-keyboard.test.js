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

test('输入法覆盖页面底部时，仍能编辑回复并点到发送和收起', {skip: !existsSync(chrome)}, async () => {
  const dir = await mkdtemp(join(tmpdir(), 'nomad-ime-'));
  try {
    const html = await readFile(new URL('reader.html', ui), 'utf8');
    const fixture = `<base href="${ui.href}"><script>
      window.fetch = async path => ({
        ok: true, json: async () => path === '/sessions'
          ? [{key:'codex-sample', source:'Codex CLI', project:'示例', state:'idle'}]
          : {messages:[], replyTicket:'sample'}
      });
      window.addEventListener('load', () => {
        const open = document.getElementById('open-reply');
        const check = () => {
          if (open.hidden) return setTimeout(check, 10);
          open.click();
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
    const file = join(dir, 'reader.html');
    await writeFile(file, html.replaceAll('src="/', 'src="').replaceAll('href="/', 'href="').replace('<head>', '<head>' + fixture));
    const {stdout} = await promisify(execFile)(chrome, [
      '--headless', '--disable-gpu', '--no-first-run', '--allow-file-access-from-files',
      '--window-size=620,440', '--virtual-time-budget=1500', '--dump-dom',
      '--user-data-dir=' + join(dir, 'profile'), pathToFileURL(file).href,
    ], {timeout: 20000});
    assert.equal(stdout.match(/<body[^>]*data-ime-result="([^"]+)"/)?.[1], 'pass');
  } finally {
    await rm(dir, {recursive: true, force: true, maxRetries: 3});
  }
});
