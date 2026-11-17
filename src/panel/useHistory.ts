import { useCallback, useEffect, useRef, useState } from 'react';
import { api } from '../shared/api';
import type { ClipItem, ListFilter, HistoryOptions } from '../shared/contract';

const PAGE = 60;

export type LoadStatus = 'loading' | 'ready' | 'error';

/**
 * The visible slice of history for one search + filter. Newer requests always win;
 * `refresh` reloads everything currently loaded so scroll depth survives live updates.
 */
export function useHistory(query: string, filter: ListFilter, options: HistoryOptions = {}) {
  const optionsKey = JSON.stringify(options);
  const [items, setItems] = useState<ClipItem[]>([]);
  const [total, setTotal] = useState(0);
  const [status, setStatus] = useState<LoadStatus>('loading');
  const latest = useRef({ query, filter, options, count: 0, total: 0, busy: false });
  const seq = useRef(0);
  latest.current.query = query;
  latest.current.filter = filter;
  latest.current.options = options;

  const fetchFrom = useCallback(async (offset: number, limit: number, append: boolean) => {
    const mine = ++seq.current;
    latest.current.busy = true;
    try {
      const { query: q, filter: f } = latest.current;
      const res = await api.listItems({ query: q, filter: f, offset, limit, ...latest.current.options });
      if (mine !== seq.current) return;
      setItems((prev) => {
        const next = append ? [...prev, ...res.items.filter((i) => !prev.some((p) => p.id === i.id))] : res.items;
        latest.current.count = next.length;
        return next;
      });
      latest.current.total = res.total;
      setTotal(res.total);
      setStatus('ready');
    } catch {
      if (mine === seq.current) setStatus('error');
    } finally {
      if (mine === seq.current) latest.current.busy = false;
    }
  }, []);

  // A new search or filter starts from the top. Typing is debounced a little.
  useEffect(() => {
    seq.current += 1;
    const timer = window.setTimeout(() => void fetchFrom(0, PAGE, false), query ? 90 : 0);
    return () => window.clearTimeout(timer);
  }, [query, filter, optionsKey, fetchFrom]);

  const refresh = useCallback(
    () => fetchFrom(0, Math.min(200, Math.max(PAGE, latest.current.count)), false),
    [fetchFrom],
  );

  const loadMore = useCallback(() => {
    const { count, total: all, busy } = latest.current;
    if (busy || count >= all) return;
    void fetchFrom(count, PAGE, true);
  }, [fetchFrom]);

  /** Optimistic local change, confirmed or undone by the next refresh. */
  const patch = useCallback((id: number, change: Partial<ClipItem>) => {
    setItems((prev) => prev.map((i) => (i.id === id ? { ...i, ...change } : i)));
  }, []);

  return { items, total, status, refresh, loadMore, patch };
}
