# Copybook — frontend/backend contract (v1.5.0)

## v1.5 favorite folders (take precedence below)

* Favorite groups are persistent virtual folder paths separated by `/`, with up to 32
  levels and 1..40 trimmed, nonempty Unicode characters per component; control characters
  are rejected. Folder identity is case-sensitive. Existing flat groups migrate to folders;
  literal slashes in old names become full-width slashes, with a suffix on name collisions.
* `list_groups({}) -> string[]` lists every folder path, including empty folders and parents.
  `create_group({ name }) -> void` creates a folder and missing ancestors. An existing
  destination is rejected. The TypeScript method is `createGroup(name)`.
* `set_item_group({ ids, group }) -> number` atomically moves items to a folder and favorites
  them, creating missing ancestors. Null or empty means the Favorites root and also favorites
  items. Unfavoriting clears the item path, preserving the empty folder.
* `rename_group({ name, replacement }) -> number` atomically renames/moves a folder and its
  whole subtree, including empty folders and temporarily deleted records. Reject moves into
  itself, path-limit violations and existing destinations. Null removes the entire subtree,
  moves its items to the Favorites root and preserves favorite flags. Returns affected items.
* `ListQuery.group` remains exact: null includes all folders, empty includes root favorites,
  and a path includes only its direct items. Search, source/date filters, pagination and
  select-all all use that same scope. Folder rows are never selected or merged as clip items.
* Favorites opens at its root, showing direct child folders followed by root items.
  Breadcrumbs and Up/Escape navigate levels; All favorites shows every favorite. Changing
  category clears the folder scope. The standalone Groups button and group-management
  dialog are removed. Folder menus provide New subfolder, Rename (F2), Move and Remove;
  item menus and the selection toolbar offer Move to folder with a hierarchical destination.
* Backup JSON adds optional `folders: string[]` (default empty for old backups). New backups
  preserve empty directories. Folder validation and insertion share the record transaction,
  so a failed restore changes neither items nor folders. The encrypted format stays v1.


## v1.4.1 fixes (take precedence below)

* WebDAV uses a `copybook/` child of the configured collection. If the configured URL
  already ends with `copybook/` (case-insensitive), use it directly. MKCOL creates the
  child automatically; new backups are written only there. Backup entries additionally
  have `legacy: boolean`; listing includes v1.4.0 backups from the configured parent
  without moving/deleting them. `webdav_restore` accepts optional `legacy` (default false)
  to read that parent; every filename is still validated, and requests stay in these two
  collections. The UI identifies legacy backups separately.
* A paste target is the actual foreground top-level window (`GA_ROOT`), including an owned
  dialog or Copybook settings, with its process/thread identity and focused child captured
  before showing the panel. Only the history panel itself and system shell surfaces are
  excluded. Reopening while the panel is already foreground preserves the saved target.
* Before injecting Ctrl+V once, restore the target and its still-valid focused child, hide
  the panel, wait for held Ctrl/Shift/Alt/Win and mouse buttons to release (<= 1 s), then
  require stable target/input focus. Never paste into a window whose identity changed or
  into a different foreground window. Windowed/Chromium inputs retain their text selection.


## v1.4 additions (take precedence over the v1 descriptions below)

* `ClipItem.group: string | null`: optional favorite group (1..40 trimmed characters).
  Assigning a group also favorites the item; unfavoriting clears its group. Groups are
  names stored on items; renaming/deleting a group updates its members atomically.
* `ListQuery` additionally accepts optional nullable `sourceExe`, `since`, `until`, `group`.
  Source matches an exact executable name ignoring case. Time filters apply to `lastUsedAt`,
  with inclusive `since` and exclusive `until` (Unix milliseconds). Empty group means
  ungrouped favorites. All filters combine with AND and apply before pagination.
* `list_item_ids({ query }) -> number[]` returns ALL matching IDs in list order, bounded
  to 100000. The UI labels select-all as the complete current search/filter result;
  selection is a fixed ID snapshot, cleared when filters change.
