// TypeScript mirror of docs/contract.md. Keep the two in sync; the contract wins.

export type ClipKind = 'text' | 'image' | 'files';
export type SubKind = 'url' | 'email' | 'color' | 'path';

export interface SourceApp {
  name: string;
  exe: string;
  icon: string | null;
}

export interface ImageInfo {
  path: string;
  thumb: string;
  width: number;
  height: number;
  bytes: number;
}

export interface FileEntry {
  path: string;
  name: string;
  isDir: boolean;
  exists: boolean;
  bytes: number | null;
}

export interface ClipItem {
  id: number;
  kind: ClipKind;
  subKind: SubKind | null;
  text: string;
  textLength: number;
  lineCount: number;
  hasHtml: boolean;
  hasRtf: boolean;
  image: ImageInfo | null;
  files: FileEntry[] | null;
  source: SourceApp | null;
  favorite: boolean;
  pinned: boolean;
  note: string | null;
  useCount: number;
  createdAt: number;
  lastUsedAt: number;
}

export type ListFilter = 'all' | 'text' | 'image' | 'files' | 'url' | 'favorite';

export interface ListQuery {
  query: string;
  filter: ListFilter;
  offset: number;
  limit: number;
}

export interface ListResult {
  items: ClipItem[];
  total: number;
}

export interface ItemContent {
  text: string;
  truncated: boolean;
  html: string | null;
}

export interface PasteOutcome {
  pasted: boolean;
  reason: null | 'paste_disabled' | 'no_target' | 'elevated_target';
}

export interface Stats {
  total: number;
  text: number;
  image: number;
  files: number;
  favorite: number;
  pinned: number;
  marked: number;
  dbBytes: number;
  imageBytes: number;
}

export interface UpdateInfo {
  current: string;
  latest: string;
  newer: boolean;
  url: string;
  installer: string | null;
}

export interface UpdateProgress {
  stage: 'idle' | 'downloading' | 'installing' | 'error';
  version: string | null;
  downloaded: number;
  total: number | null;
}

export interface AppInfo {
  version: string;
  dataDir: string;
  elevated: boolean;
}

export interface SeenApp extends SourceApp {
  count: number;
}

export type Language = 'system' | 'zh-CN' | 'en-US';
export type ThemeSetting = 'system' | 'light' | 'dark';
export type Density = 'comfortable' | 'compact';
export type PanelPosition = 'cursor' | 'center' | 'remember';

export interface Settings {
  language: Language;
  theme: ThemeSetting;
  density: Density;
  showSource: boolean;
  autostart: boolean;
  runAsAdmin: boolean;
  hotkey: string;
  winV: boolean;
  panelPosition: PanelPosition;
  hideOnBlur: boolean;
  autoPaste: boolean;
  pastePlain: boolean;
  moveToTop: boolean;
  captureText: boolean;
  captureRich: boolean;
  captureImages: boolean;
  captureFiles: boolean;
  historyLimit: number;
  retentionDays: number;
  maxImageMB: number;
  ignoredApps: string[];
  respectPrivacyHints: boolean;
  skipSecrets: boolean;
}

export type ErrorCode =
  | 'not_found'
  | 'invalid'
  | 'hotkey_invalid'
  | 'hotkey_conflict'
  | 'io'
  | 'db'
  | 'network'
  | 'internal';

export interface CmdError {
  code: ErrorCode;
  message: string;
}

export type SettingsSection = 'general' | 'capture' | 'paste' | 'data' | 'about';

export type HistoryChangeReason = 'added' | 'bumped' | 'updated' | 'deleted' | 'cleared' | 'cleanup';

export interface EventMap {
  'history://changed': { reason: HistoryChangeReason; id: number | null };
  'panel://shown': { locked: boolean };
  'panel://hidden': Record<string, never>;
  'settings://changed': Settings;
  'settings://navigate': { section: SettingsSection };
  'capture://paused': { paused: boolean };
  'update://progress': UpdateProgress;
}

export const DEFAULT_SETTINGS: Settings = {
  language: 'system',
  theme: 'system',
  density: 'comfortable',
  showSource: true,
  autostart: false,
  runAsAdmin: false,
  hotkey: 'Alt+KeyV',
  winV: false,
  panelPosition: 'cursor',
  hideOnBlur: true,
  autoPaste: true,
  pastePlain: false,
  moveToTop: true,
  captureText: true,
  captureRich: true,
  captureImages: true,
  captureFiles: true,
  historyLimit: 1000,
  retentionDays: 0,
  maxImageMB: 20,
  ignoredApps: ['KeePass.exe', 'KeePassXC.exe', '1Password.exe', 'Bitwarden.exe'],
  respectPrivacyHints: true,
  skipSecrets: true,
};
