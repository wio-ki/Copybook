import { useEffect, useState } from 'react';
import { api, toCmdError } from '../shared/api';
import { useApp } from '../shared/app';
import type { ClipItem } from '../shared/contract';
import { FeatureDialog } from '../shared/FeatureDialog';

export type SnippetDraft = { group: string | null } | { item: ClipItem };

export function SnippetEditor({ draft, onClose, onSaved }: {
  draft: SnippetDraft;
  onClose: () => void;
  onSaved: (item: ClipItem) => void;
}) {
  const { lang, t } = useApp();
  const zh = lang === 'zh-CN';
  const item = 'item' in draft ? draft.item : null;
  const group = 'group' in draft ? draft.group : item?.group;
  const [value, setValue] = useState<string | null>(item ? null : '');
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState('');
  useEffect(() => {
    if (!item) return;
    let active = true;
    void api.getItemContent(item.id, 1024 * 1024).then((content) => {
      if (!active) return;
      if (content.truncated) setFailure(t.errors.too_large);
      else setValue(content.text);
    }).catch((e) => {
      if (active) setFailure(t.errors[toCmdError(e).code]);
    });
    return () => { active = false; };
  }, [item, t]);

  const tooLarge = value != null && new TextEncoder().encode(value).length > 1024 * 1024;
  const canSave = !busy && !!value?.trim() && !tooLarge;
  const save = async () => {
    if (!canSave || value == null) return;
    setBusy(true);
    setFailure('');
    try {
      const result = item ? await api.editSnippet(item.id, value) : await api.createSnippet(group ?? null, value);
      onSaved(result);
    } catch (e) {
      setFailure(t.errors[toCmdError(e).code]);
    } finally {
      setBusy(false);
    }
  };

  return (
    <FeatureDialog title={item ? (zh ? '编辑片段' : 'Edit snippet') : (zh ? '新建片段' : 'New snippet')}
      onClose={() => { if (!busy) onClose(); }}>
      <form onSubmit={(e) => { e.preventDefault(); void save(); }}>
        {!item && <p>{group?.replaceAll('/', ' / ') || (zh ? '收藏根目录' : 'Favorites root')}</p>}
        {item && (item.hasHtml || item.hasRtf) && <p>
          {zh ? '修改内容后将以纯文本保存。' : 'Changing the content saves it as plain text.'}
        </p>}
        {value != null ? (
          <label>
            {zh ? '片段内容' : 'Snippet text'}
            <textarea autoFocus spellCheck={false} value={value} disabled={busy}
              onChange={(e) => setValue(e.target.value)}
              onKeyDown={(e) => {
                if (e.ctrlKey && e.key === 'Enter' && !e.nativeEvent.isComposing) {
                  e.preventDefault();
                  void save();
                }
              }} />
          </label>
        ) : !failure && <p role="status">{zh ? '正在读取…' : 'Loading…'}</p>}
        {(failure || tooLarge) && <p role="alert">{failure || t.errors.too_large}</p>}
        <div className="feature-actions">
          <button type="button" className="btn btn--quiet" disabled={busy} onClick={onClose}>
            {zh ? '取消' : 'Cancel'}
          </button>
          <button type="submit" className="btn btn--primary" disabled={!canSave}>
            {zh ? '保存' : 'Save'}
          </button>
        </div>
      </form>
    </FeatureDialog>
  );
}
