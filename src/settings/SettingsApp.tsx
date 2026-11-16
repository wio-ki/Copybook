import {
  ClipboardPaste,
  Database,
  ExternalLink,
  FolderOpen,
  Info,
  Minus,
  Plus,
  Radio,
  SlidersHorizontal,
  X,
} from 'lucide-react';
import { useCallback, useEffect, useMemo, useState, type ReactNode } from 'react';
import { api, assetUrl, toCmdError } from '../shared/api';
import { useApp } from '../shared/app';
import type { AppInfo, SeenApp, Settings, SettingsSection, Stats } from '../shared/contract';
import { formatBytes, formatNumber } from '../shared/format';
import { Seal } from '../shared/Seal';
import { Toast, useToast } from '../shared/Toast';
import { Group, HotkeyRecorder, Row, Segmented, Select, Switch } from './controls';

const SECTIONS: { id: SettingsSection; icon: typeof Info }[] = [
  { id: 'general', icon: SlidersHorizontal },
  { id: 'capture', icon: Radio },
  { id: 'paste', icon: ClipboardPaste },
  { id: 'data', icon: Database },
  { id: 'about', icon: Info },
];

const REPO = 'https://github.com/wio-ki/Copybook';

function sectionFromHash(): SettingsSection {
  const h = location.hash.replace('#', '');
  return SECTIONS.some((s) => s.id === h) ? (h as SettingsSection) : 'general';
}

export function SettingsApp() {
  const { t, settings, update } = useApp();
  const [section, setSection] = useState<SettingsSection>(sectionFromHash);
  const { toast, show, dismiss } = useToast();

  useEffect(() => {
    const off = api.on('settings://navigate', ({ section: s }) => setSection(s));
    return () => void off.then((f) => f());
  }, []);

  useEffect(() => {
    history.replaceState(null, '', `#${section}`);
  }, [section]);

  const save = useCallback(
    async (patch: Partial<Settings>) => {
      try {
        await update(patch);
        return true;
      } catch (e) {
        const err = toCmdError(e);
        show(t.errors[err.code] ?? t.toast.failed);
        return false;
      }
    },
    [update, show, t],
  );

  const page = useMemo(() => {
    switch (section) {
      case 'general':
        return <General settings={settings} save={save} />;
      case 'capture':
        return <Capture settings={settings} save={save} />;
      case 'paste':
        return <Paste settings={settings} save={save} />;
      case 'data':
        return <Data notify={show} />;
      case 'about':
        return <About />;
    }
  }, [section, settings, save, show]);

  return (
    <div className="settings">
      <header className="titlebar" data-tauri-drag-region>
        <span className="titlebar-brand" data-tauri-drag-region>
          <Seal size={18} />
          <span className="titlebar-name" data-tauri-drag-region>
            Copybook
          </span>
          <span className="titlebar-sub" data-tauri-drag-region>
            {t.settings.title}
          </span>
        </span>
        <span className="titlebar-buttons">
          <button type="button" className="wbtn" aria-label={t.settings.minimize} title={t.settings.minimize} onClick={() => void api.minimizeWindow()}>
            <Minus size={16} strokeWidth={1.5} />
          </button>
          <button type="button" className="wbtn wbtn--close" aria-label={t.settings.close} title={t.settings.close} onClick={() => void api.closeWindow()}>
            <X size={16} strokeWidth={1.5} />
          </button>
        </span>
      </header>

      <div className="settings-body">
        <nav className="sidebar" aria-label={t.settings.title}>
          {SECTIONS.map(({ id, icon: Icon }) => (
            <button
              key={id}
              type="button"
              className={`nav-item${section === id ? ' is-active' : ''}`}
              aria-current={section === id ? 'page' : undefined}
              onClick={() => setSection(id)}
            >
              <Icon size={16} strokeWidth={1.75} aria-hidden="true" />
              {t.settings.nav[id]}
            </button>
          ))}
        </nav>

        <main className="sheet" key={section}>
          {page}
        </main>
      </div>

      <Toast toast={toast} onDismiss={dismiss} />
    </div>
  );
}

