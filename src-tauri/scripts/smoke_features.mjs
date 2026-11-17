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
mkdirSync('.review/shots/v150', { recursive: true });
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
    await backend(page, async () => (await import('/src/shared/api.ts')).api.createGroup('工作/项目'));
    await page.locator('.selection-bar').getByRole('button', { name: '移至文件夹', exact: true }).click();
    await page.getByLabel('目标文件夹', { exact: true }).selectOption('工作/项目');
    await page.getByRole('dialog').getByRole('button', { name: '移动', exact: true }).click();
    await page.getByRole('dialog').waitFor({ state: 'hidden' });
    await page.locator('.row-group').first().waitFor();
    await page.getByRole('tab', { name: '收藏', exact: true }).click();
    await page.locator('[data-folder="工作"] .folder-open').click();
    await page.locator('[data-folder="工作/项目"] .folder-open').click();
    await page.getByRole('button', { name: '筛选', exact: true }).click();
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
      'Merge: full preview, reverse order, clipboard copy; folders: move and navigate; source/time filters combine',
    );
  }
  {
    const { context, page } = await open('/panel.html?lang=zh-CN');
    await page.locator('.row').first().waitFor();
    assert.equal(await page.getByRole('button', { name: '分组', exact: true }).count(), 0);
    await page.getByRole('tab', { name: '收藏', exact: true }).click();
    const create = async (name) => {
      await page.getByRole('button', { name: '新建文件夹', exact: true }).click();
      await page.getByLabel('文件夹名称', { exact: true }).fill(name);
      await page.getByRole('dialog').getByRole('button', { name: '保存', exact: true }).click();
      await page.getByRole('dialog').waitFor({ state: 'hidden' });
    };
    const enter = async (path) => page.locator(`[data-folder="${path}"] .folder-open`).click();
    const action = async (path, label) => {
      await page.locator(`[data-folder="${path}"] > .icon-btn`).click();
      await page.getByRole('menuitem', { name: label }).click();
    };
    await create('工作');
    await action('工作', '新建子文件夹');
    await page.getByLabel('文件夹名称', { exact: true }).fill('项目');
    await page.getByRole('dialog').getByRole('button', { name: '保存', exact: true }).click();
    await page.getByRole('dialog').waitFor({ state: 'hidden' });
    await enter('工作');
    await enter('工作/项目');
    await create('资料');
    const movedIds = await backend(page, async () => {
      const api = (await import('/src/shared/api.ts')).api;
      const { items } = await api.listItems({ query: '', filter: 'text', offset: 0, limit: 2 });
      const ids = items.map((i) => i.id);
      await api.setItemGroup(ids, '工作/项目/资料');
      return ids;
    });
    await enter('工作/项目/资料');
    await page.waitForFunction(() => document.querySelectorAll('.row').length === 2);
    await page.getByRole('button', { name: '多选', exact: true }).click();
    await page.getByRole('button', { name: '全选结果', exact: true }).click();
    await page.getByText('已选 2 条', { exact: true }).waitFor();
    await page.getByRole('button', { name: '移至文件夹', exact: true }).click();
    await page.getByLabel('目标文件夹', { exact: true }).selectOption('工作/项目');
    await page.getByRole('dialog').getByRole('button', { name: '移动', exact: true }).click();
    await page.getByRole('dialog').waitFor({ state: 'hidden' });
    await page.waitForFunction(() => document.querySelectorAll('.row').length === 0);
    await page.getByRole('button', { name: '返回上一级', exact: true }).click();
    await page.waitForFunction(() => document.querySelectorAll('.row').length === 2);
    await page.screenshot({ path: '.review/shots/v150/nested-folders.png' });
    await page.getByRole('navigation', { name: '收藏目录' }).getByRole('button', { name: '收藏', exact: true }).click();
    await action('工作', '重命名');
    await page.getByLabel('文件夹名称', { exact: true }).fill('办公');
    await page.getByRole('dialog').getByRole('button', { name: '保存', exact: true }).click();
    await page.getByRole('dialog').waitFor({ state: 'hidden' });
    await page.locator('[data-folder="办公"]').waitFor();
    assert.ok((await backend(page, async () => (await import('/src/shared/api.ts')).api.listGroups())).includes('办公/项目/资料'));
    await create('归档');
    await action('办公', '移动文件夹');
    await page.getByLabel('目标目录', { exact: true }).selectOption('归档');
    await page.getByRole('dialog').getByRole('button', { name: '保存', exact: true }).click();
    await page.getByRole('dialog').waitFor({ state: 'hidden' });
    await enter('归档');
    await page.locator('[data-folder="归档/办公"]').waitFor();
    await page.getByRole('button', { name: '返回上一级', exact: true }).click();
    await action('归档', '移除文件夹');
    await page.getByRole('dialog').getByRole('button', { name: '移除', exact: true }).click();
    await page.getByRole('dialog').waitFor({ state: 'hidden' });
    const result = await page.evaluate(async (ids) => {
      const api = (await import('/src/shared/api.ts')).api;
      return Promise.all(ids.map((id) => api.getItem(id)));
    }, movedIds);
    assert.ok(result.every((i) => i.favorite && i.group == null));
    assert.deepEqual(await backend(page, async () => (await import('/src/shared/api.ts')).api.listGroups()), []);
    await context.close();
    console.log('Folders: create empty folders, three levels, breadcrumbs, batch moves, subtree rename/move/remove preserve favorites');
  }
  for (const lang of ['zh-CN', 'en-US']) {
    for (const width of [320, 384, 480]) {
      const { context, page } = await open(`/panel.html?lang=${lang}`, { width, height: 576 });
      await page.locator('.row').first().waitFor();
      await backend(page, async () => {
        const api = (await import('/src/shared/api.ts')).api;
        await api.createGroup('工作文件夹名称测试'.repeat(4).slice(0, 40) + '/项目/资料');
      });
      await page.getByRole('tab', { name: lang === 'zh-CN' ? '收藏' : 'Starred', exact: true }).click();
      await page.locator('.folder-open').first().waitFor();
      await page.waitForFunction(() => document.querySelectorAll('.row').length === 2);
      await page.screenshot({ path: `.review/shots/v150/folders-${lang}-${width}.png` });
      assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
      assert.equal(await page.getByRole('button', { name: lang === 'zh-CN' ? '分组' : 'Groups', exact: true }).count(), 0);
      await page.locator('.folder-open').first().click();
      await page.waitForFunction(() => document.querySelectorAll('.row').length === 0);
      await page.screenshot({ path: `.review/shots/v150/folders-nested-${lang}-${width}.png` });
      assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
      await context.close();
    }
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
