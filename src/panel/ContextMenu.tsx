import type { LucideIcon } from 'lucide-react';
import { useLayoutEffect, useRef, useState, type KeyboardEvent } from 'react';

export interface MenuAction {
  id: string;
  label: string;
  icon: LucideIcon;
  keys?: string[];
  danger?: boolean;
  run: () => void;
}

export type MenuEntry = MenuAction | 'separator';

const EDGE = 6;

/** A small menu anchored at a point inside the panel; flips to stay on screen. */
export function ContextMenu({
  x,
  y,
  entries,
  onClose,
}: {
  x: number;
  y: number;
  entries: MenuEntry[];
  onClose: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<{ left: number; top: number } | null>(null);
  const actions = entries.filter((e): e is MenuAction => e !== 'separator');
  const [active, setActive] = useState(0);

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const { width, height } = el.getBoundingClientRect();
    const left = Math.max(EDGE, Math.min(x, window.innerWidth - width - EDGE));
    const top = y + height > window.innerHeight - EDGE ? Math.max(EDGE, y - height) : y;
    setPos({ left, top });
    el.focus();
  }, [x, y]);
  useLayoutEffect(() => {
    ref.current?.querySelector('.menu-item.is-active')?.scrollIntoView({ block: 'nearest' });
  }, [active]);

  const runAt = (i: number) => {
    const action = actions[i];
    if (!action) return;
    onClose();
    action.run();
  };

  const onKeyDown = (e: KeyboardEvent) => {
    e.stopPropagation();
    switch (e.key) {
      case 'ArrowDown':
        e.preventDefault();
        setActive((a) => (a + 1) % actions.length);
        break;
      case 'ArrowUp':
        e.preventDefault();
        setActive((a) => (a - 1 + actions.length) % actions.length);
        break;
      case 'Home':
        e.preventDefault();
        setActive(0);
        break;
      case 'End':
        e.preventDefault();
        setActive(actions.length - 1);
        break;
      case 'Enter':
      case ' ':
        e.preventDefault();
        runAt(active);
        break;
      case 'Escape':
      case 'Tab':
        e.preventDefault();
        onClose();
        break;
    }
  };

  let actionIndex = -1;
  return (
    <>
      <div className="menu-scrim" onMouseDown={onClose} onContextMenu={(e) => (e.preventDefault(), onClose())} />
      <div
        ref={ref}
        className="menu"
        role="menu"
        tabIndex={-1}
        style={pos ?? { left: x, top: y, visibility: 'hidden' }}
        onKeyDown={onKeyDown}
        onContextMenu={(e) => e.preventDefault()}
      >
        {entries.map((entry, i) => {
          if (entry === 'separator') return <div key={`sep-${i}`} className="menu-sep" role="separator" />;
          actionIndex += 1;
          const index = actionIndex;
          const Icon = entry.icon;
          return (
            <button
              key={entry.id}
              type="button"
              role="menuitem"
              className={`menu-item${index === active ? ' is-active' : ''}${entry.danger ? ' is-danger' : ''}`}
              onMouseEnter={() => setActive(index)}
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => runAt(index)}
            >
              <Icon size={15} strokeWidth={1.75} aria-hidden="true" />
              <span className="menu-label">{entry.label}</span>
              {entry.keys && (
                <span className="menu-keys">
                  {entry.keys.map((k) => (
                    <kbd key={k} className="kbd">
                      {k}
                    </kbd>
                  ))}
                </span>
              )}
            </button>
          );
        })}
      </div>
    </>
  );
}