* `delete_items({ ids }) -> number` now hides a batch for 15 seconds; `undo_delete({}) ->
  number` restores the latest batch, including original image files and metadata. A new
  deletion finalizes the previous batch. `clear_history` and retention cleanup remain
  permanent. Tombstones are purged after expiry and on application start. Recapturing a
  hidden item revives it without duplication. Hidden items cannot be read/copied/backed up.
* `get_merged_text({ ids, separator }) -> string` and `copy_merged({ ids, separator }) ->
  void` accept 1..1000 distinct, existing text items, preserve requested order, use FULL
  text (never list previews), and cap the result at 5 MB UTF-8, separator at 100 chars.
  The UI previews before copying; images/files are rejected rather than silently omitted.
* `list_groups({}) -> string[]`, `set_item_group({ ids, group }) -> number`,
  `rename_group({ name, replacement }) -> number` (null replacement ungroups members,
  preserves favorite); names are trimmed and limited to 40 characters.
* WebDAV commands: `webdav_test({ connection }) -> void`,
  `webdav_list({ connection }) -> BackupEntry[]`,
  `webdav_backup({ connection, password }) -> BackupEntry`,
  `webdav_restore({ connection, name, password, restoreSettings }) -> RestoreResult`.
  Connection = `{ url, username, password }`; no credentials or encryption passwords are
  persisted, backed up, returned or logged. The UI may remember URL/username only locally.
  HTTPS is mandatory except HTTP on loopback for local servers/tests. TLS is validated,
  redirects disabled; all requests stay in the configured collection. The collection must
  already exist. Backups have an opaque timestamp/random `.cpbk` filename.
* BackupEntry = `{ name: string, bytes: number | null }`.
  RestoreResult = `{ imported: number, skipped: number, settingsRestored: boolean | null }`.
  Null means preference restore was not requested; false reports that records imported but
  applying preferences failed (the UI must say so). Restore merges; existing content
  wins (no overwrite), all new entries commit atomically. Images are embedded portably;
  file entries back up references, not the referenced files. Source icons are omitted.
  Schema version, format, sizes and image dimensions are validated before import; a wrong
  password or corrupt backup changes nothing. Limit: 256 MiB decrypted JSON, 100000 records.
* Backup format v1: `CPBK0001` + 16 random salt bytes + 12 random nonce bytes + AES-256-GCM
  ciphertext/tag. Header is authenticated AAD. Argon2id v1.3 derives a 32-byte key with
  65536 KiB memory, 3 iterations, parallelism 1; plaintext is gzip-compressed JSON. Password
  length 8..1024 UTF-8 bytes. Every backup uses independent OS randomness. Encryption is
  always on. Password loss cannot be recovered. Cloud sees ciphertext, file size and name.
  Optional settings restore applies validated appearance/capture/paste/data preferences,
  preserving this machine's hotkey, Win+V, autostart and elevation choices.
* New error codes: `auth` (WebDAV authentication), `crypto` (wrong password or tampering),
  `too_large` (size/item limits). Errors never contain URL credentials or server response
  bodies. Backend history changes continue to use `updated`/`deleted`/`added` reasons.
* Panel multi-select: explicit checkbox mode, Ctrl+click toggle, Shift+click range,
  Shift+arrow extend, Space toggle focused item, Ctrl+A select all matching results;
  Ctrl+A in a text input selects text except in explicit multi-select mode. Escape exits
  dialogs first, then multi-select. Single/double click cannot paste in multi-select mode.
  Marked-item deletion asks for confirmation; UI undo action lasts 15 seconds.


This file is the single source of truth between the web frontend (`src/`, written by Claude)
and the Rust backend (`src-tauri/`, written by Codex). Any change to a name, payload or
behaviour below is made here first, then on both sides. The TypeScript mirror of these
types lives in `src/shared/contract.ts`; the Rust mirror must serialize to exactly the same
JSON (`#[serde(rename_all = "camelCase")]`, enums as the lowercase strings shown).

