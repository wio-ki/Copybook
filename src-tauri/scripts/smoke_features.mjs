// Runs against a local Vite dev server; uses Windows' installed Edge, no browser download.
import assert from 'node:assert/strict';
import { mkdirSync } from 'node:fs';
import { chromium } from 'playwright-core';
import { spawn } from 'node:child_process';

const base = process.env.COPYBOOK_TEST_URL ?? 'http://127.0.0.1:1424';
const server = process.env.COPYBOOK_TEST_URL
  ? null
  : spawn(
      process.execPath,
      ['node_modules/vite/bin/vite.js', '--host', '127.0.0.1', '--port', '1424', '--strictPort'],
      { stdio: 'ignore', windowsHide: true },
    );
if (server) {
  let ready = false;
  for (let attempt = 0; attempt < 60; attempt++) {
    try {
      ready = (await fetch(base + '/panel.html', { signal: AbortSignal.timeout(2000) })).ok;
    } catch {
      /* Starting the local server. */
    }
    if (ready) break;
    await new Promise((resolve) => setTimeout(resolve, 500));
  }
  if (!ready) {
    server.kill();
    throw new Error('UI test server did not start');
  }
}
const browser = await chromium.launch({ channel: 'msedge', headless: true, args: ['--no-proxy-server'] });
const errors = [];
mkdirSync('.review/shots/v140', { recursive: true });
async function open(url, viewport = { width: 384, height: 576 }) {
  const context = await browser.newContext({
    viewport,
    locale: 'zh-CN',
    permissions: ['clipboard-read', 'clipboard-write'],
  });
  const page = await context.newPage();
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.goto(base + url, { waitUntil: 'domcontentloaded', timeout: 90000 });
  await page.locator('.panel, .settings').waitFor({ timeout: 90000 });
  return { context, page };
}
async function backend(page, fn) {
  return page.evaluate(fn);
}
try {
  {
    const { context, page } = await open('/panel.html?lang=zh-CN');
    await page.locator('.row').first().waitFor();
    await backend(page, async () => {
      const b = await (await import('/src/shared/api.ts')).getBackend();
      window.__pasteCalls = 0;
      b.pasteItem = async () => {
        window.__pasteCalls++;
        return { pasted: true, reason: null };
      };
    });
    await page
      .locator('.row')
      .nth(0)
      .click({ modifiers: ['Control'] });
    await page
      .locator('.row')
      .nth(2)
      .click({ modifiers: ['Shift'] });
    await page.getByText('已选 3 条', { exact: true }).waitFor();
    await page.locator('.row').nth(3).dblclick();
    assert.equal(await page.evaluate(() => window.__pasteCalls), 0);
    await page.locator('.selection-bar').getByRole('button', { name: '全选结果', exact: true }).click();
    const total = await backend(
      page,
      async () => (await (await import('/src/shared/api.ts')).api.getStats()).total,
    );
    await page.getByText(`已选 ${total} 条`, { exact: true }).waitFor();
    await page.screenshot({ path: '.review/shots/v140/multiselect.png' });
    await page.locator('.selection-bar').getByRole('button', { name: '删除', exact: true }).click();
    await page.getByRole('dialog').getByRole('button', { name: '删除', exact: true }).click();
    await page.getByRole('button', { name: '撤销', exact: true }).waitFor();
    assert.equal(
      await backend(page, async () => (await (await import('/src/shared/api.ts')).api.getStats()).total),
      0,
    );
    await page.getByRole('button', { name: '撤销', exact: true }).click();
    await page.locator('.row').first().waitFor();
    assert.equal(
      await backend(page, async () => (await (await import('/src/shared/api.ts')).api.getStats()).total),
      total,
    );
    await context.close();
    console.log(
      'Multi-select: Ctrl/Shift selection, select all, batch delete, undo, no accidental double-click paste',
    );
  }
  {
    const { context, page } = await open('/panel.html?lang=zh-CN');
    await page.locator('.row').first().waitFor();
    await page.getByRole('tab', { name: '文本', exact: true }).click();
    await page.waitForFunction(() =>
      [...document.querySelectorAll('.row')].every((e) => e.classList.contains('row--text')),
    );
    await page
      .locator('.row')
      .nth(0)
      .click({ modifiers: ['Control'] });
    await page.locator('.row').nth(1).click();
    await page.locator('.selection-bar').getByRole('button', { name: '合并', exact: true }).click();
    const preview = page.getByRole('textbox', { name: '合并预览', exact: true });
    await page.waitForFunction(() => !document.querySelector('dialog textarea').value.includes('正在加载'));
    const first = await preview.inputValue();
    assert.ok(first.length > 10);
    await page.getByLabel('反转所选顺序', { exact: true }).check();
    await page.waitForFunction(
      (old) =>
        document.querySelector('dialog textarea').value !== old &&
        !document.querySelector('dialog textarea').value.includes('正在加载'),
      first,
    );
    await page.getByRole('dialog').getByRole('button', { name: '复制', exact: true }).click();
    await page.getByRole('dialog').waitFor({ state: 'hidden' });
    assert.ok((await page.evaluate(() => navigator.clipboard.readText())).length > 10);
    await page.locator('.selection-bar').getByRole('button', { name: '分组', exact: true }).click();
    await page.getByRole('dialog').locator('input').fill('工作');
    await page.getByRole('button', { name: '保存', exact: true }).click();
    await page.getByRole('dialog').waitFor({ state: 'hidden' });
    await page.locator('.row-group').first().waitFor();
    await page.getByRole('button', { name: '筛选', exact: true }).click();
    await page.getByLabel('收藏分组', { exact: true }).selectOption({ label: '工作' });
    await page.waitForFunction(() => document.querySelectorAll('.row').length === 2);
    await page.screenshot({ path: '.review/shots/v140/group-filter.png' });
    await page.getByLabel('来源应用', { exact: true }).selectOption({ label: 'Windows Terminal' });
    await page.waitForFunction(() => document.querySelectorAll('.row').length === 1);
    await page.getByLabel('结束日期', { exact: true }).fill('2000-01-01');
    await page.waitForFunction(() => document.querySelectorAll('.row').length === 0);
    await page.getByRole('button', { name: '重置', exact: true }).click();
    await page.locator('.row').first().waitFor();
    await context.close();
    console.log(
      'Merge: full preview, reverse order, clipboard copy; groups: assign and filter; source/time filters combine',
    );
  }
  for (const lang of ['zh-CN', 'en-US']) {
    const { context, page } = await open(`/settings.html?lang=${lang}#data`, { width: 760, height: 650 });
    await page.locator('.webdav-backup').waitFor();
    const passwords = page.locator('.webdav-backup input[type=password]');
    assert.equal(await passwords.count(), 3);
    const fields = page.locator('.backup-fields input');
    await fields.nth(0).fill('https://dav.example.com/Copybook/');
    await fields.nth(1).fill('demo');
    await fields.nth(2).fill('server-secret');
    await fields.nth(3).fill('backup-secret');
    await fields.nth(4).fill('backup-secret');
    assert.ok(!(await page.evaluate(() => localStorage.getItem('copybook.webdav') ?? '')).includes('secret'));
    await page.screenshot({ path: `.review/shots/v140/webdav-${lang}.png` });
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
    await context.close();
  }
  assert.deepEqual(errors, []);
  console.log('WebDAV UI: bilingual layout, masked passwords, no secret persistence; no browser errors');
} finally {
  await browser.close();
  server?.kill();
}
