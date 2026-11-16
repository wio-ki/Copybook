import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from 'react';
import { api } from './api';
import { DEFAULT_SETTINGS, type Language, type Settings } from './contract';
import { STRINGS, type Strings } from './strings';

export type Lang = Exclude<Language, 'system'>;

export function resolveLanguage(setting: Language): Lang {
  if (setting !== 'system') return setting;
  return navigator.language.toLowerCase().startsWith('zh') ? 'zh-CN' : 'en-US';
}

interface AppContextValue {
  settings: Settings;
  ready: boolean;
  lang: Lang;
  t: Strings;
  /** Persist a change; resolves to the saved settings or rejects with a CmdError. */
  update: (patch: Partial<Settings>) => Promise<Settings>;
}

const AppContext = createContext<AppContextValue | null>(null);

export function AppProvider({ children }: { children: ReactNode }) {
  const [settings, setSettings] = useState<Settings>(DEFAULT_SETTINGS);
  const [ready, setReady] = useState(false);

  useEffect(() => {
    let alive = true;
    let unlisten: (() => void) | undefined;
    api
      .getSettings()
      .then((s) => alive && setSettings(s))
      .catch(() => {})
      .finally(() => alive && setReady(true));
    api.on('settings://changed', (s) => setSettings(s)).then((u) => {
      if (alive) unlisten = u;
      else u();
    });
    return () => {
      alive = false;
      unlisten?.();
    };
  }, []);

  // Theme: explicit choices set data-theme; "system" leaves it to the media query in tokens.css.
  useEffect(() => {
    const root = document.documentElement;
    if (settings.theme === 'system') delete root.dataset.theme;
    else root.dataset.theme = settings.theme;
  }, [settings.theme]);

  const lang = resolveLanguage(settings.language);
  useEffect(() => {
    document.documentElement.lang = lang;
  }, [lang]);

  const update = useCallback(async (patch: Partial<Settings>) => {
    const saved = await api.updateSettings(patch);
    setSettings(saved);
    return saved;
  }, []);

  const value = useMemo(
    () => ({ settings, ready, lang, t: STRINGS[lang], update }),
    [settings, ready, lang, update],
  );
  return <AppContext.Provider value={value}>{children}</AppContext.Provider>;
}

export function useApp(): AppContextValue {
  const ctx = useContext(AppContext);
  if (!ctx) throw new Error('useApp outside AppProvider');
  return ctx;
}
