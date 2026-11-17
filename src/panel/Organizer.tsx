import { CheckSquare, Filter, FolderPlus, Undo2, X } from 'lucide-react';
import { useCallback, useEffect, useRef, useState, type MouseEvent } from 'react';
import { api, toCmdError } from '../shared/api';
import { useApp } from '../shared/app';
import type { ClipItem, HistoryOptions, ListFilter, SeenApp } from '../shared/contract';
import { FeatureDialog } from '../shared/FeatureDialog';

interface Props {
  items: ClipItem[];
  query: string;
  filter: ListFilter;
  options: HistoryOptions;
  setOptions: (value: HistoryOptions) => void;
  focusId: number | null;
  setFocus: (id: number) => void;
  refresh: () => Promise<void>;
  notify: (text: string) => void;
}
export function useOrganizer(p: Props) {
  const { lang, t } = useApp();
  const zh = lang === 'zh-CN';
  const [mode, setMode] = useState(false);
  const [ids, setIds] = useState<number[]>([]);
  const [groups, setGroups] = useState<string[]>([]);
  const [sources, setSources] = useState<SeenApp[]>([]);
  const [filters, setFilters] = useState(false);
  const [dialog, setDialog] = useState<'delete' | 'merge' | 'group' | 'manage' | null>(null);
  const [targets, setTargets] = useState<number[]>([]);
  const [busy, setBusy] = useState(false);
  const [separator, setSeparator] = useState('\n');
  const [reverse, setReverse] = useState(false);
  const [preview, setPreview] = useState<string | null>(null);
  const [mergeError, setMergeError] = useState(false);
  const [draft, setDraft] = useState('');
  const [rename, setRename] = useState('');
  const [undoUntil, setUndoUntil] = useState(0);
  const [undoCount, setUndoCount] = useState(0);
  const anchor = useRef<number | null>(null);
  const generation = useRef(0);
  const optionsKey = JSON.stringify(p.options);
  const error = useCallback((e: unknown) => p.notify(t.errors[toCmdError(e).code]), [p.notify, t]);
  const reset = useCallback(() => {
    setMode(false);
    setIds([]);
    anchor.current = null;
  }, []);
  useEffect(() => {
    generation.current++;
    reset();
    setDialog(null);
  }, [p.query, p.filter, optionsKey, reset]);
  useEffect(() => {
    let alive = true;
    const load = () => {
      void api
        .listGroups()
        .then((g) => {
          if (alive) setGroups(g);
        })
        .catch(error);
      void api
        .listSeenApps()
        .then((a) => {
          if (alive) setSources(a);
        })
        .catch(error);
    };
    load();
    const off = api.on('history://changed', load);
    const shown = api.on('panel://shown', () => {
      reset();
      setDialog(null);
    });
    return () => {
      alive = false;
      void off.then((f) => f());
      void shown.then((f) => f());
    };
  }, [error, reset]);
  useEffect(() => {
    if (!undoUntil) return;
    const timer = window.setTimeout(() => setUndoUntil(0), Math.max(0, undoUntil - Date.now()));
    return () => window.clearTimeout(timer);
  }, [undoUntil]);
  useEffect(() => {
    setPreview(null);
    setMergeError(false);
    if (dialog !== 'merge') return;
    let alive = true;
    const timer = window.setTimeout(
      () =>
        void api
          .getMergedText(reverse ? [...targets].reverse() : targets, separator)
          .then((v) => {
            if (alive) setPreview(v);
          })
          .catch((e) => {
            if (alive) {
              setMergeError(true);
              error(e);
            }
          }),
      120,
    );
    return () => {
      alive = false;
      window.clearTimeout(timer);
    };
  }, [dialog, targets, separator, reverse, error]);
  const selectAll = async () => {
    if (busy) return;
    setBusy(true);
    const mine = generation.current;
    try {
      const all = await api.listItemIds({
        query: p.query,
        filter: p.filter,
        offset: 0,
        limit: 200,
        ...p.options,
      });
      if (mine === generation.current) {
        setIds(all);
        setMode(true);
      }
    } catch (e) {
      error(e);
    } finally {
      setBusy(false);
    }
  };
  const select = (id: number, event?: MouseEvent) => {
    p.setFocus(id);
    if (!(mode || event?.ctrlKey || event?.shiftKey)) {
      anchor.current = id;
      return;
    }
    setMode(true);
    if (event?.shiftKey && anchor.current != null) {
      const from = p.items.findIndex((i) => i.id === anchor.current);
      const to = p.items.findIndex((i) => i.id === id);
      if (from >= 0 && to >= 0) {
        const range = p.items.slice(Math.min(from, to), Math.max(from, to) + 1).map((i) => i.id);
        setIds((current) => [...new Set([...current, ...range])]);
        return;
      }
    }
    anchor.current = id;
    setIds((current) => (current.includes(id) ? current.filter((i) => i !== id) : [...current, id]));
  };
  const remove = async (targets: number[]) => {
    if (busy) return;
    setBusy(true);
    try {
      const n = await api.deleteItems(targets);
      if (n) {
        setUndoCount(n);
        setUndoUntil(Date.now() + 15000);
      }
      setDialog(null);
      reset();
      await p.refresh();
    } catch (e) {
      error(e);
    } finally {
      setBusy(false);
    }
  };
  const requestDelete = (values: number[], confirm = true) => {
    if (!values.length) return;
    if (confirm) {
      setTargets(values);
      setDialog('delete');
    } else void remove(values);
  };
  const undo = async () => {
    if (busy) return;
    setBusy(true);
    try {
      const n = await api.undoDelete();
      setUndoUntil(0);
      p.notify(
        n ? (zh ? `已恢复 ${n} 条记录` : `Restored ${n} items`) : zh ? '撤销时间已过' : 'Undo has expired',
      );
      await p.refresh();
    } catch (e) {
      error(e);
    } finally {
      setBusy(false);
    }
  };
  const open = (action: 'merge' | 'group', values = ids) => {
    if (!values.length) return;
    setTargets(values);
    setPreview(null);
    setReverse(false);
    setDraft('');
    setDialog(action);
  };
  const onKey = (e: KeyboardEvent) => {
    if (dialog) return true;
    const target = e.target as HTMLElement | null;
    const editing =
      target?.tagName === 'TEXTAREA' ||
      target?.tagName === 'SELECT' ||
      (target?.tagName === 'INPUT' && !target.closest('.search'));
    if (editing) return true;
    if (
      e.ctrlKey &&
      e.key.toLowerCase() === 'z' &&
      undoUntil &&
      !['INPUT', 'TEXTAREA'].includes(target?.tagName ?? '')
    ) {
      e.preventDefault();
      void undo();
      return true;
    }
    if (mode && e.key === 'Escape') {
      e.preventDefault();
      reset();
      return true;
    }
    if (e.ctrlKey && e.key.toLowerCase() === 'a' && (mode || target?.tagName !== 'INPUT')) {
      e.preventDefault();
      void selectAll();
      return true;
    }
    if (e.shiftKey && ['ArrowDown', 'ArrowUp'].includes(e.key)) {
      const at = Math.max(
        0,
        p.items.findIndex((i) => i.id === p.focusId),
      );
      const next = p.items[at + (e.key === 'ArrowDown' ? 1 : -1)];
      if (next) {
        e.preventDefault();
        setMode(true);
        if (anchor.current == null) anchor.current = p.items[at].id;
        const from = p.items.findIndex((i) => i.id === anchor.current);
        setIds(
          p.items
            .slice(
              Math.min(from, at + (e.key === 'ArrowDown' ? 1 : -1)),
              Math.max(from, at + (e.key === 'ArrowDown' ? 1 : -1)) + 1,
            )
            .map((i) => i.id),
        );
        p.setFocus(next.id);
      }
      return true;
    }
    if (!mode) return false;
    if (e.code === 'Space' && target?.tagName !== 'INPUT') {
      e.preventDefault();
      if (p.focusId) select(p.focusId);
      return true;
    }
    if (e.ctrlKey && e.key === 'Delete') {
      e.preventDefault();
      requestDelete(ids);
      return true;
    }
    if (e.ctrlKey && e.key.toLowerCase() === 'c') {
      const input = target as HTMLInputElement | null;
      if (target?.tagName === 'INPUT' && input?.selectionStart !== input?.selectionEnd) return false;
      e.preventDefault();
      open('merge');
      return true;
    }
    if (e.key === 'Enter') {
      e.preventDefault();
      open('merge');
      return true;
    }
    if (e.ctrlKey && (/^[1-9]$/.test(e.key) || ['s', 'p'].includes(e.key.toLowerCase()))) {
      e.preventDefault();
      return true;
    }
    return false;
  };
  const button = (label: string, run: () => void, disabled = false) => (
    <button type="button" className="btn btn--quiet" onClick={run} disabled={busy || disabled}>
      {label}
    </button>
  );
  const filterCount = Object.values(p.options).filter((v) => v != null && v !== '').length;
  const controls = (
    <>
      <div className="organizer-tools">
        <button
          type="button"
          className={`btn btn--quiet${mode ? ' is-pressed' : ''}`}
          onClick={() => {
            if (mode) reset();
            else {
              setMode(true);
              document.getElementById('clip-list')?.focus();
            }
          }}
        >
          <CheckSquare size={14} />
          {zh ? '多选' : 'Select'}
        </button>
        <button
          type="button"
          className="btn btn--quiet"
          aria-expanded={filters}
          onClick={() => setFilters(!filters)}
        >
          <Filter size={14} />
          {zh ? '筛选' : 'Filter'}
          {filterCount ? ` (${filterCount})` : ''}
        </button>
        <button
          type="button"
          className="btn btn--quiet"
          onClick={() => {
            setDraft('');
            setRename('');
            setDialog('manage');
          }}
        >
          <FolderPlus size={14} />
          {zh ? '分组' : 'Groups'}
        </button>
        {filterCount > 0 && button(zh ? '重置' : 'Reset', () => p.setOptions({}))}
      </div>
      {filters && (
        <div className="advanced-filters">
          <label>
            {zh ? '来源应用' : 'Source app'}
            <select
              aria-label={zh ? '来源应用' : 'Source app'}
              value={p.options.sourceExe ?? ''}
              onChange={(e) => p.setOptions({ ...p.options, sourceExe: e.target.value || null })}
            >
              <option value="">{zh ? '全部应用' : 'All apps'}</option>
              {sources.map((s) => (
                <option key={s.exe} value={s.exe}>
                  {s.name}
                </option>
              ))}
            </select>
          </label>
          <label>
            {zh ? '收藏分组' : 'Favorite group'}
            <select
              aria-label={zh ? '收藏分组' : 'Favorite group'}
              value={p.options.group == null ? 'all' : `group:${p.options.group}`}
              onChange={(e) =>
                p.setOptions({
                  ...p.options,
                  group: e.target.value === 'all' ? null : e.target.value.slice(6),
                })
              }
            >
              <option value="all">{zh ? '全部分组' : 'All groups'}</option>
              <option value="group:">{zh ? '未分组收藏' : 'Ungrouped favorites'}</option>
              {groups.map((g) => (
                <option key={g} value={`group:${g}`}>
                  {g}
                </option>
              ))}
            </select>
          </label>
          <label>
            {zh ? '起始日期' : 'From date'}
            <input
              type="date"
              aria-label={zh ? '起始日期' : 'From date'}
              onChange={(e) =>
                p.setOptions({
                  ...p.options,
                  since: e.target.value ? new Date(e.target.value + 'T00:00:00').getTime() : null,
                })
              }
              value={p.options.since == null ? '' : localDate(p.options.since)}
            />
          </label>
          <label>
            {zh ? '结束日期' : 'Through date'}
            <input
              type="date"
              aria-label={zh ? '结束日期' : 'Through date'}
              onChange={(e) => {
                const d = e.target.value ? new Date(e.target.value + 'T00:00:00') : null;
                d?.setDate(d.getDate() + 1);
                p.setOptions({ ...p.options, until: d?.getTime() ?? null });
              }}
              value={p.options.until == null ? '' : localDate(p.options.until - 1)}
            />
          </label>
          <p>{zh ? '按最近使用时间筛选' : 'Filters use last-used time'}</p>
        </div>
      )}
      {mode && (
        <div className="selection-bar">
          <span role="status">{zh ? `已选 ${ids.length} 条` : `${ids.length} selected`}</span>
          {button(zh ? '全选结果' : 'Select results', () => void selectAll())}
          {button(zh ? '删除' : 'Delete', () => requestDelete(ids), !ids.length)}
          {button(zh ? '合并' : 'Merge', () => open('merge'), !ids.length || ids.length > 1000)}
          {button(zh ? '分组' : 'Group', () => open('group'), !ids.length)}
          <button className="icon-btn" aria-label={zh ? '退出多选' : 'Exit selection'} onClick={reset}>
            <X size={14} />
          </button>
        </div>
      )}
      {undoUntil > 0 && (
        <div className="undo-bar" role="status">
          <span>
            {zh ? `已删除 ${undoCount} 条 · 15 秒内可撤销` : `Deleted ${undoCount} · undo within 15s`}
          </span>
          <button className="btn btn--quiet" onClick={() => void undo()} disabled={busy}>
            <Undo2 size={14} />
            {zh ? '撤销' : 'Undo'}
          </button>
        </div>
      )}
    </>
  );
  const dialogs = dialog && (
    <FeatureDialog
      title={
        dialog === 'delete'
          ? zh
            ? '删除所选记录'
            : 'Delete selected items'
          : dialog === 'merge'
            ? zh
              ? '合并复制'
              : 'Merge and copy'
            : dialog === 'manage'
              ? zh
                ? '管理收藏分组'
                : 'Manage favorite groups'
              : zh
                ? '设置收藏分组'
                : 'Set favorite group'
      }
      onClose={() => {
        if (!busy) setDialog(null);
      }}
    >
      {dialog === 'delete' && (
        <>
          <p>
            {zh
              ? `确定删除 ${targets.length} 条记录？包含的收藏和置顶也会删除。`
              : `Delete ${targets.length} items? Any selected favorites and pins will also be deleted.`}
          </p>
          <p>{zh ? '删除后 15 秒内可撤销。' : 'You can undo for 15 seconds.'}</p>
          <div className="feature-actions">
            {button(zh ? '取消' : 'Cancel', () => setDialog(null))}
            <button className="btn btn--danger" disabled={busy} onClick={() => void remove(targets)}>
              {zh ? '删除' : 'Delete'}
            </button>
          </div>
        </>
      )}
      {dialog === 'merge' && (
        <>
          <label>
            {zh ? '分隔符' : 'Separator'}
            <select
              value={['\n', ' ', '\n\n', ', '].includes(separator) ? separator : 'custom'}
              onChange={(e) => setSeparator(e.target.value === 'custom' ? '' : e.target.value)}
            >
              <option value={'\n'}>{zh ? '换行' : 'Newline'}</option>
              <option value=" ">{zh ? '空格' : 'Space'}</option>
              <option value={'\n\n'}>{zh ? '空一行' : 'Blank line'}</option>
              <option value=", ">{zh ? '逗号' : 'Comma'}</option>
              <option value="custom">{zh ? '自定义' : 'Custom'}</option>
            </select>
          </label>
          <input
            aria-label={zh ? '自定义分隔符' : 'Custom separator'}
            maxLength={100}
            value={separator}
            onChange={(e) => setSeparator(e.target.value)}
          />
          <label className="feature-check">
            <input type="checkbox" checked={reverse} onChange={(e) => setReverse(e.target.checked)} />
            {zh ? '反转所选顺序' : 'Reverse selection order'}
          </label>
          <p>
            {zh
              ? `${targets.length} 条文字，按选择顺序合并；全选时使用列表顺序。`
              : `${targets.length} text items, in selection order; select-all uses list order.`}
          </p>
          <textarea
            readOnly
            aria-label={zh ? '合并预览' : 'Merge preview'}
            value={
              preview ??
              (mergeError
                ? zh
                  ? '仅支持合并最多 1000 条文字记录，总大小不超过 5 MB。请调整选择后重试。'
                  : 'Select up to 1000 text items, totaling at most 5 MB. Adjust the selection and try again.'
                : zh
                  ? '正在加载完整文字…'
                  : 'Loading full text…')
            }
          />
          <div className="feature-actions">
            {button(zh ? '取消' : 'Cancel', () => setDialog(null))}
            <button
              className="btn btn--primary"
              disabled={busy || preview == null}
              onClick={() => {
                setBusy(true);
                void api
                  .copyMerged(reverse ? [...targets].reverse() : targets, separator)
                  .then(() => {
                    setDialog(null);
                    p.notify(zh ? '已复制合并文字' : 'Merged text copied');
                  })
                  .catch(error)
                  .finally(() => setBusy(false));
              }}
            >
              {zh ? '复制' : 'Copy'}
            </button>
          </div>
        </>
      )}
      {dialog === 'group' && (
        <>
          <label>
            {zh ? '分组名称（留空移出分组）' : 'Group name (empty to ungroup)'}
            <input
              list="favorite-groups"
              maxLength={40}
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
            />
          </label>
          <datalist id="favorite-groups">
            {groups.map((g) => (
              <option key={g} value={g} />
            ))}
          </datalist>
          <p>{zh ? '添加到分组会同时收藏所选记录。' : 'Grouping also favorites the selected items.'}</p>
          <div className="feature-actions">
            {button(zh ? '取消' : 'Cancel', () => setDialog(null))}
            <button
              className="btn btn--primary"
              disabled={busy}
              onClick={() => {
                setBusy(true);
                void api
                  .setItemGroup(targets, draft.trim() || null)
                  .then(() => {
                    setDialog(null);
                    reset();
                  })
                  .catch(error)
                  .finally(() => setBusy(false));
              }}
            >
              {zh ? '保存' : 'Save'}
            </button>
          </div>
        </>
      )}
      {dialog === 'manage' && (
        <>
          <label>
            {zh ? '选择分组' : 'Choose group'}
            <select
              value={draft}
              onChange={(e) => {
                setDraft(e.target.value);
                setRename(e.target.value);
              }}
            >
              <option value="">{zh ? '选择…' : 'Choose…'}</option>
              {groups.map((g) => (
                <option key={g}>{g}</option>
              ))}
            </select>
          </label>
          <label>
            {zh ? '新名称' : 'New name'}
            <input maxLength={40} value={rename} onChange={(e) => setRename(e.target.value)} />
          </label>
          <p>
            {zh
              ? '移除分组保留其中的收藏记录。可在单条右键菜单或多选工具栏中创建分组。'
              : 'Removing a group keeps its favorites. Create groups from an item menu or the selection toolbar.'}
          </p>
          <div className="feature-actions">
            {button(zh ? '关闭' : 'Close', () => setDialog(null))}
            {button(
              zh ? '移除分组' : 'Remove group',
              () => {
                setBusy(true);
                void api
                  .renameGroup(draft, null)
                  .then(() => {
                    setDraft('');
                    setRename('');
                    p.setOptions({ ...p.options, group: null });
                  })
                  .catch(error)
                  .finally(() => setBusy(false));
              },
              !draft,
            )}
            <button
              className="btn btn--primary"
              disabled={busy || !draft || !rename.trim()}
              onClick={() => {
                setBusy(true);
                void api
                  .renameGroup(draft, rename.trim())
                  .then(() => {
                    if (p.options.group === draft) p.setOptions({ ...p.options, group: rename.trim() });
                    setDraft('');
                    setRename('');
                  })
                  .catch(error)
                  .finally(() => setBusy(false));
              }}
            >
              {zh ? '重命名' : 'Rename'}
            </button>
          </div>
        </>
      )}
    </FeatureDialog>
  );
  return { mode, ids, select, onKey, reset, requestDelete, open, controls, dialogs };
}
function localDate(ms: number) {
  const date = new Date(ms);
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`;
}