Windows 10/11 only. No macOS or Linux code paths.

## 1. Identity and files

| What | Value |
| --- | --- |
| productName | `Copybook` |
| identifier | `io.github.wio-ki.copybook` |
| version | `1.4.1` (single source: `package.json`, `tauri.conf.json` reads it) |
| data dir | Tauri `app_data_dir()` = `%APPDATA%\io.github.wio-ki.copybook\` |
| database | `<data>\copybook.db` (SQLite, WAL) |
| settings | `<data>\settings.json` |
| window state | `<data>\state.json` (panel position and size; backend only) |
| images | `<data>\images\<id>.png` (original) and `<data>\images\<id>.thumb.png` |
| app icons | `<data>\icons\<hash>.png` (source-app icons, 32×32 or larger) |
| logs | Tauri log dir, plugin-log, level info |

The frontend shows images and app icons through the asset protocol
(`convertFileSrc(path)`), so `tauri.conf.json` must enable it:

```jsonc
"security": {
  "assetProtocol": { "enable": true, "scope": ["$APPDATA/images/**", "$APPDATA/icons/**"] },
  "csp": "default-src 'self'; img-src 'self' asset: http://asset.localhost data: blob:; style-src 'self' 'unsafe-inline'; font-src 'self' data:; connect-src ipc: http://ipc.localhost"
}
```

## 2. Windows

Both windows are created at startup, hidden. The frontend draws all chrome itself.

| label | url | size (logical) | min | flags |
| --- | --- | --- | --- | --- |
| `panel` | `panel.html` | 384 × 576 | 320 × 420 | decorations false, shadow true, resizable true, alwaysOnTop true, skipTaskbar true, visible false, focus true |
| `settings` | `settings.html` | 760 × 560 | 640 × 480 | decorations false, shadow true, resizable true, skipTaskbar false, visible false, center true |

`shadow: true` on an undecorated window gives the Windows 11 rounded corners and native
shadow; do not make either window transparent.

Capabilities for both windows must allow at least: `core:default`,
`core:window:allow-start-dragging`, `core:window:allow-minimize`,
`core:window:allow-toggle-maximize`, `core:window:allow-hide`, `core:window:allow-close`,
`core:window:allow-set-focus`, `core:event:default`, and all commands in section 5.

Closing the settings window (title-bar X calls `getCurrentWindow().close()`) only hides it.
The app quits only from the tray menu.

### Panel lifecycle

* **Show** (`toggle_panel` path): if the panel is visible and focused, hide it (with focus
  restore). Otherwise: remember the current foreground window as the *paste target* unless it
  belongs to Copybook; position the panel (below); show it; bring it to the foreground with
  keyboard focus (reliably, including when triggered by a global hotkey or Win+V); then emit
  `panel://shown`.
* **Position** by `settings.panelPosition`, on the monitor that contains the mouse cursor,
  always clamped inside that monitor's work area, DPI-aware:
  * `cursor`: top-left corner 8 px right/below the cursor; flip to the left/up side when
    the panel would not fit.
  * `center`: centred in the work area.
  * `remember`: the position saved in `state.json` when the panel was last moved; if it is
    not on any connected monitor, fall back to `cursor`.
* **Size**: the user may resize the panel; persist the size in `state.json` and reuse it.
* **Hide**:
  * `hide_panel(restoreFocus)` command (Esc in the UI calls it with `true`; it then
    re-activates the paste target).
  * Focus loss, when `settings.hideOnBlur` is true and the panel is not locked
    (`set_panel_locked`). Ignore blur events during the first 150 ms after showing and while
    the user is dragging or resizing the panel. No focus restore here (the user clicked
    elsewhere on purpose).
  * Inside `paste_item`.
  * Every hide emits `panel://hidden`.
* The panel lock is runtime state only, reset to `false` on app start.

## 3. Types

