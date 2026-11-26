import { execFile } from 'node:child_process';
import { existsSync } from 'node:fs';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { promisify } from 'node:util';

const chrome = process.env.CHROME_BIN || '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
export const hasChrome = existsSync(chrome);
const ui = new URL('../ui/', import.meta.url);

export async function renderReader(fixture, virtualTimeMs) {
  const dir = await mkdtemp(join(tmpdir(), 'nomad-reader-'));
  try {
    const html = await readFile(new URL('reader.html', ui), 'utf8');
    const file = join(dir, 'reader.html');
    const head = '<head><base href="' + ui.href + '">' + fixture;
    await writeFile(file, html.replaceAll('src="/', 'src="').replaceAll('href="/', 'href="').replace('<head>', () => head));
    const {stdout} = await promisify(execFile)(chrome, [
      '--headless', '--disable-gpu', '--no-first-run', '--allow-file-access-from-files',
      '--window-size=620,440', '--virtual-time-budget=' + virtualTimeMs, '--dump-dom',
      '--user-data-dir=' + join(dir, 'profile'), pathToFileURL(file).href,
    ], {timeout: 20000});
    return stdout;
  } finally {
    await rm(dir, {recursive: true, force: true, maxRetries: 3});
  }
}
