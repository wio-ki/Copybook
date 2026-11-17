import { ChevronRight, Folder, FolderPlus, MoreHorizontal, Pencil, FolderInput, Trash2 } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import { api, toCmdError } from '../shared/api';
import { useApp } from '../shared/app';
import { FeatureDialog } from '../shared/FeatureDialog';
import { ContextMenu } from './ContextMenu';

export const folderParent = (path: string) => path.includes('/') ? path.slice(0, path.lastIndexOf('/')) : '';
export const folderLabel = (path: string) => path.slice(path.lastIndexOf('/') + 1);
export const insideFolder = (path: string, parent: string) => path === parent || path.startsWith(parent + '/');
const join = (parent: string, name: string) => parent ? `${parent}/${name}` : name;

export function FavoriteFolders({ groups, current, navigate, notify, refresh }: {
  groups: string[];
  current: string | null;
  navigate: (path: string | null) => void;
  notify: (message: string) => void;
  refresh: () => Promise<void>;
}) {
  const { lang, t } = useApp();
  const zh = lang === 'zh-CN';
  const [menu, setMenu] = useState<{ path: string; x: number; y: number } | null>(null);
  const [dialog, setDialog] = useState<{ kind: 'create' | 'rename' | 'move' | 'remove'; path: string } | null>(null);
  const [draft, setDraft] = useState('');
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState('');
  const breadcrumbs = useRef<HTMLElement>(null);
  useEffect(() => {
    if (breadcrumbs.current) breadcrumbs.current.scrollLeft = breadcrumbs.current.scrollWidth;
  }, [current]);
  const children = current == null ? [] : groups.filter((g) => folderParent(g) === current);
  const parts = current?.split('/').filter(Boolean) ?? [];
  const open = (kind: NonNullable<typeof dialog>['kind'], path: string) => {
    setDraft(kind === 'rename' ? folderLabel(path) : kind === 'move' ? folderParent(path) : '');
    setFailure('');
    setDialog({ kind, path });
  };
  const title = dialog?.kind === 'create' ? (zh ? '新建文件夹' : 'New folder')
    : dialog?.kind === 'rename' ? (zh ? '重命名文件夹' : 'Rename folder')
      : dialog?.kind === 'move' ? (zh ? '移动文件夹' : 'Move folder')
        : (zh ? '移除文件夹' : 'Remove folder');
  const name = draft.trim();
  const invalidName = !name || name.includes('/') || [...name].length > 40 || /[\x00-\x1f\x7f]/.test(name);
  const save = async () => {
    if (!dialog || busy) return;
    const target = dialog.kind === 'remove' ? null : dialog.kind === 'move'
      ? join(draft, folderLabel(dialog.path))
      : join(dialog.kind === 'create' ? dialog.path : folderParent(dialog.path), name);
    if (target !== dialog.path && target != null && groups.includes(target)) {
      setFailure(zh ? '同一目录中已存在此名称的文件夹。' : 'A folder with this name already exists here.');
      return;
    }
    setBusy(true);
    try {
      if (dialog.kind === 'create') await api.createGroup(target!);
      else await api.renameGroup(dialog.path, target);
      if (dialog.kind !== 'create' && current && insideFolder(current, dialog.path)) {
        navigate(target == null ? folderParent(dialog.path) : target + current.slice(dialog.path.length));
      }
      setDialog(null);
      await refresh();
      notify(zh ? '文件夹已更新' : 'Folder updated');
    } catch (e) {
      setFailure(t.errors[toCmdError(e).code]);
    } finally {
      setBusy(false);
    }
  };
  return (
    <section className="favorite-folders" aria-label={zh ? '收藏文件夹' : 'Favorite folders'}>
      <div className="folder-toolbar">
        <nav ref={breadcrumbs} className="folder-breadcrumbs" aria-label={zh ? '收藏目录' : 'Favorite path'}>
          <button type="button" aria-current={current === '' ? 'page' : undefined} onClick={() => navigate('')}>
            {zh ? '收藏' : 'Favorites'}
          </button>
          {current == null && <><ChevronRight size={12} /><span>{zh ? '全部收藏' : 'All favorites'}</span></>}
          {parts.map((part, i) => <span className="folder-crumb" key={i}>
            <ChevronRight size={12} />
            <button type="button" title={part} aria-current={i === parts.length - 1 ? 'page' : undefined}
              onClick={() => navigate(parts.slice(0, i + 1).join('/'))}>{part}</button>
          </span>)}
        </nav>
        <button type="button" className="icon-btn" aria-label={zh ? '新建文件夹' : 'New folder'}
          title={zh ? '新建文件夹' : 'New folder'} onClick={() => open('create', current ?? '')}>
          <FolderPlus size={16} />
        </button>
        {current != null && <button type="button" className="btn btn--quiet folder-all" onClick={() => navigate(null)}>
          {zh ? '全部收藏' : 'All favorites'}
        </button>}
      </div>
      {current && <button type="button" className="folder-up btn btn--quiet" onClick={() => navigate(folderParent(current))}>
        {zh ? '返回上一级' : 'Up one level'}
      </button>}
      {children.length > 0 && <ul className="folder-list">
        {children.map((path) => <li className="folder-row" key={path} data-folder={path}
          onContextMenu={(e) => { e.preventDefault(); setMenu({ path, x: e.clientX, y: e.clientY }); }}
          onKeyDown={(e) => { if (e.key === 'F2') { e.preventDefault(); open('rename', path); } }}>
          <button type="button" className="folder-open" onClick={() => navigate(path)} title={folderLabel(path)}>
            <Folder size={20} strokeWidth={1.6} /><span>{folderLabel(path)}</span><ChevronRight size={14} />
          </button>
          <button type="button" className="icon-btn" aria-label={`${zh ? '文件夹操作' : 'Folder actions'} ${folderLabel(path)}`}
            onClick={(e) => { const r = e.currentTarget.getBoundingClientRect(); setMenu({ path, x: r.right, y: r.bottom }); }}>
            <MoreHorizontal size={16} />
          </button>
        </li>)}
      </ul>}
      {menu && <ContextMenu x={menu.x} y={menu.y} onClose={() => setMenu(null)} entries={[
        { id: 'create', label: zh ? '新建子文件夹' : 'New subfolder', icon: FolderPlus, run: () => open('create', menu.path) },
        { id: 'rename', label: zh ? '重命名' : 'Rename', icon: Pencil, keys: ['F2'], run: () => open('rename', menu.path) },
        { id: 'move', label: zh ? '移动文件夹' : 'Move folder', icon: FolderInput, run: () => open('move', menu.path) },
        { id: 'remove', label: zh ? '移除文件夹' : 'Remove folder', icon: Trash2, danger: true, run: () => open('remove', menu.path) },
      ]} />}
      {dialog && <FeatureDialog title={title} onClose={() => { if (!busy) setDialog(null); }}>
        <form onSubmit={(e) => { e.preventDefault(); void save(); }}>
          {dialog.kind === 'remove' ? <p>{zh
            ? `移除“${folderLabel(dialog.path)}”及其所有子文件夹？其中的收藏记录会保留在收藏根目录。`
            : `Remove “${folderLabel(dialog.path)}” and all subfolders? Their items stay in the Favorites root.`}</p>
            : dialog.kind === 'move' ? <label>{zh ? '目标目录' : 'Destination'}
              <select aria-label={zh ? '目标目录' : 'Destination'} value={draft} onChange={(e) => setDraft(e.target.value)}>
                <option value="">{zh ? '收藏根目录' : 'Favorites root'}</option>
                {groups.filter((g) => !insideFolder(g, dialog.path)).map((g) => <option key={g} value={g}>{g.replaceAll('/', ' / ')}</option>)}
              </select>
            </label> : <label>{zh ? '文件夹名称' : 'Folder name'}
              <input autoFocus value={draft} onChange={(e) => setDraft(e.target.value)} />
            </label>}
          {failure && <p role="alert">{failure}</p>}
          <div className="feature-actions">
            <button type="button" className="btn btn--quiet" disabled={busy} onClick={() => setDialog(null)}>{zh ? '取消' : 'Cancel'}</button>
            <button type="submit" className={`btn ${dialog.kind === 'remove' ? 'btn--danger' : 'btn--primary'}`}
              disabled={busy || (['create', 'rename'].includes(dialog.kind) && invalidName)}>
              {dialog.kind === 'remove' ? (zh ? '移除' : 'Remove') : (zh ? '保存' : 'Save')}
            </button>
          </div>
        </form>
      </FeatureDialog>}
    </section>
  );
}