type SaveFn = (patch: Partial<Settings>) => Promise<boolean>;

function PageHead({ title, lead }: { title: string; lead: string }) {
  return (
    <header className="page-head">
      <h1>{title}</h1>
      <p>{lead}</p>
    </header>
  );
}

// ---- General --------------------------------------------------------------------------

function General({ settings, save }: { settings: Settings; save: SaveFn }) {
  const { t } = useApp();
  const g = t.settings.general;
  return (
    <>
      <PageHead title={g.title} lead={g.lead} />
      <Group title={g.appearance}>
        <Row label={g.language}>
          <Segmented
            label={g.language}
            value={settings.language}
            onChange={(language) => void save({ language })}
            options={[
              { value: 'system', label: g.langSystem },
              { value: 'zh-CN', label: '简体中文' },
              { value: 'en-US', label: 'English' },
            ]}
          />
        </Row>
        <Row label={g.theme}>
          <Segmented
            label={g.theme}
            value={settings.theme}
            onChange={(theme) => void save({ theme })}
            options={[
              { value: 'system', label: g.themeSystem },
              { value: 'light', label: g.themeLight },
              { value: 'dark', label: g.themeDark },
            ]}
          />
        </Row>
        <Row label={g.density} hint={g.densityHint}>
          <Segmented
            label={g.density}
            value={settings.density}
            onChange={(density) => void save({ density })}
            options={[
              { value: 'comfortable', label: g.comfortable },
              { value: 'compact', label: g.compact },
            ]}
          />
        </Row>
      </Group>

      <Group title={g.launch}>
        <Row label={g.hotkey} hint={g.hotkeyHint}>
          <HotkeyRecorder value={settings.hotkey} t={t} onCommit={(hotkey) => save({ hotkey })} />
        </Row>
        <Row label={g.winV} hint={g.winVHint} htmlFor="s-winv">
          <Switch id="s-winv" checked={settings.winV} onChange={(winV) => void save({ winV })} />
        </Row>
        <Row label={g.autostart} hint={g.autostartHint} htmlFor="s-autostart">
          <Switch id="s-autostart" checked={settings.autostart} onChange={(autostart) => void save({ autostart })} />
        </Row>
      </Group>

      <Group title={g.panel}>
        <Row label={g.position}>
          <Segmented
            label={g.position}
            value={settings.panelPosition}
            onChange={(panelPosition) => void save({ panelPosition })}
            options={[
              { value: 'cursor', label: g.posCursor },
              { value: 'center', label: g.posCenter },
              { value: 'remember', label: g.posRemember },
            ]}
          />
        </Row>
        <Row label={g.hideOnBlur} hint={g.hideOnBlurHint} htmlFor="s-blur">
          <Switch id="s-blur" checked={settings.hideOnBlur} onChange={(hideOnBlur) => void save({ hideOnBlur })} />
        </Row>
      </Group>
    </>
  );
}

// ---- Capture --------------------------------------------------------------------------

