import {
  ArrowLeft,
  Copy,
  ExternalLink,
  File,
  Folder,
  FolderOpen,
  Pin,
  PinOff,
  Star,
  Trash2,
} from 'lucide-react';
import { useEffect, useMemo, useState } from 'react';
import { api, assetUrl } from '../shared/api';
import type { Lang } from '../shared/app';
import type { ClipItem, ItemContent } from '../shared/contract';
import {
  absoluteTime,
  formatBytes,
  formatNumber,
  isCssColor,
  looksLikeCode,
  splitPath,
} from '../shared/format';
import type { Strings } from '../shared/strings';

const MAX_CHARS = 200_000;

interface DetailProps {
  item: ClipItem;
  t: Strings;
  lang: Lang;
  pastePlain: boolean;
  onClose: () => void;
  onPaste: (item: ClipItem, invert: boolean) => void;
  onCopy: (item: ClipItem) => void;
  onToggleFavorite: (item: ClipItem) => void;
  onTogglePin: (item: ClipItem) => void;
  onDelete: (item: ClipItem) => void;
  onSaveNote: (item: ClipItem, note: string | null) => Promise<void>;
}

export function Detail(props: DetailProps) {
  const { item, t, lang, pastePlain } = props;
  const [content, setContent] = useState<ItemContent | null>(null);
  const [note, setNote] = useState(item.note ?? '');

  useEffect(() => {
    setNote(item.note ?? '');
  }, [item.id, item.note]);

  useEffect(() => {
    if (item.kind !== 'text') return;
    let alive = true;
    setContent(null);
    api
      .getItemContent(item.id, MAX_CHARS)
      .then((c) => alive && setContent(c))
      .catch(() => alive && setContent({ text: item.text, truncated: item.textLength > item.text.length, html: null }));
    return () => {
      alive = false;
    };
  }, [item.id, item.kind, item.text, item.textLength]);

  const saveNote = () => {
    const next = note.trim();
    if (next === (item.note ?? '')) return;
    void props.onSaveNote(item, next || null);
  };

  const kindLabel =
    item.kind === 'image'
      ? t.kinds.image
      : item.kind === 'files'
        ? t.kinds.files
        : item.subKind
          ? t.kinds[item.subKind]
          : item.hasHtml || item.hasRtf
            ? t.kinds.rich
            : t.kinds.text;

  const canPlain = (item.kind === 'text' && (item.hasHtml || item.hasRtf)) || item.kind === 'files';
  const plainLabel = item.kind === 'files' ? t.actions.pastePaths : pastePlain ? t.actions.pasteRich : t.actions.pastePlain;

  return (
    <section className="detail" aria-label={kindLabel}>
      <header className="detail-bar">
        <button type="button" className="icon-btn" title={`${t.actions.back} (Esc)`} aria-label={t.actions.back} onClick={props.onClose}>
          <ArrowLeft size={16} strokeWidth={1.75} />
        </button>
        <div className="detail-title">
          <span className="detail-kind">{kindLabel}</span>
          <span className="detail-sub tnum">{summary(item, t, lang)}</span>
        </div>
        <div className="detail-tools">
          <button
            type="button"
            className={`icon-btn${item.favorite ? ' is-on' : ''}`}
            title={`${item.favorite ? t.actions.unfavorite : t.actions.favorite} (Ctrl+S)`}
            aria-pressed={item.favorite}
            onClick={() => props.onToggleFavorite(item)}
          >
            <Star size={16} strokeWidth={1.75} />
          </button>
          <button
            type="button"
            className={`icon-btn${item.pinned ? ' is-on-ink' : ''}`}
            title={`${item.pinned ? t.actions.unpin : t.actions.pin} (Ctrl+P)`}
            aria-pressed={item.pinned}
            onClick={() => props.onTogglePin(item)}
          >
            {item.pinned ? <PinOff size={16} strokeWidth={1.75} /> : <Pin size={16} strokeWidth={1.75} />}
          </button>
          <button
            type="button"
            className="icon-btn icon-btn--danger"
            title={`${t.actions.delete} (Ctrl+Del)`}
            onClick={() => props.onDelete(item)}
          >
            <Trash2 size={16} strokeWidth={1.75} />
          </button>
        </div>
      </header>

      <div className="detail-scroll">
        <Body item={item} content={content} t={t} lang={lang} />

        <dl className="detail-info">
          <dt>{t.detail.source}</dt>
          <dd className="info-source">
            {item.source?.icon && <img className="app-icon" src={assetUrl(item.source.icon)} alt="" />}
            <span>{item.source ? item.source.name : t.meta.unknownApp}</span>
            {item.source && <span className="info-dim">{item.source.exe}</span>}
          </dd>
          <dt>{t.detail.created}</dt>
          <dd className="tnum">{absoluteTime(item.createdAt, lang)}</dd>
          <dt>{t.detail.lastUsed}</dt>
          <dd className="tnum">{absoluteTime(item.lastUsedAt, lang)}</dd>
          <dt>{t.detail.used}</dt>
          <dd className="tnum">{t.detail.times(item.useCount)}</dd>
          <dt>{t.detail.format}</dt>
          <dd>{formats(item, t).join(' · ')}</dd>
        </dl>

        <label className="detail-note">
          <span className="note-label">{t.detail.note}</span>
          <textarea
            value={note}
            maxLength={500}
            rows={2}
            placeholder={t.detail.notePlaceholder}
            onChange={(e) => setNote(e.target.value)}
            onBlur={saveNote}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && (e.ctrlKey || !e.shiftKey)) {
                e.preventDefault();
                e.currentTarget.blur();
              }
            }}
          />
        </label>
      </div>

      <footer className="detail-actions">
        <button type="button" className="btn btn--primary" onClick={() => props.onPaste(item, false)}>
          {pastePlain && item.kind !== 'image' ? t.actions.pastePlain : t.actions.paste}
          <kbd className="kbd kbd--on-primary">↵</kbd>
        </button>
        {canPlain && (
          <button type="button" className="btn" onClick={() => props.onPaste(item, true)}>
            {plainLabel}
            <kbd className="kbd">⇧↵</kbd>
          </button>
        )}
        <button type="button" className="btn" onClick={() => props.onCopy(item)}>
          <Copy size={14} strokeWidth={1.75} aria-hidden="true" />
          {t.actions.copy}
        </button>
        {item.subKind === 'url' && (
          <button type="button" className="btn btn--quiet" onClick={() => void api.openUrl(withScheme(item.text.trim()))}>
            <ExternalLink size={14} strokeWidth={1.75} aria-hidden="true" />
            {t.actions.open}
          </button>
        )}
        {item.subKind === 'path' && (
          <button type="button" className="btn btn--quiet" onClick={() => void api.revealPath(splitPathFull(item.text))}>
            <FolderOpen size={14} strokeWidth={1.75} aria-hidden="true" />
            {t.actions.revealShort}
          </button>
        )}
      </footer>
    </section>
  );
}