```ts
type ClipKind = 'text' | 'image' | 'files';
type SubKind = 'url' | 'email' | 'color' | 'path';   // only for kind 'text'

interface SourceApp {
  name: string;          // display name: exe FileDescription, else exe stem, e.g. "Visual Studio Code"
  exe: string;           // file name, e.g. "Code.exe"
  icon: string | null;   // absolute path of the cached PNG in <data>\icons, or null
}

interface ImageInfo {
  path: string;          // absolute path of the original PNG
  thumb: string;         // absolute path of the thumbnail PNG (longest side <= 480 px; may equal path)
  width: number;         // original pixels
  height: number;
  bytes: number;         // original PNG file size
}

interface FileEntry {
  path: string;
  name: string;          // last path component
  isDir: boolean;
  exists: boolean;       // checked when the item is returned
  bytes: number | null;  // size for existing files, null for folders / missing
}

interface ClipItem {
  id: number;                 // SQLite INTEGER PRIMARY KEY
  kind: ClipKind;
  subKind: SubKind | null;
  text: string;               // text: first 2000 chars of the plain text; files: paths joined by "\n"; image: ""
  textLength: number;         // char count (Unicode scalar values) of the full plain text; 0 for image
  lineCount: number;          // line count of the full plain text; 0 for image
  hasHtml: boolean;           // rich formats stored alongside the text (kind 'text' only)
  hasRtf: boolean;
  image: ImageInfo | null;    // kind 'image' only
  files: FileEntry[] | null;  // kind 'files' only, in clipboard order
  source: SourceApp | null;   // app that put the content on the clipboard, if known
  favorite: boolean;
  pinned: boolean;
  note: string | null;
  useCount: number;           // 1 on capture, +1 per re-copy / paste from history
  createdAt: number;          // unix ms, first capture
  lastUsedAt: number;         // unix ms, last capture / copy / paste (sort key)
}

type ListFilter = 'all' | 'text' | 'image' | 'files' | 'url' | 'favorite';

interface ListQuery {
  query: string;    // raw search box text
  filter: ListFilter;
  offset: number;   // >= 0
  limit: number;    // 1..200
}

interface ListResult {
  items: ClipItem[];
  total: number;    // number of items matching query+filter (ignoring offset/limit)
}

interface ItemContent {
  text: string;        // full plain text, cut at maxChars
  truncated: boolean;  // true when cut
  html: string | null; // HTML fragment (CF_HTML fragment only, no header), null if none
}

interface PasteOutcome {
  pasted: boolean;     // true when Ctrl+V was sent to the paste target
  reason: null | 'paste_disabled' | 'no_target' | 'elevated_target';
}

interface Stats {
  total: number; text: number; image: number; files: number; favorite: number;
  pinned: number;
  marked: number;      // favorite OR pinned (what clear_history keeps with keepMarked)
  dbBytes: number;     // database file(s) size incl. WAL
  imageBytes: number;  // sum of <data>\images
}

interface AppInfo {
  version: string;
  dataDir: string;
  elevated: boolean;   // Copybook itself runs as administrator
}

interface UpdateInfo {
  current: string;     // running version, e.g. "1.0.0"
  latest: string;      // newest published release tag without the "v", e.g. "1.1.0"
  newer: boolean;      // true when latest is a strictly higher version than current
  url: string;         // the release page on GitHub
  installer: string | null;  // direct download of the NSIS installer, when the release has one
}

interface UpdateProgress {
  stage: 'idle' | 'downloading' | 'installing' | 'error';
  version: string | null;
  downloaded: number;      // bytes received, cumulative
  total: number | null;    // null when the server omits Content-Length
}

interface SeenApp extends SourceApp { count: number }   // distinct sources in history

interface Settings {
  language: 'system' | 'zh-CN' | 'en-US';
  theme: 'system' | 'light' | 'dark';
  density: 'comfortable' | 'compact';   // frontend only, backend just stores it
  showSource: boolean;       // show source app name/icon in history rows and details; capture unchanged
  autostart: boolean;
  runAsAdmin: boolean;       // persistent elevation preference, independent of current token
  hotkey: string;            // see "Hotkey strings"; '' = no hotkey
  winV: boolean;             // take over Win+V
  panelPosition: 'cursor' | 'center' | 'remember';
  hideOnBlur: boolean;
  autoPaste: boolean;        // paste_item sends Ctrl+V (true) or only copies (false)
  pastePlain: boolean;       // frontend default for Enter (backend just stores it)
  moveToTop: boolean;        // copy/paste from history bumps lastUsedAt
  captureText: boolean;
  captureRich: boolean;      // also store HTML / RTF next to the text
  captureImages: boolean;
  captureFiles: boolean;
  historyLimit: number;      // 0 = unlimited, else 50..100000; favorites and pinned never count or get removed
  retentionDays: number;     // 0 = forever, else 1..3650; favorites and pinned exempt
  maxImageMB: number;        // 1..200; larger images are not captured
  ignoredApps: string[];     // exe file names, case-insensitive, e.g. "KeePass.exe"
  respectPrivacyHints: boolean;
  skipSecrets: boolean;
}

interface CmdError {
  code: 'not_found' | 'invalid' | 'hotkey_invalid' | 'hotkey_conflict' | 'io' | 'db' | 'network' | 'internal';
  message: string;     // English, for logs; the UI maps code to its own text
}
```

