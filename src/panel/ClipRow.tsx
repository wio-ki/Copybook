import { File, Folder, MoreHorizontal, PenLine, Pin, Star } from 'lucide-react';
import { memo, type MouseEvent } from 'react';
import type { Lang } from '../shared/app';
import { assetUrl } from '../shared/api';
import type { ClipItem, Density } from '../shared/contract';
import {
  formatBytes,
  formatNumber,
  isCssColor,
  looksLikeCode,
  relativeTime,
  splitPath,
  splitUrl,
  tidyPreview,
} from '../shared/format';
import type { Strings } from '../shared/strings';

interface RowProps {
  item: ClipItem;
  index: number;
  selected: boolean;
  multi?: boolean;
  focused?: boolean;
  density: Density;
  showSource: boolean;
  t: Strings;
  lang: Lang;
  now: number;
  onSelect: (id: number, e?: MouseEvent) => void;
  onPaste: (item: ClipItem) => void;
  onMenu: (item: ClipItem, x: number, y: number) => void;
  onToggleFavorite: (item: ClipItem) => void;
}

const keepFocus = (e: MouseEvent) => e.preventDefault();

export const ClipRow = memo(function ClipRow(props: RowProps) {
  const { item, index, selected, density, showSource, t, lang, now } = props;
  const compact = density === 'compact';

  const openMenu = (e: MouseEvent) => {
    e.preventDefault();
    props.onSelect(item.id);
    props.onMenu(item, e.clientX, e.clientY);
  };
  const openMenuFromButton = (e: MouseEvent<HTMLButtonElement>) => {
    e.stopPropagation();
    const r = e.currentTarget.getBoundingClientRect();
    props.onSelect(item.id);
    props.onMenu(item, r.right, r.bottom + 4);
  };

  return (
    <div
      id={`clip-${item.id}`}
      role="option"
      aria-selected={selected}
      data-index={index}
      className={`row row--${item.kind}${compact ? ' row--compact' : ''}${selected ? ' is-selected' : ''}${props.multi ? ' row--multi' : ''}${props.multi && props.focused ? ' is-focused' : ''}`}
      onMouseDown={keepFocus}
      onClick={(e) => props.onSelect(item.id,e)}
      onDoubleClick={() => { if (!props.multi) props.onPaste(item); }}
      onContextMenu={openMenu}
    >
      <span className="stamp tnum" aria-hidden="true">
        {props.multi ? (selected ? '✓' : null) : index < 9 ? index + 1 : ''}
      </span>

      <div className="row-body">
        {item.note && !compact && (
          <p className="row-note">
            <PenLine size={11} strokeWidth={2} aria-hidden="true" />
            <span>{item.note}</span>
          </p>
        )}
        {item.group && <span className="row-group">{item.group}</span>}
        <Content item={item} compact={compact} t={t} />
        {!compact && <Meta item={item} showSource={showSource} t={t} lang={lang} now={now} />}
      </div>

      {compact && (
        <time className="row-time tnum" dateTime={new Date(item.lastUsedAt).toISOString()}>
          {relativeTime(item.lastUsedAt, t, lang, now)}
        </time>
      )}

      <div className="row-marks" aria-hidden="true">
        {item.pinned && <Pin size={12} strokeWidth={2} className="mark-pin" />}
        {item.favorite && <Star size={12} strokeWidth={2} className="mark-star" />}
      </div>

      <div className="row-actions">
        <button
          type="button"
          className={`icon-btn icon-btn--sm${item.favorite ? ' is-on' : ''}`}
          title={item.favorite ? t.actions.unfavorite : t.actions.favorite}
          aria-label={item.favorite ? t.actions.unfavorite : t.actions.favorite}
          onMouseDown={keepFocus}
          onClick={(e) => {
            e.stopPropagation();
            props.onToggleFavorite(item);
          }}
          onDoubleClick={(e) => e.stopPropagation()}
        >
          <Star size={14} strokeWidth={1.75} />
        </button>
        <button
          type="button"
          className="icon-btn icon-btn--sm"
          title={t.actions.more}
          aria-label={t.actions.more}
          onMouseDown={keepFocus}
          onClick={openMenuFromButton}
          onDoubleClick={(e) => e.stopPropagation()}
        >
          <MoreHorizontal size={14} strokeWidth={1.75} />
        </button>
      </div>
    </div>
  );
});

