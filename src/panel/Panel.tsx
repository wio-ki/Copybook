import {
  ClipboardPaste,
  Copy,
  ExternalLink,
  FolderOpen,
  Info,
  Lock,
  LockOpen,
  Pin,
  PinOff,
  Search,
  Settings2,
  Star,
  StarOff,
  Trash2,
  Type,
} from 'lucide-react';
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { api, toCmdError } from '../shared/api';
import { useApp } from '../shared/app';
import type { ClipItem, ListFilter, HistoryOptions } from '../shared/contract';
import { formatNumber } from '../shared/format';
import { hotkeyParts } from '../shared/hotkey';
import { Toast, useToast } from '../shared/Toast';
import { ClipRow } from './ClipRow';
import { ContextMenu, type MenuEntry } from './ContextMenu';
import { Detail } from './Detail';
import { EmptyState } from './EmptyState';
import { useHistory } from './useHistory';
import { useOrganizer } from './Organizer';
import { FavoriteFolders, folderParent } from './FavoriteFolders';

const FILTERS: ListFilter[] = ['all', 'text', 'image', 'files', 'url', 'favorite'];

interface MenuState {
  item: ClipItem;
  x: number;
  y: number;
}

export function Panel() {
  const { settings, t, lang } = useApp();
  const [query, setQuery] = useState('');
  const [filter, setFilter] = useState<ListFilter>('all');
  const [options, setOptions] = useState<HistoryOptions>({});
  const { items, total, status, refresh, loadMore, patch } = useHistory(query, filter, options);
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [detailId, setDetailId] = useState<number | null>(null);
  const [menu, setMenu] = useState<MenuState | null>(null);
  const [paused, setPaused] = useState(false);
  const [locked, setLocked] = useState(false);
  const [now, setNow] = useState(() => Date.now());
  const { toast, show: showToast, dismiss: dismissToast } = useToast();
  const changeFilter = (next: ListFilter) => {
    if (next === filter) return;
    setFilter(next);
    setOptions({ ...options, group: next === 'favorite' ? '' : null });
  };

  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const sentinelRef = useRef<HTMLDivElement>(null);

  const index = useMemo(() => {
    const i = items.findIndex((it) => it.id === selectedId);
    return i < 0 ? 0 : i;
  }, [items, selectedId]);
  const selected: ClipItem | undefined = items[index];
  const detail = detailId == null ? null : (items.find((i) => i.id === detailId) ?? null);

  const organizer = useOrganizer({ items, query, filter, options, setOptions, focusId: selected?.id ?? null, setFocus: setSelectedId, refresh, notify: showToast });
  const focusSearch = useCallback(() => inputRef.current?.focus(), []);
  const errorText = useCallback((e: unknown) => t.errors[toCmdError(e).code] ?? t.toast.failed, [t]);

  // ---- backend events -------------------------------------------------------------------

  useEffect(() => {
    const offs: Promise<() => void>[] = [];
    let pending: number | undefined;
    offs.push(
      api.on('history://changed', () => {
        window.clearTimeout(pending);
        pending = window.setTimeout(() => void refresh(), 50);
      }),
    );
    offs.push(
      api.on('panel://shown', ({ locked: isLocked }) => {
        setQuery('');
        setFilter('all');
        setOptions((previous) => ({ ...previous, group: null }));
        setSelectedId(null);
        setDetailId(null);
        setMenu(null);
        dismissToast();
        setLocked(isLocked);
        setNow(Date.now());
        listRef.current?.scrollTo({ top: 0 });
        void refresh();
        requestAnimationFrame(() => inputRef.current?.focus());
      }),
    );
    offs.push(api.on('capture://paused', ({ paused: p }) => setPaused(p)));
    void api.getCapturePaused().then(setPaused).catch(() => {});
    return () => {
      window.clearTimeout(pending);
      offs.forEach((p) => void p.then((off) => off()));
    };
  }, [refresh, dismissToast]);

  // Relative times stay honest while the panel is open.
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 30_000);
    return () => window.clearInterval(timer);
  }, []);

  // A new search starts at the top.
  useEffect(() => {
    setSelectedId(null);
    listRef.current?.scrollTo({ top: 0 });
  }, [query, filter, options.group]);

  // Keep the selection visible during keyboard travel.
  useLayoutEffect(() => {
    const el = listRef.current?.querySelector<HTMLElement>(`[data-index="${index}"]`);
    el?.scrollIntoView({ block: 'nearest' });
  }, [index]);

  // Infinite scroll.
  useEffect(() => {
    const root = listRef.current;
    const target = sentinelRef.current;
    if (!root || !target) return;
    const io = new IntersectionObserver((entries) => entries.some((e) => e.isIntersecting) && loadMore(), {
      root,
      rootMargin: '240px',
    });
    io.observe(target);
    return () => io.disconnect();
  }, [loadMore, items.length]);

  // ---- actions --------------------------------------------------------------------------

  const paste = useCallback(
    async (item: ClipItem, invert = false) => {
      const plain = item.kind !== 'image' && settings.pastePlain !== invert;
      try {
        const out = await api.pasteItem(item.id, plain);
        if (!out.pasted && out.reason === 'elevated_target') showToast(t.toast.elevated, true);
      } catch (e) {
        showToast(errorText(e));
      }
    },
    [settings.pastePlain, showToast, t, errorText],
  );

  const copy = useCallback(
    async (item: ClipItem) => {
      try {
        await api.copyItem(item.id, false);
        showToast(t.toast.copied);
      } catch (e) {
        showToast(errorText(e));
      }
    },
    [showToast, t, errorText],
  );

  const toggleFavorite = useCallback(
    async (item: ClipItem) => {
      const value = !item.favorite;
      patch(item.id, { favorite: value });
      try {
        await api.setFavorite(item.id, value);
        showToast(value ? t.toast.favorited : t.toast.unfavorited);
      } catch (e) {
        patch(item.id, { favorite: !value });
        showToast(errorText(e));
      }
    },
    [patch, showToast, t, errorText],
  );

  const togglePin = useCallback(
    async (item: ClipItem) => {
      const value = !item.pinned;
      try {
        await api.setPinned(item.id, value);
        showToast(value ? t.toast.pinned : t.toast.unpinned);
      } catch (e) {
        showToast(errorText(e));
      }
    },
    [showToast, t, errorText],
  );

  const remove = (item: ClipItem) => {
    setDetailId(null);
    organizer.requestDelete([item.id], item.favorite || item.pinned);
  };

  const saveNote = useCallback(
    async (item: ClipItem, note: string | null) => {
      patch(item.id, { note });
      try {
        await api.setNote(item.id, note);
        showToast(t.toast.noteSaved);
      } catch (e) {
        patch(item.id, { note: item.note });
        showToast(errorText(e));
      }
    },
    [patch, showToast, t, errorText],
  );

  const toggleLock = useCallback(() => {
    const next = !locked;
    setLocked(next);
    void api.setPanelLocked(next).catch(() => setLocked(!next));
  }, [locked]);

  const [clearing, setClearing] = useState(false);
  const [keepMarked, setKeepMarked] = useState(true);
  const [toDelete, setToDelete] = useState<number | null>(null);
  const [clearBusy, setClearBusy] = useState(false);

  const openClear = useCallback(() => {
    setClearing(true);
    setToDelete(null);
    api
      .getStats()
      .then((s) => setToDelete(keepMarked ? s.total - s.marked : s.total))
      .catch(() => setToDelete(null));
  }, [keepMarked]);

  const toggleKeepMarked = useCallback(() => {
    const next = !keepMarked;
    setKeepMarked(next);
    setToDelete(null);
    api
      .getStats()
      .then((s) => setToDelete(next ? s.total - s.marked : s.total))
      .catch(() => setToDelete(null));
  }, [keepMarked]);

  const runClear = useCallback(async () => {
    if (clearBusy) return;
    setClearBusy(true);
    try {
      const count = await api.clearHistory(keepMarked);
      setClearing(false);
      setDetailId(null);
      setSelectedId(null);
      showToast(t.toast.cleared(formatNumber(count, lang)));
      void refresh();
    } catch (e) {
      showToast(errorText(e));
    } finally {
      setClearBusy(false);
    }
  }, [clearBusy, keepMarked, showToast, t, lang, refresh, errorText]);

  const clearRef = useRef<HTMLDivElement>(null);

  // A click anywhere else, or Escape, puts the confirmation away.
  useEffect(() => {
    if (!clearing) return;
    const onDown = (e: PointerEvent) => {
      if (!clearRef.current?.contains(e.target as Node)) setClearing(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        e.stopPropagation();
        setClearing(false);
        focusSearch();
      }
    };
    window.addEventListener('pointerdown', onDown, true);
    window.addEventListener('keydown', onKey, true);
    return () => {
      window.removeEventListener('pointerdown', onDown, true);
      window.removeEventListener('keydown', onKey, true);
    };
  }, [clearing, focusSearch]);

  const hide = useCallback(() => void api.hidePanel(true), []);

  // ---- keyboard -------------------------------------------------------------------------

  const move = useCallback(
    (delta: number) => {
      if (!items.length) return;
      const next = Math.max(0, Math.min(items.length - 1, index + delta));
      setSelectedId(items[next].id);
      if (next >= items.length - 8) loadMore();
    },
    [items, index, loadMore],
  );

  const pageSize = () => {
    const list = listRef.current;
    const row = list?.querySelector<HTMLElement>('.row');
    return list && row ? Math.max(1, Math.floor(list.clientHeight / row.offsetHeight) - 1) : 5;
  };

  const onKey = useRef<(e: KeyboardEvent) => void>(() => {});
  onKey.current = (e: KeyboardEvent) => {
    if (e.isComposing || e.keyCode === 229 || menu || clearing) return;
    if ((e.target as HTMLElement | null)?.closest('.favorite-folders')) return;
    if (organizer.onKey(e)) return;
    if ((e.target as HTMLElement | null)?.closest('.organizer-tools, .selection-bar, .advanced-filters, .undo-bar')) return;
    const mod = e.ctrlKey || e.metaKey;
    const target = e.target as HTMLElement | null;
    const inTextarea = target?.tagName === 'TEXTAREA';
    const item = detail ?? selected;

    if (inTextarea) {
      if (e.key === 'Escape') {
        e.preventDefault();
        target?.blur();
      }
      return;
    }

    if (e.key === 'Escape') {
      e.preventDefault();
      if (detail) {
        setDetailId(null);
        focusSearch();
      } else if (query) setQuery('');
      else if (filter === 'favorite' && options.group) setOptions({ ...options, group: folderParent(options.group) });
      else hide();
      return;
    }

    if (mod && e.key === ',') {
      e.preventDefault();
      void api.openSettings(null);
      return;
    }

    if (e.key === 'Tab' && !detail) {
      e.preventDefault();
      const at = FILTERS.indexOf(filter);
      changeFilter(FILTERS[(at + (e.shiftKey ? -1 : 1) + FILTERS.length) % FILTERS.length]);
      return;
    }

    if (!item) return;

    if (e.key === 'Enter') {
      e.preventDefault();
      if (e.altKey && !detail) setDetailId(item.id);
      else void paste(item, e.shiftKey);
      return;
    }

    // Ctrl+1..9 or Ctrl+Shift+1..9: some apps hold Ctrl+1..3 as global hotkeys and eat them.
    const digit =
      /^Digit([1-9])$/.exec(e.code)?.[1] ?? (!e.shiftKey && /^[1-9]$/.test(e.key) ? e.key : null);
    if (mod && digit && !detail) {
      e.preventDefault();
      const nth = items[Number(digit) - 1];
      if (nth) void paste(nth, e.shiftKey);
      return;
    }

    if (mod && !e.shiftKey && !e.altKey) {
      const key = e.key.toLowerCase();
      if (key === 'c') {
        const input = inputRef.current;
        const hasSelection =
          (document.activeElement === input && input && input.selectionStart !== input.selectionEnd) ||
          !!window.getSelection()?.toString();
        if (hasSelection) return;
        e.preventDefault();
        void copy(item);
        return;
      }
      if (key === 's') {
        e.preventDefault();
        void toggleFavorite(item);
        return;
      }
      if (key === 'p') {
        e.preventDefault();
        void togglePin(item);
        return;
      }
      if (e.key === 'Delete') {
        e.preventDefault();
        void remove(item);
        return;
      }
    }

    if (detail) return;

    switch (e.key) {
      case 'ArrowDown':
        e.preventDefault();
        move(1);
        break;
      case 'ArrowUp':
        e.preventDefault();
        move(-1);
        break;
      case 'PageDown':
        e.preventDefault();
        move(pageSize());
        break;
      case 'PageUp':
        e.preventDefault();
        move(-pageSize());
        break;
      default:
        if (mod && e.key === 'Home') {
          e.preventDefault();
          move(-items.length);
        } else if (mod && e.key === 'End') {
          e.preventDefault();
          move(items.length);
        }
    }
  };

  useEffect(() => {
    const handler = (e: KeyboardEvent) => onKey.current(e);
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, []);

  // ---- menu -----------------------------------------------------------------------------

  const menuEntries = (item: ClipItem): MenuEntry[] => {
    const plainFirst = settings.pastePlain && item.kind !== 'image';
    const entries: MenuEntry[] = [
      {
        id: 'paste',
        label: t.actions.paste,
        icon: ClipboardPaste,
        keys: [plainFirst ? '⇧↵' : '↵'],
        run: () => void paste(item, plainFirst),
      },
    ];
    if ((item.kind === 'text' && (item.hasHtml || item.hasRtf)) || item.kind === 'files') {
      entries.push({
        id: 'plain',
        label: item.kind === 'files' ? t.actions.pastePaths : t.actions.pastePlain,
        icon: Type,
        keys: [plainFirst ? '↵' : '⇧↵'],
        run: () => void paste(item, !plainFirst),
      });
    }
    entries.push({ id: 'copy', label: t.actions.copy, icon: Copy, keys: ['Ctrl+C'], run: () => void copy(item) });
    entries.push('separator');
    entries.push({
      id: 'fav',
      label: item.favorite ? t.actions.unfavorite : t.actions.favorite,
      icon: item.favorite ? StarOff : Star,
      keys: ['Ctrl+S'],
      run: () => void toggleFavorite(item),
    });
    entries.push({
      id: 'pin',
      label: item.pinned ? t.actions.unpin : t.actions.pin,
      icon: item.pinned ? PinOff : Pin,
      keys: ['Ctrl+P'],
      run: () => void togglePin(item),
    });
    entries.push({ id: 'group', label: lang === 'zh-CN' ? '移至收藏文件夹' : 'Move to favorite folder', icon: FolderOpen, run: () => organizer.open('group', [item.id]) });
    entries.push({ id: 'detail', label: t.actions.detail, icon: Info, keys: ['Alt+↵'], run: () => setDetailId(item.id) });
    if (item.subKind === 'url') {
      const url = /^[a-z][a-z0-9+.-]*:/i.test(item.text.trim()) ? item.text.trim() : `https://${item.text.trim()}`;
      entries.push({ id: 'open', label: t.actions.openLink, icon: ExternalLink, run: () => void api.openUrl(url) });
    }
    const revealTarget =
      item.subKind === 'path'
        ? item.text.trim().replace(/^"(.*)"$/, '$1')
        : item.kind === 'files'
          ? item.files?.find((f) => f.exists)?.path
          : undefined;
    if (revealTarget) {
      entries.push({ id: 'reveal', label: t.actions.reveal, icon: FolderOpen, run: () => void api.revealPath(revealTarget) });
    }
    entries.push('separator');
    entries.push({
      id: 'delete',
      label: t.actions.delete,
      icon: Trash2,
      keys: ['Ctrl+Del'],
      danger: true,
      run: () => void remove(item),
    });
    return entries;
  };

  const openMenu = useCallback((item: ClipItem, x: number, y: number) => setMenu({ item, x, y }), []);
  const closeMenu = useCallback(() => {
    setMenu(null);
    focusSearch();
  }, [focusSearch]);
  const selectRow = (id: number, e?: React.MouseEvent) => organizer.select(id, e);
  const pasteRow = (item: ClipItem) => { if (!organizer.mode) void paste(item); };
  const favoriteRow = useCallback((item: ClipItem) => void toggleFavorite(item), [toggleFavorite]);

  // ---- render ---------------------------------------------------------------------------

  const hotkey = hotkeyParts(settings.hotkey);
  const plainDefault = settings.pastePlain;

  let empty: React.ReactNode = null;
  if (status === 'error' && !items.length) {
    empty = <EmptyState title={t.panel.loadError} action={{ label: t.panel.retry, run: () => void refresh() }} />;
  } else if (status === 'ready' && !items.length) {
    if (query.trim()) empty = <EmptyState title={t.panel.noResultTitle} body={t.panel.noResultBody(query.trim())} />;
    else if (filter !== 'all') empty = <EmptyState title={t.panel.noResultTitle} body={t.panel.noResultFilter} />;
    else
      empty = (
        <EmptyState
          stroke
          title={t.panel.emptyTitle}
          body={t.panel.emptyBody}
          footnote={
            hotkey.length ? (
              <>
                {t.panel.emptyHotkeyLead}
                {hotkey.map((k) => (
                  <kbd key={k} className="kbd">
                    {k}
                  </kbd>
                ))}
                {t.panel.emptyHotkeyTail}
              </>
            ) : null
          }
        />
      );
  }

  return (
    <div className={`panel density-${settings.density}`}>
      <header className="panel-head" data-tauri-drag-region>
        <label className="search" data-tauri-drag-region>
          <Search size={16} strokeWidth={1.75} aria-hidden="true" className="search-icon" />
          <input
            ref={inputRef}
            type="text"
            role="combobox"
            aria-expanded="true"
            aria-controls="clip-list"
            aria-activedescendant={selected ? `clip-${selected.id}` : undefined}
            aria-label={t.panel.search}
            placeholder={t.panel.search}
            value={query}
            spellCheck={false}
            autoComplete="off"
            autoFocus
            onChange={(e) => setQuery(e.target.value)}
          />
        </label>
        <div className="head-tools">
          <div className="clear-wrap" ref={clearRef}>
            <button
              type="button"
              className={`icon-btn icon-btn--danger${clearing ? ' is-on-ink' : ''}`}
              title={t.panel.clear}
              aria-label={t.panel.clear}
              aria-expanded={clearing}
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => (clearing ? setClearing(false) : openClear())}
            >
              <Trash2 size={15} strokeWidth={1.75} />
            </button>
            {clearing && (
              <div className="clear-pop" role="dialog" aria-label={t.panel.clear}>
                <p className="clear-title">{t.panel.clear}</p>
                <label className="clear-keep">
                  <input type="checkbox" checked={keepMarked} onChange={toggleKeepMarked} />
                  <span>{t.panel.clearKeep}</span>
                </label>
                <p className="clear-text">
                  {toDelete == null ? '…' : t.panel.clearConfirm(formatNumber(toDelete, lang))}
                </p>
                <div className="clear-actions">
                  <button type="button" className="btn btn--quiet" onClick={() => setClearing(false)}>
                    {t.panel.cancel}
                  </button>
                  <button
                    type="button"
                    className="btn btn--danger"
                    onClick={() => void runClear()}
                    disabled={clearBusy || toDelete === 0}
                  >
                    {t.panel.clearAction}
                  </button>
                </div>
              </div>
            )}
          </div>
          <button
            type="button"
            className={`icon-btn${locked ? ' is-on-ink' : ''}`}
            title={locked ? t.panel.unlock : t.panel.lock}
            aria-label={locked ? t.panel.unlock : t.panel.lock}
            aria-pressed={locked}
            onMouseDown={(e) => e.preventDefault()}
            onClick={toggleLock}
          >
            {locked ? <Lock size={15} strokeWidth={1.75} /> : <LockOpen size={15} strokeWidth={1.75} />}
          </button>
          <button
            type="button"
            className="icon-btn"
            title={`${t.panel.settings} (Ctrl+,)`}
            aria-label={t.panel.settings}
            onMouseDown={(e) => e.preventDefault()}
            onClick={() => void api.openSettings(null)}
          >
            <Settings2 size={15} strokeWidth={1.75} />
          </button>
        </div>
      </header>

      <nav className="tabs" role="tablist" aria-label={t.panel.search} data-tauri-drag-region>
        {FILTERS.map((f) => (
          <button
            key={f}
            type="button"
            role="tab"
            aria-selected={filter === f}
            className={`tab${filter === f ? ' is-active' : ''}`}
            onMouseDown={(e) => e.preventDefault()}
            onClick={() => changeFilter(f)}
          >
            {t.filters[f]}
          </button>
        ))}
      </nav>

      {organizer.controls}
      <div className="panel-body">
        <div ref={listRef} className="list">
          {filter === 'favorite' && <FavoriteFolders groups={organizer.groups} current={options.group ?? null}
            navigate={(group) => { setOptions({ ...options, group }); setDetailId(null); setMenu(null); }}
            notify={showToast} refresh={refresh} />}
          <div id="clip-list" role="listbox" tabIndex={0} aria-multiselectable={organizer.mode} aria-label={t.filters[filter]}>
          {items.map((item, i) => (
            <ClipRow
              key={item.id}
              item={item}
              index={i}
              selected={organizer.mode ? organizer.ids.includes(item.id) : i === index}
              multi={organizer.mode}
              focused={i === index}
              density={settings.density}
              showSource={settings.showSource}
              t={t}
              lang={lang}
              now={now}
              onSelect={selectRow}
              onPaste={pasteRow}
              onMenu={openMenu}
              onToggleFavorite={favoriteRow}
            />
          ))}
          {items.length > 0 && items.length < total && <div ref={sentinelRef} className="sentinel" />}
          {!(status === 'ready' && filter === 'favorite' && !query.trim() && organizer.groups.some((g) => folderParent(g) === options.group)) && empty}
          </div>
        </div>

        {detail && (
          <Detail
            key={detail.id}
            item={detail}
            t={t}
            lang={lang}
            pastePlain={plainDefault}
            showSource={settings.showSource}
            onClose={() => {
              setDetailId(null);
              focusSearch();
            }}
            onPaste={(item, invert) => void paste(item, invert)}
            onCopy={(item) => void copy(item)}
            onToggleFavorite={(item) => void toggleFavorite(item)}
            onTogglePin={(item) => void togglePin(item)}
            onDelete={(item) => void remove(item)}
            onSaveNote={saveNote}
          />
        )}

        <Toast toast={toast} onDismiss={dismissToast} />
      </div>

      <footer className="panel-foot" data-tauri-drag-region>
        {organizer.mode ? <span className="hints"><kbd className="kbd">Ctrl+A</kbd><span>{lang==='zh-CN'?'全选':'All'}</span><kbd className="kbd">Ctrl+Del</kbd><span>{t.actions.delete}</span><kbd className="kbd">Esc</kbd><span>{lang==='zh-CN'?'退出':'Exit'}</span></span> : <span className="hints" data-tauri-drag-region>
          <kbd className="kbd">↵</kbd>
          <span>{plainDefault ? t.panel.hintPlain : t.panel.hintPaste}</span>
          {detail ? (
            <>
              <kbd className="kbd">Esc</kbd>
              <span>{t.panel.hintBack}</span>
            </>
          ) : (
            <>
              <kbd className="kbd">⇧↵</kbd>
              <span>{plainDefault ? t.panel.hintRich : t.panel.hintPlain}</span>
              <kbd className="kbd">Alt↵</kbd>
              <span>{t.panel.hintDetail}</span>
            </>
          )}
        </span>}
        {paused ? (
          <button type="button" className="paused" onClick={() => void api.setCapturePaused(false)} title={t.panel.resume}>
            <span className="paused-dot" aria-hidden="true" />
            {t.panel.paused}
            <span className="paused-action">{t.panel.resume}</span>
          </button>
        ) : (
          <span className="count tnum" data-tauri-drag-region>
            {t.panel.count(formatNumber(total, lang))}
          </span>
        )}
      </footer>

      {organizer.dialogs}
      {menu && <ContextMenu x={menu.x} y={menu.y} entries={menuEntries(menu.item)} onClose={closeMenu} />}
    </div>
  );
}