Default settings (also the fallback for every missing or invalid key in `settings.json`):

```json
{
  "language": "system", "theme": "system", "density": "comfortable", "showSource": true,
  "autostart": false, "runAsAdmin": false, "hotkey": "Alt+KeyV", "winV": false,
  "panelPosition": "cursor", "hideOnBlur": true,
  "autoPaste": true, "pastePlain": false, "moveToTop": true,
  "captureText": true, "captureRich": true, "captureImages": true, "captureFiles": true,
  "historyLimit": 1000, "retentionDays": 0, "maxImageMB": 20,
  "ignoredApps": ["KeePass.exe", "KeePassXC.exe", "1Password.exe", "Bitwarden.exe"],
  "respectPrivacyHints": true, "skipSecrets": true
}
```

On the very first start (no `settings.json` yet) show the settings window, so the user sees
the hotkey.

`runAsAdmin` is a switch. Enabling saves the preference and requests an elevated restart
unless already elevated; cancellation/failure restores the previous preference. Every later
launch (including `--autostart`) checks it before starting capture and requests UAC when
needed. A declined startup prompt exits instead of running unelevated with the switch on.
The elevated replacement preserves the data directory and silent startup; only an explicit
settings restart reopens General. Disabling takes effect on the next launch; the current
process keeps its Windows token until it exits.

### Hotkey strings

`Modifier+...+Key`: modifiers from `Ctrl`, `Alt`, `Shift`, `Super`, in that order; key is a
`KeyboardEvent.code` value (`KeyV`, `Digit1`, `F9`, `Space`, `Backquote`, `ArrowUp`, ...).
At least one modifier is required unless the key is `F1`..`F24`. The frontend's recorder
produces these strings; the backend parses them for the global-shortcut plugin and rejects
anything else with `hotkey_invalid`.

## 4. Capture rules (backend)

* Listen with `AddClipboardFormatListener` on a message-only window in its own thread.
  Debounce bursts (about 100 ms) and read the clipboard once per burst; retry `OpenClipboard`
  briefly when another process holds it.
* Skip when capture is paused, when the change came from Copybook's own write (compare
  `GetClipboardSequenceNumber`), when the owner process (`GetClipboardOwner`, else the
  foreground window) is in `ignoredApps` or is Copybook, and, if `respectPrivacyHints`, when
  the clipboard holds `ExcludeClipboardContentFromMonitorProcessing`,
  `Clipboard Viewer Ignore`, or `CanIncludeInClipboardHistory` / `CanUploadToCloudClipboard`
  with DWORD 0.