function Content({ item, compact, t }: { item: ClipItem; compact: boolean; t: Strings }) {
  if (item.kind === 'image' && item.image) {
    const { width, height, thumb } = item.image;
    return (
      <div className="row-image">
        <img
          className="thumb checker"
          src={assetUrl(thumb)}
          width={width}
          height={height}
          alt=""
          loading="lazy"
          decoding="async"
          draggable={false}
        />
        {compact && (
          <span className="row-dim tnum">
            {width} × {height}
          </span>
        )}
      </div>
    );
  }

  if (item.kind === 'files' && item.files) {
    const shown = item.files.slice(0, compact ? 1 : 2);
    const rest = item.files.length - shown.length;
    return (
      <ul className="row-files">
        {shown.map((f) => (
          <li key={f.path} className={f.exists ? '' : 'is-missing'}>
            {f.isDir ? (
              <Folder size={14} strokeWidth={1.75} aria-hidden="true" />
            ) : (
              <File size={14} strokeWidth={1.75} aria-hidden="true" />
            )}
            <span className="file-name">{f.name}</span>
            {!f.exists && <span className="file-flag">{t.meta.missing}</span>}
            {compact && rest > 0 && <span className="file-more">+{rest}</span>}
          </li>
        ))}
        {!compact && rest > 0 && <li className="file-more">{t.meta.moreFiles(rest)}</li>}
      </ul>
    );
  }

  const value = item.text;
  switch (item.subKind) {
    case 'url': {
      const parts = splitUrl(value);
      if (parts)
        return (
          <p className={`row-text row-url${compact ? ' is-single' : ''}`}>
            <span className="url-host">{parts.host}</span>
            <span className="url-rest">{parts.rest}</span>
          </p>
        );
      break;
    }
    case 'color':
      if (isCssColor(value))
        return (
          <p className="row-text row-color">
            <span className="swatch" style={{ background: value.trim() }} aria-hidden="true" />
            <span className="mono">{value.trim()}</span>
          </p>
        );
      break;
    case 'path': {
      const { dir, name } = splitPath(value);
      return (
        <p className={`row-text row-path${compact ? ' is-single' : ''}`}>
          <span className="path-dir">{dir}</span>
          <span className="path-name">{name}</span>
        </p>
      );
    }
  }

  const tidy = tidyPreview(value);
  const code = looksLikeCode(tidy, item.lineCount);
  return (
    <p className={`row-text${code ? ' is-code mono' : ''}${compact ? ' is-single' : ''}`} lang={code ? 'en' : undefined}>
      {compact ? tidy.replace(/\s*\n\s*/g, ' ⏎ ') : tidy}
    </p>
  );
}

function Meta({ item, showSource, t, lang, now }: { item: ClipItem; showSource: boolean; t: Strings; lang: Lang; now: number }) {
  let extra = '';
  if (item.kind === 'image' && item.image) {
    // Pixels only: the row is narrow, the app name matters more than the file size here.
    extra = `${item.image.width}×${item.image.height}`;
  } else if (item.kind === 'files' && item.files) {
    const one = item.files.length === 1 ? item.files[0] : null;
    extra = one
      ? one.isDir
        ? t.meta.folder
        : one.bytes != null
          ? formatBytes(one.bytes, lang)
          : ''
      : t.meta.items(item.files.length);
  } else if (item.lineCount > 1) {
    extra = t.meta.lines(formatNumber(item.lineCount, lang));
  } else {
    extra = t.meta.chars(formatNumber(item.textLength, lang));
  }

  return (
    <div className="row-meta">
      {showSource && (
        <>
          <span className="meta-source">
            {item.source?.icon ? (
              <img className="app-icon" src={assetUrl(item.source.icon)} alt="" draggable={false} />
            ) : (
              <span className="app-icon app-icon--blank" aria-hidden="true" />
            )}
            <span className="app-name">{item.source?.name ?? t.meta.unknownApp}</span>
          </span>
          <span className="meta-sep" aria-hidden="true">·</span>
        </>
      )}
      <time className="tnum" dateTime={new Date(item.lastUsedAt).toISOString()}>
        {relativeTime(item.lastUsedAt, t, lang, now)}
      </time>
      <span className="meta-extra tnum">
        {(item.hasHtml || item.hasRtf) && <span className="chip">{t.kinds.rich}</span>}
        {extra}
      </span>
    </div>
  );
}
