import { Check, ChevronDown } from 'lucide-react';
import { useEffect, useRef, useState, type ReactNode } from 'react';
import { hotkeyFromEvent, hotkeyParts } from '../shared/hotkey';
import type { Strings } from '../shared/strings';

/* A settings row: label and hint on the left, control on the right. */
export function Row({
  label,
  hint,
  children,
  htmlFor,
  stacked = false,
}: {
  label: string;
  hint?: string;
  children: ReactNode;
  htmlFor?: string;
  stacked?: boolean;
}) {
  return (
    <div className={`srow${stacked ? ' srow--stacked' : ''}`}>
      <div className="srow-text">
        {htmlFor ? (
          <label className="srow-label" htmlFor={htmlFor}>
            {label}
          </label>
        ) : (
          <span className="srow-label">{label}</span>
        )}
        {hint && <span className="srow-hint">{hint}</span>}
      </div>
      <div className="srow-control">{children}</div>
    </div>
  );
}

export function Group({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="sgroup">
      <h2 className="sgroup-title">{title}</h2>
      <div className="sgroup-rows">{children}</div>
    </section>
  );
}

export function Switch({
  id,
  checked,
  onChange,
  disabled,
  label,
}: {
  id?: string;
  checked: boolean;
  onChange: (value: boolean) => void;
  disabled?: boolean;
  label?: string;
}) {
  return (
    <button
      id={id}
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      className={`switch${checked ? ' is-on' : ''}`}
      onClick={() => onChange(!checked)}
    >
      <span className="switch-thumb" />
    </button>
  );
}

export function Segmented<T extends string>({
  value,
  options,
  onChange,
  label,
}: {
  value: T;
  options: { value: T; label: string }[];
  onChange: (value: T) => void;
  label: string;
}) {
  return (
    <div className="segmented" role="radiogroup" aria-label={label}>
      {options.map((o) => (
        <button
          key={o.value}
          type="button"
          role="radio"
          aria-checked={value === o.value}
          className={`seg${value === o.value ? ' is-active' : ''}`}
          onClick={() => onChange(o.value)}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

export function Select<T extends string | number>({
  id,
  value,
  options,
  onChange,
}: {
  id?: string;
  value: T;
  options: { value: T; label: string }[];
  onChange: (value: T) => void;
}) {
  const known = options.some((o) => o.value === value);
  return (
    <span className="select">
      <select
        id={id}
        value={String(value)}
        onChange={(e) => {
          const raw = e.target.value;
          const hit = options.find((o) => String(o.value) === raw);
          if (hit) onChange(hit.value);
        }}
      >
        {!known && <option value={String(value)}>{String(value)}</option>}
        {options.map((o) => (
          <option key={String(o.value)} value={String(o.value)}>
            {o.label}
          </option>
        ))}
      </select>
      <ChevronDown size={14} strokeWidth={1.75} aria-hidden="true" />
    </span>
  );
}

/**
 * Click, then press a combination. Esc cancels, Backspace clears. The recorder only
 * proposes; `onCommit` decides (the backend may reject a taken shortcut).
 */
export function HotkeyRecorder({
  value,
  t,
  onCommit,
}: {
  value: string;
  t: Strings;
  onCommit: (hotkey: string) => Promise<boolean>;
}) {
  const [recording, setRecording] = useState(false);
  const [pending, setPending] = useState<string[]>([]);
  const [hint, setHint] = useState<string | null>(null);
  const ref = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (!recording) return;
    const onKey = async (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === 'Escape' && !e.ctrlKey && !e.altKey && !e.shiftKey && !e.metaKey) {
        setRecording(false);
        return;
      }
      if (e.key === 'Backspace' && !e.ctrlKey && !e.altKey && !e.shiftKey && !e.metaKey) {
        setRecording(false);
        await onCommit('');
        return;
      }
      const r = hotkeyFromEvent(e);
      if (r.kind === 'pending') {
        setPending(r.parts);
        setHint(null);
      } else if (r.kind === 'needs-modifier') {
        setHint(t.settings.recorder.needModifier);
      } else {
        setRecording(false);
        await onCommit(r.hotkey);
      }
    };
    const onUp = (e: KeyboardEvent) => {
      const r = hotkeyFromEvent(e);
      if (r.kind === 'pending') setPending(r.parts.filter((p) => p !== keyNameOf(e)));
    };
    window.addEventListener('keydown', onKey, true);
    window.addEventListener('keyup', onUp, true);
    return () => {
      window.removeEventListener('keydown', onKey, true);
      window.removeEventListener('keyup', onUp, true);
    };
  }, [recording, onCommit, t]);

  const parts = recording ? pending : hotkeyParts(value);
  return (
    <div className="recorder-wrap">
      <span className="recorder-hint" aria-live="polite">
        {hint ?? ''}
      </span>
      <button
        ref={ref}
        type="button"
        className={`recorder${recording ? ' is-recording' : ''}`}
        title={recording ? t.settings.recorder.recording : t.settings.recorder.idle}
        onClick={() => {
          setPending([]);
          setHint(null);
          setRecording((r) => !r);
        }}
        onBlur={() => setRecording(false)}
      >
        {parts.length ? (
          parts.map((p, i) => (
            <kbd key={`${p}-${i}`} className="kbd kbd--lg">
              {p}
            </kbd>
          ))
        ) : (
          <span className="recorder-empty">{recording ? t.settings.recorder.recording : t.settings.recorder.none}</span>
        )}
        {recording && parts.length > 0 && <span className="recorder-caret" aria-hidden="true" />}
      </button>
    </div>
  );
}

function keyNameOf(e: KeyboardEvent): string {
  if (e.key === 'Control') return 'Ctrl';
  if (e.key === 'Meta' || e.key === 'OS') return 'Win';
  return e.key;
}

export function SavedTick({ show }: { show: boolean }) {
  return (
    <span className={`saved-tick${show ? ' is-shown' : ''}`} aria-hidden={!show}>
      <Check size={12} strokeWidth={2.25} />
    </span>
  );
}