* Pick one kind per change, first match wins: **files** (`CF_HDROP`, if `captureFiles`) →
  **text** (`CF_UNICODETEXT` with non-whitespace content, if `captureText`; keep the
  `HTML Format` fragment and `Rich Text Format` too when `captureRich`) → **image** (`PNG`
  format if present, else `CF_DIBV5`, else `CF_DIB`, if `captureImages` and within
  `maxImageMB`) → **text from HTML only** (tags stripped) when there is HTML but no text and
  no image. Anything else is ignored.
* `skipSecrets`: drop text that is, with high confidence, a secret: PEM private key blocks,
  GitHub tokens (`ghp_`, `gho_`, `ghu_`, `ghs_`, `ghr_`, `github_pat_`), AWS access key ids
  (`AKIA`/`ASIA` + 16), Slack tokens (`xox[abprs]-`), OpenAI/Anthropic style keys
  (`sk-` / `sk-ant-` followed by 20+ key chars), JWTs (`eyJ…`.`eyJ…`.sig). Only when the
  whole trimmed text is one such token or one PEM block; never drop ordinary prose that
  merely mentions one.
* Dedupe: same kind + same content hash (text: the plain text; image: decoded pixels plus
  size; files: the joined paths) updates the existing row instead of inserting: refresh rich
  formats and source, `lastUsedAt = now`, `useCount += 1`. Favorite, pinned and note stay.
* `subKind` for text, on the trimmed text: `url` (single http/https URL, or `www.` host, no
  whitespace), `email` (single address), `color` (one CSS colour: `#rgb[a]`, `#rrggbb[aa]`,
  `rgb[a](...)`, `hsl[a](...)`), `path` (one absolute Windows path, `C:\…` or `\\server\…`,
  optionally in double quotes). Otherwise null.
* Images: save the original as PNG and a thumbnail (longest side <= 480 px) under
  `<data>\images`. Delete both files with the row.
* After each capture, enforce `historyLimit`; enforce `retentionDays` at start and hourly.
  Emit `history://changed` after every change that the list could show.

## 5. Commands (`invoke`)

Arguments are passed as one object with these camelCase keys. Every command returns
`Result<T, CmdError>`.

| command | args | returns | notes |
| --- | --- | --- | --- |
| `list_items` | `{ query: ListQuery }` | `ListResult` | see Search |
| `get_item` | `{ id }` | `ClipItem` | `not_found` if missing |
| `get_item_content` | `{ id, maxChars }` | `ItemContent` | `maxChars` 1..5 000 000 |
| `paste_item` | `{ id, plain }` | `PasteOutcome` | see Paste |

