// In-memory backend for `npm run dev` in a plain browser, so the UI can be designed and
// checked without the Rust side. Never loaded inside Tauri.

import type { Backend } from './api';
import {
  DEFAULT_SETTINGS,
  type ClipItem,
  type EventMap,
  type FileEntry,
  type ListQuery,
  type SeenApp,
  type Settings,
  type SourceApp,
  type UpdateInfo,
  type UpdateProgress,
} from './contract';

const svg = (body: string, w: number, h: number) =>
  `data:image/svg+xml;utf8,${encodeURIComponent(
    `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}" viewBox="0 0 ${w} ${h}">${body}</svg>`,
  )}`;

const appIcon = (letter: string, bg: string, fg = '#fff') =>
  svg(
    `<rect width="32" height="32" rx="7" fill="${bg}"/><text x="16" y="22" font-family="Segoe UI" font-weight="700" font-size="16" fill="${fg}" text-anchor="middle">${letter}</text>`,
    32,
    32,
  );

const APPS: Record<string, SourceApp> = {
  code: { name: 'Visual Studio Code', exe: 'Code.exe', icon: appIcon('{ }', '#1f6fb2') },
  edge: { name: 'Microsoft Edge', exe: 'msedge.exe', icon: appIcon('e', '#0f7a6b') },
  wechat: { name: '微信', exe: 'Weixin.exe', icon: appIcon('微', '#22a845') },
  explorer: { name: '文件资源管理器', exe: 'explorer.exe', icon: appIcon('▤', '#d8a12a') },
  terminal: { name: 'Windows Terminal', exe: 'WindowsTerminal.exe', icon: appIcon('>_', '#2b2b2b') },
  figma: { name: 'Figma', exe: 'Figma.exe', icon: appIcon('F', '#a259ff') },
  notepad: { name: '记事本', exe: 'Notepad.exe', icon: appIcon('N', '#2f6fd6') },
  snip: { name: '截图工具', exe: 'SnippingTool.exe', icon: appIcon('✂', '#c2410c') },
  obsidian: { name: 'Obsidian', exe: 'Obsidian.exe', icon: appIcon('◆', '#6f42c1') },
};

const screenshot = svg(
  `<rect width="1280" height="800" fill="#eef1f5"/>
   <rect width="1280" height="56" fill="#1f2430"/>
   <circle cx="32" cy="28" r="8" fill="#ff5f57"/><circle cx="60" cy="28" r="8" fill="#febc2e"/><circle cx="88" cy="28" r="8" fill="#28c840"/>
   <rect x="0" y="56" width="248" height="744" fill="#e3e7ee"/>
   <rect x="32" y="96" width="160" height="14" rx="7" fill="#c3cad6"/><rect x="32" y="132" width="120" height="14" rx="7" fill="#c3cad6"/><rect x="32" y="168" width="140" height="14" rx="7" fill="#c3cad6"/>
   <rect x="296" y="104" width="420" height="28" rx="6" fill="#1f2430"/>
   <rect x="296" y="160" width="900" height="16" rx="8" fill="#c9d0db"/><rect x="296" y="192" width="820" height="16" rx="8" fill="#c9d0db"/><rect x="296" y="224" width="860" height="16" rx="8" fill="#c9d0db"/>
   <rect x="296" y="288" width="420" height="260" rx="12" fill="#fff"/><rect x="744" y="288" width="440" height="260" rx="12" fill="#fff"/>
   <rect x="328" y="320" width="200" height="16" rx="8" fill="#3b82f6"/><rect x="776" y="320" width="160" height="16" rx="8" fill="#f97316"/>`,
  1280,
  800,
);

const photo = svg(
  `<defs><linearGradient id="s" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#f6c99a"/><stop offset="0.55" stop-color="#e98a6b"/><stop offset="1" stop-color="#6b4b7a"/></linearGradient></defs>
   <rect width="960" height="1280" fill="url(#s)"/><circle cx="480" cy="560" r="150" fill="#fff3d6" opacity="0.85"/>
   <path d="M0 900 L240 700 L420 860 L640 640 L960 920 L960 1280 L0 1280Z" fill="#3d2c4f"/>
   <path d="M0 1040 L300 880 L560 1010 L780 900 L960 980 L960 1280 L0 1280Z" fill="#2a1f38"/>`,
  960,
  1280,
);

const logo = svg(
  `<rect width="512" height="512" fill="none"/><rect x="96" y="96" width="320" height="320" rx="56" fill="#b94631"/><path d="M200 176h112v160H200z" fill="none" stroke="#fff" stroke-width="20"/>`,
  512,
  512,
);

const now = Date.now();
const min = 60_000;
const hour = 60 * min;
const day = 24 * hour;

let nextId = 1;

function text(
  value: string,
  source: SourceApp | null,
  ago: number,
  extra: Partial<ClipItem> = {},
): ClipItem {
  const full = value;
  return {
    id: nextId++,
    kind: 'text',
    subKind: null,
    text: full.slice(0, 2000),
    textLength: [...full].length,
    lineCount: full.split(/\r?\n/).length,
    hasHtml: false,
    hasRtf: false,
    image: null,
    files: null,
    source,
    favorite: false,
    pinned: false,
    note: null,
    group: null,
    useCount: 1,
    createdAt: now - ago - 3 * day,
    lastUsedAt: now - ago,
    ...extra,
  };
}

function image(src: string, w: number, h: number, bytes: number, source: SourceApp | null, ago: number, extra: Partial<ClipItem> = {}): ClipItem {
  return {
    ...text('', source, ago),
    kind: 'image',
    text: '',
    textLength: 0,
    lineCount: 0,
    image: { path: src, thumb: src, width: w, height: h, bytes },
    ...extra,
  };
}

function files(paths: { path: string; isDir?: boolean; bytes?: number | null; exists?: boolean }[], source: SourceApp | null, ago: number): ClipItem {
  const entries: FileEntry[] = paths.map((p) => ({
    path: p.path,
    name: p.path.split('\\').pop() ?? p.path,
    isDir: p.isDir ?? false,
    exists: p.exists ?? true,
    bytes: p.isDir ? null : (p.bytes ?? 0),
  }));
  const joined = entries.map((e) => e.path).join('\n');
  return { ...text(joined, source, ago), kind: 'files', files: entries };
}

const SEED: ClipItem[] = [
  text('pnpm tauri build --target x86_64-pc-windows-msvc', APPS.terminal, 40 * 1000),
  text(
    `export function relativeTime(ms: number, now = Date.now()): string {\n  const diff = Math.max(0, now - ms);\n  if (diff < 45_000) return t.time.now;\n  if (diff < 3_600_000) return t.time.minutes(Math.round(diff / 60_000));\n  return formatClock(ms);\n}`,
    APPS.code,
    3 * min,
  ),
  text('https://github.com/EcoPasteHub/EcoPaste/blob/master/src-tauri/src/clipboard/watcher.rs', APPS.edge, 7 * min, {
    subKind: 'url',
  }),
  image(screenshot, 1280, 800, 482_113, APPS.snip, 12 * min),
  text('周五下午三点在 3 号会议室过一遍新版设计稿，记得带上上周的反馈清单。', APPS.wechat, 26 * min, {
    hasHtml: true,
  }),
  text('#B94631', APPS.figma, 41 * min, { subKind: 'color' }),
  files(
    [
      { path: 'C:\\Users\\PC\\Documents\\季度报告-2026Q3.xlsx', bytes: 284_901 },
      { path: 'C:\\Users\\PC\\Documents\\会议纪要.docx', bytes: 48_210 },
      { path: 'C:\\Users\\PC\\Pictures\\Screenshots', isDir: true },
    ],
    APPS.explorer,
    58 * min,
  ),
  text('wio-ki@users.noreply.github.com', APPS.edge, 2 * hour, { subKind: 'email' }),
  text('C:\\Program Files\\Copybook\\copybook.exe', APPS.explorer, 3 * hour, { subKind: 'path' }),
  text(
    '剪贴板历史不应该只是一个列表。它更像一本册子：你抄下来的每一段话，都按时间顺序誊在纸上，需要的时候翻回去就能找到。',
    APPS.obsidian,
    5 * hour,
    { favorite: true, note: '产品文案草稿' },
  ),
  image(photo, 960, 1280, 1_204_331, APPS.edge, 20 * hour, { favorite: true }),
  text('SELECT id, kind, last_used_at FROM items WHERE pinned = 1 ORDER BY last_used_at DESC;', APPS.code, 26 * hour),
  text('rgb(34 197 94 / 0.8)', APPS.figma, 28 * hour, { subKind: 'color' }),
  text('Meeting moved to Thursday 10:00 — same room. Please bring the Q3 numbers.', APPS.notepad, 30 * hour, {
    hasRtf: true,
  }),
  text('https://www.figma.com/design/kL0fA9/Copybook?node-id=12-408', APPS.figma, 2 * day, { subKind: 'url' }),
  files([{ path: 'D:\\Downloads\\copybook_0.1.0_x64-setup.exe', bytes: 4_812_032, exists: false }], APPS.explorer, 3 * day),
  image(logo, 512, 512, 18_400, APPS.figma, 4 * day),
  text('ssh -i ~/.ssh/id_ed25519 root@203.0.113.7 -p 22', APPS.terminal, 5 * day, { pinned: true, useCount: 14 }),
  text('收件地址：上海市徐汇区漕溪北路 88 号 12 楼', APPS.wechat, 6 * day, { pinned: true, useCount: 6 }),
  ...Array.from({ length: 26 }, (_, i) =>
    text(
      [
        'git commit -m "fix: keep selection when new items arrive"',
        '感谢反馈，这个问题已经在 0.1.1 修复了。',
        'const unlisten = await listen("history://changed", refresh);',
        'Lorem ipsum dolor sit amet, consectetur adipiscing elit.',
        'npm run tauri dev',
        '下周一之前把发票发到财务邮箱。',
      ][i % 6],
      [APPS.terminal, APPS.wechat, APPS.code, APPS.notepad, APPS.terminal, APPS.wechat][i % 6],
      (7 + i) * day,
    ),
  ),
];

export function createMockBackend(): Backend {
  // Dev-only URL switches for screenshots: ?empty=1&paused=1&density=compact&theme=dark&lang=en-US
  const params = new URLSearchParams(location.search);
  let items = params.has('empty') ? [] : SEED.map((i) => ({ ...i }));
  let settings: Settings = {
    ...DEFAULT_SETTINGS,
    ...(params.get('density') === 'compact' ? { density: 'compact' as const } : {}),
    ...(params.get('theme') === 'dark' || params.get('theme') === 'light'
      ? { theme: params.get('theme') as 'dark' | 'light' }
      : {}),
    ...(params.get('lang') === 'en-US' || params.get('lang') === 'zh-CN'
      ? { language: params.get('lang') as 'en-US' | 'zh-CN' }
      : {}),
    ...(params.has('plain') ? { pastePlain: true } : {}),
    ...(params.get('source') === '0' ? { showSource: false } : {}),
  };
  const folders = new Set<string>();
  const addFolder = (path: string) => {
    const parts = path.split('/');
    for (let i = 1; i <= parts.length; i++) folders.add(parts.slice(0, i).join('/'));
  };
  const validateFolder = (path: string) => {
    if (!path || path.split('/').length > 32 || /[\x00-\x1f\x7f]/.test(path) || path.split('/').some((part) => !part || part !== part.trim() || [...part].length > 40)) throw { code: 'invalid' };
  };
  const inFolder = (path: string, parent: string) => path === parent || path.startsWith(parent + '/');
  let undo: { items: ClipItem[]; deadline: number } | null = null;
  let paused = params.has('paused');
  let elevated = params.get('admin') === '1';
  const updateMode = params.get('update');
  let checkedUpdate: UpdateInfo | null = null;
  let installAttempts = 0;
  let updateProgress: UpdateProgress = {
    stage: updateMode === 'downloading' || updateMode === 'installing' ? updateMode : 'idle',
    version: updateMode === 'downloading' || updateMode === 'installing' ? '0.2.0' : null,
    downloaded: updateMode === 'downloading' ? 2 * 1024 * 1024 : 0,
    total: updateMode === 'downloading' ? 4 * 1024 * 1024 : null,
  };
  const listeners = new Map<string, Set<(p: unknown) => void>>();

  const emit = <E extends keyof EventMap>(event: E, payload: EventMap[E]) => {
    listeners.get(event)?.forEach((fn) => fn(payload));
  };
  const changed = (reason: EventMap['history://changed']['reason'], id: number | null) =>
    emit('history://changed', { reason, id });
  const publishProgress = (value: UpdateProgress) => {
    updateProgress = value;
    emit('update://progress', { ...value });
  };
  const updating = () => updateProgress.stage === 'downloading' || updateProgress.stage === 'installing';

  const find = (id: number) => {
    const item = items.find((i) => i.id === id);
    if (!item) throw { code: 'not_found', message: `item ${id} not found` };
    return item;
  };

  const matches = (item: ClipItem, q: ListQuery) => {
    switch (q.filter) {
      case 'text':
      case 'image':
      case 'files':
        if (item.kind !== q.filter) return false;
        break;
      case 'url':
        if (item.subKind !== 'url') return false;
        break;
      case 'favorite':
        if (!item.favorite) return false;
        break;
    }
    if (q.sourceExe && item.source?.exe.toLowerCase() !== q.sourceExe.toLowerCase()) return false;
    if (q.since != null && item.lastUsedAt < q.since) return false;
    if (q.until != null && item.lastUsedAt >= q.until) return false;
    if (q.group != null && (!item.favorite || (item.group ?? '') !== q.group)) return false;
    const terms = q.query.trim().toLowerCase().split(/\s+/).filter(Boolean);
    const hay = `${item.text}\n${item.note ?? ''}`.toLowerCase();
    return terms.every((t) => hay.includes(t));
  };

  const ranks = new Map<number, number>();
  const ordered = (folder = false) =>
    [...items].sort(
      (a, b) => Number(b.pinned) - Number(a.pinned)
        || (folder ? (ranks.get(a.id) ?? Number.MAX_SAFE_INTEGER) - (ranks.get(b.id) ?? Number.MAX_SAFE_INTEGER) : 0)
        || b.lastUsedAt - a.lastUsedAt || b.id - a.id,
    );

  const bump = (item: ClipItem) => {
    if (!settings.moveToTop) return;
    item.lastUsedAt = Date.now();
    item.useCount += 1;
    changed('bumped', item.id);
  };

  const delay = <T>(value: T, ms = 60) => new Promise<T>((r) => setTimeout(() => r(value), ms));

  // Dev helper: window.__copy('some text') simulates a capture.
  (window as unknown as { __copy: (t: string) => void }).__copy = (value: string) => {
    const item = text(value, APPS.notepad, 0);
    item.createdAt = Date.now();
    items.unshift(item);
    changed('added', item.id);
  };

  return {
    async listItems(q) {
      const all = ordered(q.group != null).filter((i) => matches(i, q));
      return delay({ items: all.slice(q.offset, q.offset + q.limit).map((i) => ({ ...i })), total: all.length });
    },
    async getItem(id) {
      return delay({ ...find(id) });
    },
    async getItemContent(id, maxChars) {
      const item = find(id);
      const full = item.text;
      return delay({
        text: full.slice(0, maxChars),
        truncated: full.length > maxChars,
        html: item.hasHtml ? `<p>${full}</p>` : null,
      }, 120);
    },
    async pasteItem(id) {
      bump(find(id));
      emit('panel://hidden', {});
      return delay({ pasted: true, reason: null });
    },
    async copyItem(id) {
      bump(find(id));
      return delay(undefined);
    },
    async setFavorite(id, value) {
      const item = find(id);
      item.favorite = value;
      if (!value) {
        item.group = null;
        ranks.delete(id);
      }
      changed('updated', id);
      return delay({ ...item });
    },
    async setPinned(id, value) {
      const item = find(id);
      if (item.pinned !== value) ranks.delete(id);
      item.pinned = value;
      changed('updated', id);
      return delay({ ...item });
    },
    async setNote(id, note) {
      const item = find(id);
      item.note = note?.trim() ? note.trim() : null;
      changed('updated', id);
      return delay({ ...item });
    },
    async deleteItems(ids) {
      const before = items.length;
      undo = {items: items.filter((i) => ids.includes(i.id)), deadline: Date.now() + 15000};
      items = items.filter((i) => !ids.includes(i.id));
      changed('deleted', ids.length === 1 ? ids[0] : null);
      return delay(before - items.length);
    },
    async listItemIds(q) { return ordered(q.group != null).filter(i => matches(i,q)).map(i=>i.id); },
    async undoDelete() {
      if (!undo || Date.now() >= undo.deadline) return 0;
      const n = undo.items.length; items.push(...undo.items); undo = null; changed('added',null); return n;
    },
    async getMergedText(ids, separator) {
      if (ids.some(id=>find(id).kind !== 'text')) throw {code:'invalid'};
      return ids.map(id=>find(id).text).join(separator);
    },
    async copyMerged(ids, separator) { const value = ids.map(id=>find(id).text).join(separator); await navigator.clipboard.writeText(value); },
    async listGroups() { return [...folders].sort(); },
    async createGroup(name) {
      validateFolder(name);
      if (folders.has(name)) throw { code: 'invalid' };
      addFolder(name); changed('updated', null);
    },
    async createSnippet(group, value) {
      if (group && !folders.has(group)) throw { code: 'not_found' };
      const time = Date.now();
      const item = text(value, null, 0, { text: value, favorite: true, group, createdAt: time, lastUsedAt: time });
      items.push(item);
      changed('added', item.id);
      return { ...item };
    },
    async editSnippet(id, value) {
      const item = find(id);
      if (item.kind !== 'text' || !item.favorite) throw { code: 'invalid' };
      if (item.text !== value) {
        Object.assign(item, {
          text: value, textLength: [...value].length, lineCount: value.split('\n').length,
          hasHtml: false, hasRtf: false, source: null, subKind: null,
        });
      }
      changed('updated', id);
      return { ...item };
    },
    async moveFavoriteItem(id, direction) {
      const item = find(id);
      if (!item.favorite) throw { code: 'invalid' };
      const siblings = ordered(true).filter((entry) => entry.favorite && entry.group === item.group && entry.pinned === item.pinned);
      const from = siblings.indexOf(item);
      const to = from + (direction === 'up' ? -1 : 1);
      if (to < 0 || to >= siblings.length) return false;
      [siblings[from], siblings[to]] = [siblings[to], siblings[from]];
      siblings.forEach((entry, rank) => ranks.set(entry.id, rank));
      changed('updated', id);
      return true;
    },
    async setItemGroup(ids, group) {
      const path = group?.trim() || null;
      if (path) validateFolder(path);
      const targets = ids.map(find);
      if (path) addFolder(path);
      targets.forEach((item) => {
        if (item.group !== path) ranks.delete(item.id);
        item.group = path;
        item.favorite = true;
      });
      changed('updated', null); return ids.length;
    },
    async renameGroup(name, replacement) {
      const descendants = [...folders].filter((p) => inFolder(p, name));
      if (!descendants.length) throw { code: 'not_found' };
      if (replacement === name) return 0;
      if (replacement != null) {
        validateFolder(replacement);
        if (inFolder(replacement, name) || folders.has(replacement)) throw { code: 'invalid' };
        descendants.forEach((p) => validateFolder(replacement + p.slice(name.length)));
        descendants.forEach((p) => addFolder(replacement + p.slice(name.length)));
      }
      let n = 0;
      [...items, ...(undo?.items ?? [])].forEach((item) => {
        if (item.group && inFolder(item.group, name)) {
          if (replacement == null) ranks.delete(item.id);
          item.group = replacement == null ? null : replacement + item.group.slice(name.length); n++;
        }
      });
      descendants.forEach((p) => folders.delete(p));
      changed('updated', null); return n;
    },
    async webdavTest() { throw {code:'network',message:'WebDAV is available in the desktop app'}; },
    async webdavList() { throw {code:'network'}; },
    async webdavPasswordStatus() { return { access: false, encryption: false }; },
    async webdavSavePassword() { throw { code: 'io' }; },
    async webdavBackup() { throw {code:'network'}; },
    async webdavRestore() { throw {code:'network'}; },
    async clearHistory(keepMarked) {
      const before = items.length;
      items = keepMarked ? items.filter((i) => i.favorite || i.pinned) : [];
      changed('cleared', null);
      return delay(before - items.length);
    },
    async getStats() {
      return delay({
        total: items.length,
        text: items.filter((i) => i.kind === 'text').length,
        image: items.filter((i) => i.kind === 'image').length,
        files: items.filter((i) => i.kind === 'files').length,
        favorite: items.filter((i) => i.favorite).length,
        pinned: items.filter((i) => i.pinned).length,
        marked: items.filter((i) => i.favorite || i.pinned).length,
        dbBytes: 2_621_440,
        imageBytes: 14_903_296,
      });
    },
    async listSeenApps() {
      const counts = new Map<string, SeenApp>();
      for (const i of items) {
        if (!i.source) continue;
        const seen = counts.get(i.source.exe) ?? { ...i.source, count: 0 };
        seen.count += 1;
        counts.set(i.source.exe, seen);
      }
      return delay([...counts.values()].sort((a, b) => b.count - a.count));
    },
    async getSettings() {
      return delay({ ...settings });
    },
    async updateSettings(patch) {
      if (patch.hotkey === 'Ctrl+KeyC') throw { code: 'hotkey_conflict', message: 'in use' };
      settings = { ...settings, ...patch };
      emit('settings://changed', { ...settings });
      return delay({ ...settings });
    },
    async getCapturePaused() {
      return delay(paused);
    },
    async setCapturePaused(value) {
      paused = value;
      emit('capture://paused', { paused });
      return delay(undefined);
    },
    async hidePanel() {
      emit('panel://hidden', {});
      return delay(undefined);
    },
    async setPanelLocked() {
      return delay(undefined);
    },
    async openSettings(section) {
      window.open(`/settings.html${section ? `#${section}` : ''}`, '_blank');
      return delay(undefined);
    },
    async openUrl(url) {
      window.open(url, '_blank', 'noopener');
      return delay(undefined);
    },
    async openPath() {
      return delay(undefined);
    },
    async revealPath() {
      return delay(undefined);
    },
    async openDataDir() {
      return delay(undefined);
    },
    async getAppInfo() {
      return delay({
        version: '0.1.0',
        dataDir: 'C:\\Users\\PC\\AppData\\Roaming\\io.github.wio-ki.copybook',
        elevated,
      });
    },
    async restartAsAdmin() {
      if (params.get('admin') === 'error') throw { code: 'io', message: 'Could not restart' };
      if (elevated || params.get('admin') === 'cancel') return delay(false);
      elevated = true;
      return delay(true);
    },
    // Dev-only update scenarios: none, check-error, download-error, signature-error,
    // install-error, unknown (no Content-Length), slow, downloading, installing.
    async checkUpdate() {
      if (updating()) throw { code: 'invalid', message: 'Update already in progress' };
      if (updateMode === 'check-error') throw { code: 'network', message: 'Mock update check failed' };
      const newer = updateMode !== 'none';
      checkedUpdate = {
        current: '0.1.0',
        latest: newer ? '0.2.0' : '0.1.0',
        newer,
        url: 'https://github.com/wio-ki/Copybook/releases',
        installer: newer
          ? 'https://github.com/wio-ki/Copybook/releases/download/v0.2.0/Copybook_0.2.0_x64-setup.exe'
          : null,
      };
      publishProgress({ stage: 'idle', version: newer ? checkedUpdate.latest : null, downloaded: 0, total: null });
      return delay({ ...checkedUpdate });
    },
    async installUpdate() {
      if (updating()) throw { code: 'invalid', message: 'Update already in progress' };
      if (!checkedUpdate?.newer) throw { code: 'invalid', message: 'Check for an update first' };
      installAttempts += 1;
      const version = checkedUpdate.latest;
      const size = 4 * 1024 * 1024;
      const total = updateMode === 'unknown' ? null : size;
      publishProgress({ stage: 'downloading', version, downloaded: 0, total });
      for (let step = 1; step <= 10; step += 1) {
        await delay(undefined, updateMode === 'slow' ? 700 : 100);
        const downloaded = Math.floor(size * step / 10);
        if (updateMode === 'download-error' && installAttempts === 1 && step === 4) {
          publishProgress({ stage: 'error', version, downloaded, total });
          throw { code: 'network', message: 'Mock download interrupted' };
        }
        publishProgress({ stage: 'downloading', version, downloaded, total });
      }
      if ((updateMode === 'signature-error' || updateMode === 'install-error') && installAttempts === 1) {
        publishProgress({ stage: 'error', version, downloaded: size, total });
        throw { code: 'io', message: `Mock ${updateMode}` };
      }
      // The browser cannot install or restart. Stay in the handoff stage rather than
      // pretending that the new application version has been installed successfully.
      publishProgress({ stage: 'installing', version, downloaded: size, total });
    },
    async getUpdateProgress() {
      return delay({ ...updateProgress });
    },
    async on(event, handler) {
      const set = listeners.get(event) ?? new Set();
      set.add(handler as (p: unknown) => void);
      listeners.set(event, set);
      return () => set.delete(handler as (p: unknown) => void);
    },
    assetUrl: (path) => path,
    async minimizeWindow() {},
    async closeWindow() {},
  };
}
