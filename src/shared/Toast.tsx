import { useCallback, useEffect, useRef, useState } from 'react';

export interface ToastState {
  id: number;
  text: string;
  long: boolean;
}

/** One toast at a time; a new message replaces the old one. */
export function useToast() {
  const [toast, setToast] = useState<ToastState | null>(null);
  const timer = useRef<number | undefined>(undefined);
  const seq = useRef(0);

  const show = useCallback((text: string, long = false) => {
    window.clearTimeout(timer.current);
    seq.current += 1;
    setToast({ id: seq.current, text, long });
    timer.current = window.setTimeout(() => setToast(null), long ? 5200 : 2200);
  }, []);

  const dismiss = useCallback(() => {
    window.clearTimeout(timer.current);
    setToast(null);
  }, []);

  useEffect(() => () => window.clearTimeout(timer.current), []);
  return { toast, show, dismiss };
}

export function Toast({ toast, onDismiss }: { toast: ToastState | null; onDismiss: () => void }) {
  if (!toast) return null;
  return (
    <div className={`toast${toast.long ? ' toast--long' : ''}`} role="status" key={toast.id} onClick={onDismiss}>
      {toast.text}
    </div>
  );
}