function Body({ item, content, t, lang }: { item: ClipItem; content: ItemContent | null; t: Strings; lang: Lang }) {
  if (item.kind === 'image' && item.image) {
    return (
      <div className="detail-image checker">
        <img src={assetUrl(item.image.path)} alt="" draggable={false} />
      </div>
    );
  }

  if (item.kind === 'files' && item.files) {
    return (
      <ul className="detail-files">
        {item.files.map((f) => {
          const { dir } = splitPath(f.path);
          return (
            <li key={f.path} className={f.exists ? '' : 'is-missing'}>
              {f.isDir ? <Folder size={16} strokeWidth={1.75} aria-hidden="true" /> : <File size={16} strokeWidth={1.75} aria-hidden="true" />}
              <div className="file-text">
                <span className="file-name">{f.name}</span>
                <span className="file-dir">{dir}</span>
              </div>
              <span className="file-size tnum">
                {f.exists ? (f.isDir ? t.meta.folder : f.bytes != null ? formatBytes(f.bytes, lang) : '') : t.meta.missing}
              </span>
              {f.exists && (
                <button
                  type="button"
                  className="icon-btn icon-btn--sm"
                  title={t.actions.reveal}
                  aria-label={t.actions.reveal}
                  onClick={() => void api.revealPath(f.path)}
                >
                  <FolderOpen size={14} strokeWidth={1.75} />
                </button>
              )}
            </li>
          );
        })}
      </ul>
    );
  }

  if (item.subKind === 'color' && isCssColor(item.text)) {
    return <ColorBody value={item.text.trim()} />;
  }

  const text = content?.text ?? item.text;
  const code = looksLikeCode(text, item.lineCount);
  return (
    <div className="detail-content">
      <pre className={`detail-text${code ? ' mono' : ''}${content ? '' : ' is-loading'}`}>{text}</pre>
      {content?.truncated && <p className="detail-hint">{t.detail.truncated}</p>}
    </div>
  );
}