function Capture({ settings, save }: { settings: Settings; save: SaveFn }) {
  const { t, lang } = useApp();
  const c = t.settings.capture;
  const n = (v: number) => formatNumber(v, lang);
  return (
    <>
      <PageHead title={c.title} lead={c.lead} />
      <Group title={c.kinds}>
        <Row label={c.text} htmlFor="s-text">
          <Switch id="s-text" checked={settings.captureText} onChange={(captureText) => void save({ captureText })} />
        </Row>
        <Row label={c.rich} hint={c.richHint} htmlFor="s-rich">
          <Switch
            id="s-rich"
            checked={settings.captureRich}
            disabled={!settings.captureText}
            onChange={(captureRich) => void save({ captureRich })}
          />
        </Row>
        <Row label={c.images} htmlFor="s-images">
          <Switch id="s-images" checked={settings.captureImages} onChange={(captureImages) => void save({ captureImages })} />
        </Row>
        <Row label={c.files} htmlFor="s-files">
          <Switch id="s-files" checked={settings.captureFiles} onChange={(captureFiles) => void save({ captureFiles })} />
        </Row>
      </Group>

      <Group title={c.retention}>
        <Row label={c.limit} hint={c.limitHint} htmlFor="s-limit">
          <Select
            id="s-limit"
            value={settings.historyLimit}
            onChange={(historyLimit) => void save({ historyLimit })}
            options={[200, 500, 1000, 2000, 5000, 10000]
              .map((v) => ({ value: v, label: c.nItems(n(v)) }))
              .concat([{ value: 0, label: c.unlimited }])}
          />
        </Row>
        <Row label={c.days} htmlFor="s-days">
          <Select
            id="s-days"
            value={settings.retentionDays}
            onChange={(retentionDays) => void save({ retentionDays })}
            options={[{ value: 0, label: c.forever }].concat(
              [1, 7, 30, 90, 365].map((v) => ({ value: v, label: c.nDays(v) })),
            )}
          />
        </Row>
        <Row label={c.maxImage} htmlFor="s-maximg">
          <Select
            id="s-maximg"
            value={settings.maxImageMB}
            onChange={(maxImageMB) => void save({ maxImageMB })}
            options={[5, 10, 20, 50, 100].map((v) => ({ value: v, label: c.nMB(v) }))}
          />
        </Row>
      </Group>

      <Group title={c.privacy}>
        <Row label={c.hints} hint={c.hintsHint} htmlFor="s-hints">
          <Switch
            id="s-hints"
            checked={settings.respectPrivacyHints}
            onChange={(respectPrivacyHints) => void save({ respectPrivacyHints })}
          />
        </Row>
        <Row label={c.secrets} hint={c.secretsHint} htmlFor="s-secrets">
          <Switch id="s-secrets" checked={settings.skipSecrets} onChange={(skipSecrets) => void save({ skipSecrets })} />
        </Row>
        <IgnoredApps value={settings.ignoredApps} save={save} />
      </Group>
    </>
  );
}

function normaliseExe(raw: string): string {
  const name = raw.trim().replace(/^.*[\\/]/, '');
  if (!name) return '';
  return /\.[a-z0-9]+$/i.test(name) ? name : `${name}.exe`;
}