Panel keyboard (frontend): quick paste is `Ctrl+1..9` or `Ctrl+Shift+1..9`; some apps hold
`Ctrl+1..3` as global hotkeys, which swallow them system-wide.
| `copy_item` | `{ id, plain }` | `void` | writes the clipboard only; bumps if `moveToTop`; panel stays |
| `set_favorite` | `{ id, value }` | `ClipItem` | |
| `set_pinned` | `{ id, value }` | `ClipItem` | |
| `set_note` | `{ id, note }` | `ClipItem` | trims; `''`/null clears; max 500 chars else `invalid` |
| `delete_items` | `{ ids }` | `number` | deleted count |
| `clear_history` | `{ keepMarked }` | `number` | `keepMarked` keeps favorite or pinned items |
| `get_stats` | `{}` | `Stats` | |
| `list_seen_apps` | `{}` | `SeenApp[]` | distinct `source` apps in history, most frequent first |
| `get_settings` | `{}` | `Settings` | |
| `update_settings` | `{ patch }` | `Settings` | partial; validates every key; applies side effects (hotkey, Win+V hook, autostart, tray language, limits); on hotkey failure keeps the old hotkey registered, changes nothing, returns `hotkey_invalid` / `hotkey_conflict`; emits `settings://changed` to both windows |
| `get_capture_paused` | `{}` | `boolean` | |
| `set_capture_paused` | `{ paused }` | `void` | also updates the tray check item; emits `capture://paused` |
| `hide_panel` | `{ restoreFocus }` | `void` | |
| `set_panel_locked` | `{ locked }` | `void` | locked panel ignores blur |
| `open_settings` | `{ section }` | `void` | `section` null or one of `general`, `capture`, `paste`, `data`, `about`; shows + focuses the settings window, then emits `settings://navigate` when a section is given |
| `open_url` | `{ url }` | `void` | only `http:`, `https:`, `mailto:`; else `invalid` |
| `open_path` | `{ path }` | `void` | default app; `not_found` if missing |
| `reveal_path` | `{ path }` | `void` | Explorer with the item selected |
| `open_data_dir` | `{}` | `void` | |
| `get_app_info` | `{}` | `AppInfo` | |
| `check_update` | `{}` | `UpdateInfo` | user-triggered check of the official signed Tauri update feed; caches a newer update for installation; never downloads an installer by itself; network failures are `network` |
| `install_update` | `{}` | `void` | downloads the last checked newer update, verifies its signature and signed version, then runs the Windows updater; no browser, manual uninstall or installer steps; exits only after the updater starts; errors retain the pending update for retry, keep the app running and publish error progress |
| `get_update_progress` | `{}` | `UpdateProgress` | current process-wide download/install status, including after navigating away from About |
| `restart_as_admin` | `{}` | `boolean` | requests UAC and restarts elevated; `false` on cancellation or if already elevated; `io` on launch failure; current instance exits only after a successful launch |

### Search (`list_items`)

* Trim `query`; split on whitespace into terms; every term must match (AND),
  case-insensitively, as a substring of the full plain text, the note, or (files) any path.