function ColorBody({ value }: { value: string }) {
  const values = useMemo(() => colorFormats(value), [value]);
  return (
    <div className="detail-color">
      <div className="color-chip checker">
        <span style={{ background: value }} />
      </div>
      <ul className="color-values mono">
        {values.map((v) => (
          <li key={v}>{v}</li>
        ))}
      </ul>
    </div>
  );
}

/** The colour as written plus its hex and rgb() forms, resolved by the browser. */
function colorFormats(value: string): string[] {
  const probe = document.createElement('span');
  probe.style.color = value;
  document.body.appendChild(probe);
  const rgb = getComputedStyle(probe).color;
  probe.remove();
  const m = rgb.match(/[\d.]+/g);
  const out = [value];
  if (m && m.length >= 3) {
    const [r, g, b] = m.map(Number);
    const a = m.length > 3 ? Number(m[3]) : 1;
    const hex = `#${[r, g, b, ...(a < 1 ? [Math.round(a * 255)] : [])]
      .map((n) => Math.round(n).toString(16).padStart(2, '0'))
      .join('')}`.toUpperCase();
    const rgbText = a < 1 ? `rgb(${r} ${g} ${b} / ${a})` : `rgb(${r} ${g} ${b})`;
    for (const v of [hex, rgbText]) if (v.toLowerCase() !== value.toLowerCase()) out.push(v);
  }
  return out;
}

function summary(item: ClipItem, t: Strings, lang: Lang): string {
  if (item.kind === 'image' && item.image) {
    return `${item.image.width} × ${item.image.height} · ${formatBytes(item.image.bytes, lang)}`;
  }
  if (item.kind === 'files' && item.files) {
    const bytes = item.files.reduce((sum, f) => sum + (f.bytes ?? 0), 0);
    return bytes > 0 ? `${t.meta.items(item.files.length)} · ${formatBytes(bytes, lang)}` : t.meta.items(item.files.length);
  }
  const chars = t.meta.chars(formatNumber(item.textLength, lang));
  return item.lineCount > 1 ? `${t.meta.lines(formatNumber(item.lineCount, lang))} · ${chars}` : chars;
}

function formats(item: ClipItem, t: Strings): string[] {
  if (item.kind === 'image') return ['PNG'];
  if (item.kind === 'files') return [t.kinds.files];
  const out = [t.detail.plain];
  if (item.hasHtml) out.push('HTML');
  if (item.hasRtf) out.push('RTF');
  return out;
}

function withScheme(url: string): string {
  return /^[a-z][a-z0-9+.-]*:/i.test(url) ? url : `https://${url}`;
}

function splitPathFull(raw: string): string {
  return raw.trim().replace(/^"(.*)"$/, '$1');
}