function IgnoredApps({ value, save }: { value: string[]; save: SaveFn }) {
  const { t } = useApp();
  const c = t.settings.capture;
  const [draft, setDraft] = useState('');
  const [picking, setPicking] = useState(false);
  const [seen, setSeen] = useState<SeenApp[] | null>(null);

  const has = (exe: string) => value.some((v) => v.toLowerCase() === exe.toLowerCase());
  const add = async (raw: string) => {
    const exe = normaliseExe(raw);
    if (!exe || has(exe)) return setDraft('');
    if (await save({ ignoredApps: [...value, exe] })) setDraft('');
  };
  const remove = (exe: string) => void save({ ignoredApps: value.filter((v) => v !== exe) });

  useEffect(() => {
    if (!picking || seen) return;
    api.listSeenApps().then(setSeen).catch(() => setSeen([]));
  }, [picking, seen]);

  const candidates = (seen ?? []).filter((a) => !has(a.exe));

  return (
    <div className="srow srow--stacked">
      <div className="srow-text">
        <span className="srow-label">{c.ignored}</span>
        <span className="srow-hint">{c.ignoredHint}</span>
      </div>
      <div className="ignored">
        <ul className="tags">
          {value.length === 0 && <li className="tags-empty">{c.ignoredEmpty}</li>}
          {value.map((exe) => (
            <li key={exe} className="tag">
              <span className="mono">{exe}</span>
              <button type="button" aria-label={`${c.remove} ${exe}`} title={c.remove} onClick={() => remove(exe)}>
                <X size={12} strokeWidth={2} />
              </button>
            </li>
          ))}
        </ul>
        <div className="ignored-add">
          <input
            type="text"
            value={draft}
            placeholder={c.ignoredAdd}
            spellCheck={false}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') {
                e.preventDefault();
                void add(draft);
              }
            }}
          />
          <button type="button" className="btn" onClick={() => void add(draft)} disabled={!draft.trim()}>
            <Plus size={14} strokeWidth={1.75} aria-hidden="true" />
            {t.settings.add}
          </button>
          <button type="button" className={`btn btn--quiet${picking ? ' is-pressed' : ''}`} onClick={() => setPicking((p) => !p)}>
            {c.ignoredPick}
          </button>
        </div>
        {picking && (
          <ul className="picker">
            {seen === null && <li className="picker-empty">…</li>}
            {seen !== null && candidates.length === 0 && <li className="picker-empty">{c.pickEmpty}</li>}
            {candidates.map((a) => (
              <li key={a.exe}>
                <button type="button" onClick={() => void add(a.exe)}>
                  {a.icon ? <img className="app-icon" src={assetUrl(a.icon)} alt="" /> : <span className="app-icon app-icon--blank" />}
                  <span className="picker-name">{a.name}</span>
                  <span className="picker-exe mono">{a.exe}</span>
                  <span className="picker-count tnum">{a.count}</span>
                  <Plus size={14} strokeWidth={1.75} aria-hidden="true" />
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}

// ---- Paste ----------------------------------------------------------------------------

function Paste({ settings, save }: { settings: Settings; save: SaveFn }) {
  const { t } = useApp();
  const p = t.settings.paste;
  const keys: [string[], string][] = [
    [['↵'], p.keyPaste],
    [['Shift', '↵'], p.keyPlain],
    [['Ctrl', '1–9'], p.keyQuick],
    [['Alt', '↵'], p.keyDetail],
    [['Ctrl', 'C'], p.keyCopy],
    [['Ctrl', 'S'], p.keyFav],
    [['Ctrl', 'P'], p.keyPin],
    [['Ctrl', 'Del'], p.keyDelete],
    [['Tab'], p.keyTab],
    [['Esc'], p.keyEsc],
  ];
  return (
    <>
      <PageHead title={p.title} lead={p.lead} />
      <Group title={p.behavior}>
        <Row label={p.autoPaste} hint={p.autoPasteHint} htmlFor="s-autopaste">
          <Switch id="s-autopaste" checked={settings.autoPaste} onChange={(autoPaste) => void save({ autoPaste })} />
        </Row>
        <Row label={p.plain} hint={p.plainHint} htmlFor="s-plain">
          <Switch id="s-plain" checked={settings.pastePlain} onChange={(pastePlain) => void save({ pastePlain })} />
        </Row>
        <Row label={p.moveToTop} htmlFor="s-top">
          <Switch id="s-top" checked={settings.moveToTop} onChange={(moveToTop) => void save({ moveToTop })} />
        </Row>
      </Group>
      <Group title={p.keys}>
        <dl className="keymap">
          {keys.map(([combo, label]) => (
            <div key={label} className="keymap-row">
              <dt>
                {combo.map((k) => (
                  <kbd key={k} className="kbd kbd--lg">
                    {k}
                  </kbd>
                ))}
              </dt>
              <dd>{label}</dd>
            </div>
          ))}
        </dl>
      </Group>
    </>
  );
}

// ---- Data -----------------------------------------------------------------------------

function Data({ notify }: { notify: (text: string) => void }) {
  const { t, lang } = useApp();
  const d = t.settings.data;
  const [stats, setStats] = useState<Stats | null>(null);
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [keepMarked, setKeepMarked] = useState(true);
  const n = (v: number) => formatNumber(v, lang);

  const load = useCallback(() => {
    api.getStats().then(setStats).catch(() => {});
  }, []);

  useEffect(() => {
    load();
    api.getAppInfo().then(setInfo).catch(() => {});
    const off = api.on('history://changed', load);
    return () => void off.then((f) => f());
  }, [load]);

  const toDelete = stats ? (keepMarked ? stats.total - stats.marked : stats.total) : 0;

  const clear = async () => {
    try {
      const count = await api.clearHistory(keepMarked);
      setConfirming(false);
      notify(d.cleared(n(count)));
      load();
    } catch (e) {
      notify(t.errors[toCmdError(e).code] ?? t.toast.failed);
    }
  };

  const tiles: [string, ReactNode][] = stats
    ? [
        [d.total, n(stats.total)],
        [d.text, n(stats.text)],
        [d.image, n(stats.image)],
        [d.files, n(stats.files)],
        [d.favorite, n(stats.favorite)],
        [d.storage, formatBytes(stats.dbBytes + stats.imageBytes, lang)],
      ]
    : [];

  return (
    <>
      <PageHead title={d.title} lead={d.lead} />
      <Group title={d.overview}>
        <dl className="stats">
          {(stats ? tiles : Array.from({ length: 6 }, (_, i) => [String(i), '—'] as [string, ReactNode])).map(
            ([label, value]) => (
              <div key={label} className="stat">
                <dd className="tnum">{value}</dd>
                <dt>{stats ? label : ' '}</dt>
              </div>
            ),
          )}
        </dl>
      </Group>

      <Group title={d.location}>
        <div className="srow">
          <div className="srow-text">
            <span className="path mono" title={info?.dataDir}>
              {info?.dataDir ?? '…'}
            </span>
          </div>
          <div className="srow-control">
            <button type="button" className="btn" onClick={() => void api.openDataDir()}>
              <FolderOpen size={14} strokeWidth={1.75} aria-hidden="true" />
              {d.openFolder}
            </button>
          </div>
        </div>
      </Group>

      <Group title={d.danger}>
        <Row label={d.clear} hint={d.clearHint}>
          {!confirming && (
            <button type="button" className="btn btn--danger-outline" onClick={() => setConfirming(true)} disabled={!stats?.total}>
              {d.clear}…
            </button>
          )}
        </Row>
        {confirming && (
          <div className="confirm" role="alertdialog" aria-label={d.clear}>
            <label className="check">
              <input type="checkbox" checked={keepMarked} onChange={(e) => setKeepMarked(e.target.checked)} />
              <span>{d.keepMarked}</span>
            </label>
            <p className="confirm-text">{d.clearConfirm(n(toDelete))}</p>
            <div className="confirm-actions">
              <button type="button" className="btn" onClick={() => setConfirming(false)} autoFocus>
                {d.cancel}
              </button>
              <button type="button" className="btn btn--danger" onClick={() => void clear()} disabled={toDelete === 0}>
                {d.clearAction}
              </button>
            </div>
          </div>
        )}
      </Group>
    </>
  );
}

// ---- About ----------------------------------------------------------------------------

function About() {
  const { t } = useApp();
  const a = t.settings.about;
  const [info, setInfo] = useState<AppInfo | null>(null);
  useEffect(() => {
    api.getAppInfo().then(setInfo).catch(() => {});
  }, []);

  const links: [string, string][] = [
    [a.repo, REPO],
    [a.issues, `${REPO}/issues`],
    [a.license, `${REPO}/blob/main/LICENSE`],
  ];

  return (
    <div className="about">
      <Seal size={72} />
      <h1 className="about-name">Copybook</h1>
      <p className="about-version tnum">{info ? a.version(info.version) : ' '}</p>
      <p className="about-tagline">{a.tagline}</p>
      {info?.elevated && <p className="about-badge">{a.elevated}</p>}
      <ul className="about-links">
        {links.map(([label, url]) => (
          <li key={url}>
            <button type="button" className="link" onClick={() => void api.openUrl(url)}>
              {label}
              <ExternalLink size={12} strokeWidth={1.75} aria-hidden="true" />
            </button>
          </li>
        ))}
      </ul>
      <p className="about-credit">
        {a.creditLead}
        <button type="button" className="link" onClick={() => void api.openUrl('https://github.com/EcoPasteHub/EcoPaste')}>
          EcoPaste
        </button>
        {a.creditTail}
      </p>
    </div>
  );
}
