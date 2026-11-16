import { convertFileSrc, invoke } from '@tauri-apps/api/core';
import type {
  AppInfo,
  ClipItem,
  CmdError,
  ErrorCode,
  EventMap,
  ItemContent,
  ListQuery,
  ListResult,
  PasteOutcome,
  SeenApp,
  Settings,
  SettingsSection,
  Stats,
} from './contract';

/** Everything the UI may ask of the backend. Mirrors section 5 of docs/contract.md. */
export interface Backend {
  listItems(query: ListQuery): Promise<ListResult>;
  getItem(id: number): Promise<ClipItem>;
  getItemContent(id: number, maxChars: number): Promise<ItemContent>;
  pasteItem(id: number, plain: boolean): Promise<PasteOutcome>;
  copyItem(id: number, plain: boolean): Promise<void>;
  setFavorite(id: number, value: boolean): Promise<ClipItem>;
  setPinned(id: number, value: boolean): Promise<ClipItem>;
  setNote(id: number, note: string | null): Promise<ClipItem>;
  deleteItems(ids: number[]): Promise<number>;
  clearHistory(keepMarked: boolean): Promise<number>;
  getStats(): Promise<Stats>;
  listSeenApps(): Promise<SeenApp[]>;
  getSettings(): Promise<Settings>;
  updateSettings(patch: Partial<Settings>): Promise<Settings>;
  getCapturePaused(): Promise<boolean>;
  setCapturePaused(paused: boolean): Promise<void>;
  hidePanel(restoreFocus: boolean): Promise<void>;
  setPanelLocked(locked: boolean): Promise<void>;
  openSettings(section: SettingsSection | null): Promise<void>;
  openUrl(url: string): Promise<void>;
  openPath(path: string): Promise<void>;
  revealPath(path: string): Promise<void>;
  openDataDir(): Promise<void>;
  getAppInfo(): Promise<AppInfo>;
  on<E extends keyof EventMap>(event: E, handler: (payload: EventMap[E]) => void): Promise<() => void>;
  /** URL for a file under the data dir (images, app icons). */
  assetUrl(path: string): string;
  minimizeWindow(): Promise<void>;
  closeWindow(): Promise<void>;
}

export const inTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

const ERROR_CODES: readonly ErrorCode[] = [
  'not_found',
  'invalid',
  'hotkey_invalid',
  'hotkey_conflict',
  'io',
  'db',
  'internal',
];

/** Whatever a rejected invoke throws, turn it into a CmdError. */
export function toCmdError(e: unknown): CmdError {
  if (e && typeof e === 'object' && 'code' in e) {
    const code = (e as { code: unknown }).code;
    const message = String((e as { message?: unknown }).message ?? '');
    if (ERROR_CODES.includes(code as ErrorCode)) return { code: code as ErrorCode, message };
  }
  return { code: 'internal', message: e instanceof Error ? e.message : String(e) };
}

async function createTauriBackend(): Promise<Backend> {
  const { listen } = await import('@tauri-apps/api/event');
  const { getCurrentWindow } = await import('@tauri-apps/api/window');

  const call = async <T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> => {
    try {
      return await invoke<T>(cmd, args);
    } catch (e) {
      throw toCmdError(e);
    }
  };

  return {
    listItems: (query) => call('list_items', { query }),
    getItem: (id) => call('get_item', { id }),
    getItemContent: (id, maxChars) => call('get_item_content', { id, maxChars }),
    pasteItem: (id, plain) => call('paste_item', { id, plain }),
    copyItem: (id, plain) => call('copy_item', { id, plain }),
    setFavorite: (id, value) => call('set_favorite', { id, value }),
    setPinned: (id, value) => call('set_pinned', { id, value }),
    setNote: (id, note) => call('set_note', { id, note }),
    deleteItems: (ids) => call('delete_items', { ids }),
    clearHistory: (keepMarked) => call('clear_history', { keepMarked }),
    getStats: () => call('get_stats'),
    listSeenApps: () => call('list_seen_apps'),
    getSettings: () => call('get_settings'),
    updateSettings: (patch) => call('update_settings', { patch }),
    getCapturePaused: () => call('get_capture_paused'),
    setCapturePaused: (paused) => call('set_capture_paused', { paused }),
    hidePanel: (restoreFocus) => call('hide_panel', { restoreFocus }),
    setPanelLocked: (locked) => call('set_panel_locked', { locked }),
    openSettings: (section) => call('open_settings', { section }),
    openUrl: (url) => call('open_url', { url }),
    openPath: (path) => call('open_path', { path }),
    revealPath: (path) => call('reveal_path', { path }),
    openDataDir: () => call('open_data_dir'),
    getAppInfo: () => call('get_app_info'),
    on: (event, handler) => listen(event, (e) => handler(e.payload as never)),
    assetUrl: (path) => convertFileSrc(path),
    minimizeWindow: () => getCurrentWindow().minimize(),
    closeWindow: () => getCurrentWindow().close(),
  };
}

let backend: Promise<Backend> | null = null;

export function getBackend(): Promise<Backend> {
  backend ??= inTauri
    ? createTauriBackend()
    : import('./mock').then((m) => m.createMockBackend());
  return backend;
}

/** Synchronous asset URL for files under the data dir; the mock hands out plain URLs. */
export function assetUrl(path: string): string {
  return inTauri ? convertFileSrc(path) : path;
}

/** Proxy so call sites can write `api.listItems(...)` without awaiting the backend first. */
export const api: Omit<Backend, 'assetUrl'> = new Proxy({} as Omit<Backend, 'assetUrl'>, {
  get(_target, prop: string) {
    return async (...args: unknown[]) => {
      const b = await getBackend();
      const fn = (b as unknown as Record<string, (...a: unknown[]) => unknown>)[prop];
      return fn.apply(b, args);
    };
  },
});