* Use an FTS5 table with the `trigram` tokenizer for terms of 3+ characters and a `LIKE`
  scan (with escaped `%`, `_`, `\`) for shorter terms, so 1–2 character Chinese queries work.
* Filters: `text`, `image`, `files` by kind; `url` = kind text and subKind url;
  `favorite` = favorite items.
* Order: pinned first, then `lastUsedAt` desc, then `id` desc. Stable across pages.

### Paste (`paste_item`)

1. Write the item to the clipboard: text as `CF_UNICODETEXT`, plus `HTML Format` and
   `Rich Text Format` when stored and `plain` is false; image as `CF_DIB`, `CF_DIBV5` and `PNG`;
   files as `CF_HDROP` plus `Preferred DropEffect` = copy (with `plain`, the paths as text).
   Record the sequence number so the watcher ignores this write. Bump if `moveToTop`.
   For image auto-paste into terminal hosts (including Codex CLI / Claude Code CLI), also
   supply `CF_UNICODETEXT` containing one double-quoted absolute PNG path, without newline.
   Keep an independent PNG copy in `<data>\paste-images\<sha256>.png` so removing history
   does not invalidate a pending CLI attachment. Terminal Ctrl+V then delivers a file path
   even when the terminal intercepts the CLI's image shortcut; native image formats remain
   available. Copy-only and non-terminal paste keep image-only clipboard semantics.
2. If `autoPaste` is false: hide with focus restore, return `{ pasted: false, reason: 'paste_disabled' }`.
3. If there is no valid paste target: hide, return `no_target`.
4. If the target process is elevated and Copybook is not: keep the panel open and return
   `elevated_target` (the clipboard already holds the item; the UI tells the user).
5. Activate the target while the panel still has the foreground, hide the panel, wait until
   the target is the foreground window (<= 300 ms), wait until Shift / Alt / Win are released
   (<= 1 s), give the target time to restore its focused control (80 ms), send Ctrl+V with
   `SendInput`, return `{ pasted: true, reason: null }`. Ctrl may stay held (quick paste).

## 6. Events (`emit` to all windows)

| event | payload | when |
| --- | --- | --- |
| `history://changed` | `{ reason: 'added' \| 'bumped' \| 'updated' \| 'deleted' \| 'cleared' \| 'cleanup', id: number \| null }` | any change to items |
| `panel://shown` | `{ locked: boolean }` | after the panel is shown and focused |
| `panel://hidden` | `{}` | after the panel is hidden |
| `settings://changed` | `Settings` | after `update_settings` succeeds |
| `settings://navigate` | `{ section: string }` | from `open_settings` |
| `capture://paused` | `{ paused: boolean }` | pause state changed (command or tray) |
| `update://progress` | `UpdateProgress` | update download starts/progresses, verified installation starts, or a failure occurs |

## 7. Shell integration (backend)

* **Global hotkey** via `tauri-plugin-global-shortcut`: register `settings.hotkey` at start;
  pressing it toggles the panel. A failed registration at start is logged, not fatal.
* **Win+V** (`winV`): a `WH_KEYBOARD_LL` hook, installed only while the setting is on, that
  takes over the bare chord only — V with a Win key held and no Ctrl, Alt or Shift, so that
  other applications' Win+Ctrl+V / Win+Shift+V style shortcuts still reach them. It swallows
  the V key-down (and its repeat and matching key-up), keeps the Start menu from opening by
  injecting a balanced pair of an unassigned virtual key (0xE8, never F24: F24 is a real
  function key that other programs react to), and toggles the panel.
* **Tray**: icon `icons/tray.png` (paused: `icons/tray-paused.png`), tooltip `Copybook`.
  Left click toggles the panel. Menu, in the settings language (`system` follows the OS UI
  language: Chinese if it starts with `zh`, else English):
  `打开面板 / Open panel` (shows the hotkey), `暂停记录 / Pause capture` (check item),
  `偏好设置… / Preferences…`, separator, `退出 / Quit`.
* **App updates** (user-triggered only): use `tauri-plugin-updater` to check
  `https://github.com/wio-ki/Copybook/releases/latest/download/latest.json` (25 s timeout).
  Cache a newer update after validating that it targets this repository's HTTPS Windows
  NSIS asset. `install_update` downloads with a 5-minute timeout, publishes cumulative
  byte progress, and verifies the trusted public-key signature and signed app version.
  Only complete, verified bytes are staged under `<data>\updates`; the staged file is
  locked against changes and compared with the verified bytes before Windows executes it.
  Launch the installer with `/P /UPDATE /R /ARGS --data-dir "<data>" /D=<install-dir>`;
  NSIS `/D` is last and unquoted. Fresh ordinary restart arguments exclude stale UAC PIDs.
  Exit through Tauri only after a successful launch so normal hook/clipboard cleanup runs;
  network, signature or launch failures preserve the running instance and allow retry.
  NSIS update mode replaces the installed binary without uninstalling or deleting data,
  then restarts it. No language-selection prompt or browser download is required.
  Re-checking successfully clears prior error progress. Operation gating rejects concurrent
  checks/installs; process-wide status survives navigation away from About.
  Releases include `latest.json`, the NSIS installer and its detached signature. The private
  signing key is never committed; CI receives it from an encrypted repository secret.
  Nothing is sent on a timer: only a user click initiates a check or download.
* **Source visibility** (`showSource`, default true): General → Panel controls app name/icon
  display in history rows and item details, including existing entries. Timestamps, item
  metadata, capture, ignored-app selection and stored source information remain available.
* **Autostart** via `tauri-plugin-autostart` (HKCU Run). Starting at login never shows a window.
* **Run as administrator**: Preferences → General offers the persistent `runAsAdmin` switch
  described above and shows the current elevation status using `get_app_info`.
  `restart_as_admin` uses Windows `runas` for the current
  executable; cancelling UAC leaves the current instance running. The elevated replacement
  waits for the original process to exit before initializing the single-instance plugin,
  database, hooks and shortcuts, then reopens General. The same data directory is carried
  across the restart (including `COPYBOOK_DATA_DIR` overrides and updater `--data-dir`
  arguments). Future launches honor the saved switch, including after an update.
* **Single instance** via `tauri-plugin-single-instance`: a second launch shows the panel.
