import { CloudUpload, Download, RefreshCw, ShieldCheck } from 'lucide-react';
import { useState } from 'react';
import { api, inTauri, toCmdError } from '../shared/api';
import { useApp } from '../shared/app';
import type { BackupEntry, WebDavConnection } from '../shared/contract';
import { formatBytes } from '../shared/format';
import { FeatureDialog } from '../shared/FeatureDialog';
import { Group } from './controls';

function remembered(): Pick<WebDavConnection, 'url' | 'username'> {
  try {
    const value = JSON.parse(localStorage.getItem('copybook.webdav') ?? '{}');
    return {
      url: typeof value.url === 'string' ? value.url : '',
      username: typeof value.username === 'string' ? value.username : '',
    };
  } catch {
    return { url: '', username: '' };
  }
}
const entryKey = (entry: BackupEntry) => `${entry.legacy ? 'legacy:' : ''}${entry.name}`;

export function WebDavBackup() {
  const { lang, t } = useApp();
  const zh = lang === 'zh-CN';
  const [connection, setConnection] = useState<WebDavConnection>(() => ({ ...remembered(), password: '' }));
  const [password, setPassword] = useState('');
  const [confirmation, setConfirmation] = useState('');
  const [entries, setEntries] = useState<BackupEntry[] | null>(null);
  const [selected, setSelected] = useState('');
  const selectedEntry = entries?.find((entry) => entryKey(entry) === selected);
  const [busy, setBusy] = useState('');
  const [status, setStatus] = useState('');
  const [restoring, setRestoring] = useState(false);
  const [restoreSettings, setRestoreSettings] = useState(false);
  const valid = !!connection.url.trim() && !!connection.username.trim() && !!connection.password;
  const validPassword =
    new TextEncoder().encode(password).length >= 8 && new TextEncoder().encode(password).length <= 1024;
  const change = (key: keyof WebDavConnection, value: string) => {
    setConnection((c) => ({ ...c, [key]: value }));
    setEntries(null);
    setSelected('');
    setStatus('');
  };
  const run = async (label: string, action: () => Promise<string>) => {
    if (busy) return;
    setBusy(label);
    setStatus('');
    try {
      // Only non-secret connection details persist on this computer.
      try {
        const url = new URL(connection.url.trim());
        if (!url.username && !url.password && !url.search && !url.hash) {
          localStorage.setItem(
            'copybook.webdav',
            JSON.stringify({ url: url.toString(), username: connection.username }),
          );
        }
      } catch {
        /* Storage may be unavailable. */
      }
      setStatus(await action());
    } catch (e) {
      setStatus(t.errors[toCmdError(e).code]);
    } finally {
      setBusy('');
      setPassword('');
      setConfirmation('');
    }
  };
  const refresh = () =>
    run(zh ? '正在读取备份…' : 'Reading backups…', async () => {
      const list = await api.webdavList(connection);
      setEntries(list);
      setSelected(list[0] ? entryKey(list[0]) : '');
      return zh ? `找到 ${list.length} 份加密备份` : `Found ${list.length} encrypted backups`;
    });
  return (
    <Group title={zh ? 'WebDAV 加密备份' : 'Encrypted WebDAV backup'}>
      <div className="webdav-backup">
        <div className="backup-intro">
          <ShieldCheck size={20} />
          <p>
            {zh
              ? '历史记录、图片和设置在本机加密后上传。支持任意提供标准 WebDAV 的云盘或自建服务器。'
              : 'History, images and settings are encrypted locally before upload to a standard WebDAV server.'}
          </p>
        </div>
        {!inTauri && (
          <p className="backup-status">
            {zh
              ? '浏览器预览不连接 WebDAV，请在桌面应用中使用。'
              : 'Use the desktop app to connect to WebDAV.'}
          </p>
        )}
        <fieldset disabled={!!busy} className="backup-fields">
          <label className="backup-wide">
            {zh ? 'WebDAV 基础目录地址' : 'WebDAV base collection URL'}
            <input
              type="url"
              placeholder="https://dav.example.com/"
              value={connection.url}
              onChange={(e) => change('url', e.target.value)}
              autoComplete="off"
              spellCheck={false}
            />
          </label>
          <label>
            {zh ? '用户名' : 'Username'}
            <input
              value={connection.username}
              onChange={(e) => change('username', e.target.value)}
              autoComplete="off"
              spellCheck={false}
            />
          </label>
          <label>
            {zh ? 'WebDAV 密码 / 应用密码' : 'WebDAV password / app password'}
            <input
              type="password"
              value={connection.password}
              onChange={(e) => change('password', e.target.value)}
              autoComplete="off"
            />
          </label>
        </fieldset>
        <p className="backup-hint">
          {zh
            ? '备份自动存放在 copybook/ 子目录。仅在本机记住地址和用户名，密码不会保存。远程连接使用 HTTPS，本机测试支持 HTTP。'
            : 'Backups use the copybook/ subfolder automatically. Only URL and username are remembered; passwords are never saved. HTTPS is required remotely; loopback HTTP is supported.'}
        </p>
        <div className="backup-actions">
          <button
            className="btn"
            disabled={!!busy || !valid || !inTauri}
            onClick={() =>
              void run(zh ? '正在测试连接…' : 'Testing connection…', async () => {
                await api.webdavTest(connection);
                return zh ? '连接成功，目录可读取' : 'Connected; collection is readable';
              })
            }
          >
            {zh ? '测试连接' : 'Test connection'}
          </button>
          <button className="btn" disabled={!!busy || !valid || !inTauri} onClick={() => void refresh()}>
            <RefreshCw size={14} />
            {zh ? '读取云端备份' : 'List cloud backups'}
          </button>
        </div>
        <fieldset disabled={!!busy} className="backup-fields">
          <label>
            {zh ? '备份加密密码（至少 8 字节）' : 'Backup encryption password (8+ bytes)'}
            <input
              type="password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              autoComplete="new-password"
            />
          </label>
          <label>
            {zh ? '确认密码（创建备份时填写）' : 'Confirm password (for new backups)'}
            <input
              type="password"
              value={confirmation}
              onChange={(e) => setConfirmation(e.target.value)}
              autoComplete="new-password"
            />
          </label>
        </fieldset>
        <p className="backup-hint">
          {zh
            ? '使用 AES-256-GCM 与 Argon2id。恢复时需要原备份密码，遗失后无法找回。文件记录只备份路径，不上传其指向的文件。'
            : 'Uses AES-256-GCM and Argon2id. Keep your backup password: it cannot be recovered. File records include paths; referenced files are not uploaded.'}
        </p>
        <button
          className="btn btn--primary"
          disabled={!!busy || !valid || !validPassword || password !== confirmation || !inTauri}
          onClick={() =>
            void run(zh ? '正在加密并上传…' : 'Encrypting and uploading…', async () => {
              const entry = await api.webdavBackup(connection, password);
              setEntries((current) => [entry, ...(current ?? [])]);
              setSelected(entryKey(entry));
              return zh ? '加密备份已上传' : 'Encrypted backup uploaded';
            })
          }
        >
          <CloudUpload size={15} />
          {zh ? '创建加密备份' : 'Create encrypted backup'}
        </button>
        {entries !== null && (
          <div className="backup-restore">
            <label>
              {zh ? '云端备份' : 'Cloud backup'}
              <select value={selected} onChange={(e) => setSelected(e.target.value)} disabled={!!busy}>
                {!entries.length && <option value="">{zh ? '暂无备份' : 'No backups'}</option>}
                {entries.map((entry) => (
                  <option key={entryKey(entry)} value={entryKey(entry)}>
                    {entry.name}
                    {entry.legacy ? (zh ? ' · 旧根目录' : ' · legacy root') : ''}
                    {entry.bytes != null ? ` · ${formatBytes(entry.bytes, lang)}` : ''}
                  </option>
                ))}
              </select>
            </label>
            <button
              className="btn"
              disabled={!!busy || !selected || !valid || !validPassword || !inTauri}
              onClick={() => setRestoring(true)}
            >
              <Download size={14} />
              {zh ? '恢复所选备份' : 'Restore selected backup'}
            </button>
          </div>
        )}
        {(busy || status) && (
          <p className="backup-status" role="status" aria-live="polite">
            {busy || status}
          </p>
        )}
        {restoring && (
          <FeatureDialog
            title={zh ? '恢复加密备份' : 'Restore encrypted backup'}
            onClose={() => {
              if (!busy) setRestoring(false);
            }}
          >
            <p>
              {zh
                ? '将云端记录与本机历史合并，相同内容保留本机版本。密码校验和文件验证通过后才导入。'
                : 'Cloud records will be merged into local history. Existing content wins. Password and archive validation happen before import.'}
            </p>
            <label className="feature-check">
              <input
                type="checkbox"
                checked={restoreSettings}
                onChange={(e) => setRestoreSettings(e.target.checked)}
                disabled={!!busy}
              />
              {zh ? '同时恢复外观、记录和粘贴偏好' : 'Also restore appearance, capture and paste preferences'}
            </label>
            <p>
              {zh
                ? '本机快捷键、开机启动与管理员权限设置会保留。'
                : 'This machine’s hotkeys, startup and administrator choices are preserved.'}
            </p>
            <div className="feature-actions">
              <button className="btn btn--quiet" disabled={!!busy} onClick={() => setRestoring(false)}>
                {zh ? '取消' : 'Cancel'}
              </button>
              <button
                className="btn btn--primary"
                disabled={!!busy}
                onClick={() => {
                  setRestoring(false);
                  void run(
                    zh ? '正在下载、解密并恢复…' : 'Downloading, decrypting and restoring…',
                    async () => {
                      const result = await api.webdavRestore(
                        connection,
                        selectedEntry?.name ?? selected,
                        password,
                        restoreSettings,
                        selectedEntry?.legacy ?? false,
                      );
                      const summary = zh
                        ? `已导入 ${result.imported} 条，跳过 ${result.skipped} 条重复记录`
                        : `Imported ${result.imported}; skipped ${result.skipped} duplicates`;
                      return result.settingsRestored === false
                        ? summary +
                            (zh
                              ? '；偏好设置未能恢复，请检查本机存储权限'
                              : '; preferences could not be restored; check local storage permissions')
                        : summary;
                    },
                  );
                }}
              >
                {zh ? '合并恢复' : 'Merge and restore'}
              </button>
            </div>
          </FeatureDialog>
        )}
      </div>
    </Group>
  );
}
