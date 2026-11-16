import type { ReactNode } from 'react';

/**
 * Empty states draw a 米字格, the practice grid of a copybook. A fresh history shows one
 * vermilion stroke in it (the first stroke on an empty page); "no results" leaves it blank.
 */
export function EmptyState({
  title,
  body,
  footnote,
  stroke = false,
  action,
}: {
  title: string;
  body?: string;
  footnote?: ReactNode;
  stroke?: boolean;
  action?: { label: string; run: () => void };
}) {
  return (
    <div className="empty" role="status">
      <svg className="empty-grid" width="64" height="64" viewBox="0 0 64 64" aria-hidden="true">
        <path d="M1 1 63 63M63 1 1 63" className="grid-diag" />
        <path d="M32 1v62M1 32h62" className="grid-cross" />
        <rect x="0.5" y="0.5" width="63" height="63" className="grid-frame" />
        {stroke && <path d="M17 34.5c7.5-2.6 17.6-3.6 30.5-2.2" className="grid-stroke" />}
      </svg>
      <p className="empty-title">{title}</p>
      {body && <p className="empty-body">{body}</p>}
      {footnote && <p className="empty-foot">{footnote}</p>}
      {action && (
        <button type="button" className="btn btn--quiet" onClick={action.run}>
          {action.label}
        </button>
      )}
    </div>
  );
}
