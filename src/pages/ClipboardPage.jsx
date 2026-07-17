import { useEffect, useMemo, useRef, useState } from 'react';
import { ArrowUp, Copy, FileIcon, FolderOpen, ImageIcon, Layers, Pencil, Pin, Plus, RefreshCw, Save, Search, ShieldCheck, Sparkles, Star, Trash, X } from 'lucide-react';
import { api } from '../api.js';
import { useTransientScrollbar } from '../useTransientScrollbar.js';

function iconFor(kind) {
  if (kind === 'image') return <ImageIcon size={18} />;
  if (kind === 'file') return <FileIcon size={18} />;
  return <Layers size={18} />;
}

function ImageThumb({ record }) {
  const [src, setSrc] = useState('');
  useEffect(() => {
    let active = true;
    if (!record.image_path) return undefined;
    api.readImageDataUrl(record.id).then((value) => {
      if (active) setSrc(value || '');
    }).catch(() => {
      if (active) setSrc('');
    });
    return () => { active = false; };
  }, [record.id, record.image_path]);

  if (src) return <img className="thumb" src={src} alt="thumbnail" />;
  return <div className="thumb placeholder">{iconFor(record.kind)}</div>;
}

function FileThumb({ record }) {
  const count = record.file_paths?.length || 0;
  const hasImages = record.file_paths?.some((path) => /\.(png|jpe?g|webp|bmp|gif|tiff?)$/i.test(path));
  return (
    <div className={`thumb file-thumb file-kind-thumb ${hasImages ? 'image-file' : ''}`} title={recordDetail(record)}>
      {hasImages ? <ImageIcon size={18} /> : <FolderOpen size={18} />}
      {count > 1 ? <small>{count}</small> : null}
    </div>
  );
}

function PreviewThumb({ record }) {
  if (record.kind === 'image' || record.image_path) return <ImageThumb record={record} />;
  if (record.kind === 'file') return <FileThumb record={record} />;
  return <div className="thumb placeholder">{iconFor(record.kind)}</div>;
}

function recordDetail(record) {
  if (record.kind === 'file' && record.file_paths?.length) {
    return record.file_paths.map((path) => path.split(/[\\/]/).pop()).filter(Boolean).slice(0, 3).join(' · ');
  }
  return record.text_content || record.summary;
}

function recordTitle(record) {
  if (record.kind === 'file' && record.file_paths?.length === 1) {
    return record.file_paths[0].split(/[\\/]/).pop() || record.summary;
  }
  return record.summary;
}

function humanBytes(bytes) {
  const units = ['B', 'KB', 'MB', 'GB'];
  let value = Number(bytes || 0);
  let index = 0;
  while (value > 1024 && index < units.length - 1) {
    value /= 1024;
    index += 1;
  }
  return `${value.toFixed(index === 0 ? 0 : 1)} ${units[index]}`;
}

function ConfirmDialog({ config, onClose }) {
  if (!config) return null;
  return (
    <div className="modal-backdrop native-confirm-backdrop" role="presentation" onMouseDown={onClose}>
      <section className="native-confirm-modal" role="dialog" aria-modal="true" onMouseDown={(event) => event.stopPropagation()}>
        <div className="native-confirm-icon">{config.danger ? <Trash size={18} /> : <ShieldCheck size={18} />}</div>
        <div className="native-confirm-copy">
          <strong>{config.title}</strong>
          <p>{config.message}</p>
        </div>
        <div className="native-confirm-actions">
          <button className="soft-button" onClick={onClose}>{config.cancelLabel}</button>
          <button className={config.danger ? 'danger-button' : 'primary-button'} onClick={() => { config.onConfirm(); onClose(); }}>
            {config.confirmLabel}
          </button>
        </div>
      </section>
    </div>
  );
}

function TextRecordDialog({ mode, value, t, onChange, onCancel, onSave }) {
  if (!mode) return null;
  return (
    <div className="modal-backdrop" role="presentation" onMouseDown={onCancel}>
      <section className="text-record-modal" role="dialog" aria-modal="true" onMouseDown={(event) => event.stopPropagation()}>
        <div className="modal-head">
          <div>
            <p className="eyebrow">{mode === 'create' ? t('manualTextEyebrow') : t('editTextEyebrow')}</p>
            <h2>{mode === 'create' ? t('addTextRecord') : t('editTextRecord')}</h2>
          </div>
          <button className="icon-action" onClick={onCancel} title={t('cancel')}>
            <X size={16} />
          </button>
        </div>
        <textarea
          autoFocus
          className="text-record-editor"
          value={value}
          onChange={(event) => onChange(event.target.value)}
          placeholder={t('textRecordPlaceholder')}
        />
        <div className="modal-actions">
          <button className="soft-button" onClick={onCancel}>{t('cancel')}</button>
          <button className="primary-button" onClick={onSave}>
            <Save size={16} /> {t('save')}
          </button>
        </div>
      </section>
    </div>
  );
}

export default function ClipboardPage({ t, refreshKey, mode = 'clipboard' }) {
  const favoriteMode = mode === 'favorites';
  const paneRef = useRef(null);
  useTransientScrollbar(paneRef);
  const [query, setQuery] = useState('');
  const [kind, setKind] = useState(favoriteMode ? 'favorite' : 'all');
  const [records, setRecords] = useState([]);
  const [selected, setSelected] = useState(new Set());
  const [dialogMode, setDialogMode] = useState(null);
  const [editingId, setEditingId] = useState('');
  const [draftText, setDraftText] = useState('');
  const [showBackTop, setShowBackTop] = useState(false);
  const [confirmConfig, setConfirmConfig] = useState(null);

  useEffect(() => {
    setKind(favoriteMode ? 'favorite' : 'all');
    setSelected(new Set());
  }, [favoriteMode]);

  useEffect(() => {
    const pane = paneRef.current;
    if (!pane) return undefined;
    const updateBackTopVisibility = () => setShowBackTop(pane.scrollTop > 160);
    updateBackTopVisibility();
    // 返回顶部按钮只在内容真正滚动后出现，是为了避免工具栏占用横向空间并保持历史操作区稳定。
    // The back-to-top button appears only after real scrolling so the toolbar stays stable and does not waste horizontal space.
    pane.addEventListener('scroll', updateBackTopVisibility, { passive: true });
    return () => pane.removeEventListener('scroll', updateBackTopVisibility);
  }, [favoriteMode, refreshKey]);

  const refresh = () => {
    const requestKind = favoriteMode ? 'favorite' : kind;
    api.listHistory(query, requestKind).then(setRecords).catch(console.error);
  };

  useEffect(() => {
    refresh();
  }, [query, kind, refreshKey, favoriteMode]);

  const visibleRecords = useMemo(() => {
    // 收藏只是给记录增加保护与独立入口，不应让该记录从剪贴板历史中消失，否则用户会误以为收藏会移动内容。
    // Favorite is a protected flag plus a focused entry point, not a move operation; keeping it in history matches clipboard users' mental model.
    const scoped = records;
    if (favoriteMode && ['text', 'image', 'file'].includes(kind)) {
      return scoped.filter((record) => record.kind === kind);
    }
    return scoped;
  }, [favoriteMode, kind, records]);

  const stats = useMemo(() => {
    const pinned = records.filter((record) => record.is_pinned).length;
    const bytes = visibleRecords.reduce((sum, record) => sum + Number(record.bytes || 0), 0);
    return { total: visibleRecords.length, pinned, bytes: humanBytes(bytes) };
  }, [records, visibleRecords]);

  const groups = useMemo(() => {
    if (favoriteMode) {
      const title = ['text', 'image', 'file'].includes(kind) ? t(kind) : t('favorites');
      return [{ key: 'favorites', title, items: visibleRecords }].filter((group) => group.items.length);
    }
    const title = kind === 'all' ? t('recent') : t(kind);
    return [{ key: kind, title, items: visibleRecords }].filter((group) => group.items.length);
  }, [favoriteMode, kind, visibleRecords, t]);

  async function ensureRecordValid(record) {
    const valid = await api.validateRecord(record.id);
    if (!valid.valid) {
      setConfirmConfig({
        title: t('invalidTitle'),
        message: t('invalid'),
        confirmLabel: t('delete'),
        cancelLabel: t('cancel'),
        danger: true,
        onConfirm: async () => {
          await api.deleteRecordsForce([record.id]);
          refresh();
        },
      });
      return false;
    }
    return true;
  }

  async function handleCopy(record) {
    if (!(await ensureRecordValid(record))) return;
    await api.copyItem(record.id);
  }

  async function handlePinPopup(record) {
    if (!(await ensureRecordValid(record))) return;
    // 历史记录置顶复用后端临时弹窗缓存，是为了避免重新写剪贴板造成监听服务重复生成普通弹窗。
    // History-to-popup pinning reuses the backend temporary popup cache to avoid rewriting the clipboard and spawning a duplicate normal popup.
    await api.pinHistoryItem(record.id);
  }

  async function toggleFavorite(record) {
    // 收藏状态直接落库，是为了让导入导出、清空保护和列表置顶始终使用同一可信状态源。
    // Favorite state is persisted directly so import/export, clear protection, and list pinning all share one reliable source of truth.
    await api.toggleRecordPin(record.id, !record.is_pinned);
    refresh();
  }

  function startCreateText() {
    setEditingId('');
    setDraftText('');
    setDialogMode('create');
  }

  function startEditText(record) {
    setEditingId(record.id);
    setDraftText(record.text_content || record.summary || '');
    setDialogMode('edit');
  }

  function closeDialog() {
    setDialogMode(null);
    setEditingId('');
    setDraftText('');
  }

  async function saveTextRecord() {
    const text = draftText.trim();
    if (!text) {
      setConfirmConfig({
        title: t('textRecordEmptyTitle'),
        message: t('textRecordEmpty'),
        confirmLabel: t('ok'),
        cancelLabel: t('cancel'),
        danger: false,
        onConfirm: () => {},
      });
      return;
    }
    if (dialogMode === 'create') {
      // 收藏夹页面新增文本默认加入收藏，是为了让用户在当前工作区创建的内容立即出现在该工作区。
      // Text created from the Favorites page is favorited by default so content created in the current workspace appears there immediately.
      await api.createTextRecord(text, favoriteMode);
    } else if (dialogMode === 'edit') {
      await api.updateTextRecord(editingId, text);
    }
    closeDialog();
    refresh();
  }

  function removeRecord(record) {
    setConfirmConfig({
      title: record.is_pinned ? t('confirmDeleteFavoriteTitle') : t('confirmDeleteOneTitle'),
      message: record.is_pinned ? t('confirmDeleteFavorite') : t('confirmDeleteOne'),
      confirmLabel: t('delete'),
      cancelLabel: t('cancel'),
      danger: true,
      onConfirm: async () => {
        // 收藏记录的单条删除通过强制删除执行，是为了把“二次确认”作为唯一额外步骤，而不是要求用户先取消收藏。
        // Favorite single-item deletion uses force delete so the extra confirmation is the only additional step instead of forcing an unfavorite action first.
        const deleter = record.is_pinned ? api.deleteRecordsForce : api.deleteRecords;
        await deleter([record.id]);
        setSelected((current) => {
          const next = new Set(current);
          next.delete(record.id);
          return next;
        });
        refresh();
      },
    });
  }

  function removeSelected() {
    const targets = visibleRecords.filter((record) => selected.has(record.id));
    if (!targets.length) return;
    const hasFavorites = targets.some((record) => record.is_pinned);
    setConfirmConfig({
      title: hasFavorites ? t('confirmDeleteSelectedFavoriteTitle') : t('confirmDeleteSelectedTitle'),
      message: hasFavorites ? t('confirmDeleteSelectedFavorite') : t('confirmDeleteSelected'),
      confirmLabel: t('delete'),
      cancelLabel: t('cancel'),
      danger: true,
      onConfirm: async () => {
        const ids = targets.map((record) => record.id);
        await (hasFavorites ? api.deleteRecordsForce(ids) : api.deleteRecords(ids));
        setSelected(new Set());
        refresh();
      },
    });
  }

  async function refreshFavoritesValidity() {
    const invalid = await api.validateFavorites();
    if (!invalid.length) {
      setConfirmConfig({
        title: t('favoritesValidTitle'),
        message: t('favoritesValid'),
        confirmLabel: t('ok'),
        cancelLabel: t('cancel'),
        danger: false,
        onConfirm: () => {},
      });
      return;
    }
    const preview = invalid.slice(0, 4).map((record) => recordTitle(record)).join(' / ');
    setConfirmConfig({
      title: t('favoritesInvalidTitle'),
      message: `${t('favoritesInvalid')} ${preview}`,
      confirmLabel: t('delete'),
      cancelLabel: t('cancel'),
      danger: true,
      onConfirm: async () => {
        await api.deleteRecordsForce(invalid.map((record) => record.id));
        refresh();
      },
    });
  }

  function scrollToTop() {
    // 历史记录可能非常长，提供显式回到顶部入口可以减少用户反复滚动查找的成本。
    // History can be very long, so an explicit back-to-top action reduces repeated manual scrolling.
    paneRef.current?.scrollTo({ top: 0, behavior: 'smooth' });
  }

  return (
    <section className="clipboard-pane scroll-area" ref={paneRef}>
      <div className="clipboard-hero compact-history-hero">
        <div>
          <p className="eyebrow">{favoriteMode ? t('favoritesEyebrow') : t('historyEyebrow')}</p>
          <h2>{favoriteMode ? t('favoriteBoardTitle') : t('noScrollPopup')}</h2>
        </div>
        <div className="metric-grid">
          <div className="metric-card">
            <strong>{stats.total}</strong>
            <span>{t('itemCount')}</span>
          </div>
          <div className="metric-card coral">
            <strong>{stats.pinned}</strong>
            <span>{t('protectedCount')}</span>
          </div>
          <div className="metric-card ghosted">
            <strong>{stats.bytes}</strong>
            <span>{t('totalBytes')}</span>
          </div>
        </div>
      </div>

      <div className="history-control-panel">
        <label className="history-search-field">
          <Search size={18} />
          <input value={query} onChange={(event) => setQuery(event.target.value)} placeholder={t('search')} />
        </label>
        <div className="history-action-bar">
          <div className="filter-chips" aria-label={t('filters')}>
            {(favoriteMode ? ['favorite', 'text', 'image', 'file'] : ['all', 'text', 'image', 'file']).map((name) => (
              <button key={name} className={kind === name ? 'active' : ''} onClick={() => setKind(name)}>{name === 'favorite' ? t('allFavorites') : t(name)}</button>
            ))}
          </div>
          <button className="soft-button" onClick={startCreateText}>
            <Plus size={16} /> {t('addText')}
          </button>
          {favoriteMode ? (
            <button className="soft-button" onClick={refreshFavoritesValidity}>
              <RefreshCw size={16} /> {t('refreshFavorites')}
            </button>
          ) : null}
          <button className="soft-button danger-line" disabled={!selected.size} onClick={removeSelected}>
            <Trash size={16} /> {t('delete')}{selected.size ? ` · ${selected.size}` : ''}
          </button>
        </div>
      </div>

      <div className="history-list compact-history-list">
        {!visibleRecords.length && (
          <div className="empty-state premium-empty">
            <Sparkles size={28} />
            <strong>{favoriteMode ? t('favoriteOnlyEmpty') : t('empty')}</strong>
          </div>
        )}
        {groups.map((group) => (
          <section className="history-group" key={group.key}>
            <h2>{group.key === 'favorites' ? <ShieldCheck size={15} /> : null}{group.title}</h2>
            {group.items.map((record) => (
              <article className={`history-item compact-history-item ${record.is_pinned ? 'favorite' : ''}`} key={record.id} onDoubleClick={() => handleCopy(record)}>
                <label className="select-dot">
                  <input
                    type="checkbox"
                    checked={selected.has(record.id)}
                    onChange={(event) => {
                      const next = new Set(selected);
                      event.target.checked ? next.add(record.id) : next.delete(record.id);
                      setSelected(next);
                    }}
                  />
                  <span />
                </label>
                {/* 收藏标记前置到标题区域，避免星号挤在标题末尾造成阅读断点。 */}
                {/* The favorite marker is placed before the title so it reads as item state instead of trailing text noise. */}
                <PreviewThumb record={record} />
                <div className="record-main">
                  {(() => {
                    const title = recordTitle(record);
                    const detail = recordDetail(record);
                    const showDetail = detail && detail.trim() && detail.trim() !== title.trim();
                    return (
                      <>
                        <strong className="record-title-line" title={detail || title}>
                          {record.is_pinned ? <Star className="favorite-title-marker" size={13} aria-hidden="true" /> : null}
                          <span>{title}</span>
                        </strong>
                        {showDetail ? <span title={detail}>{detail}</span> : null}
                      </>
                    );
                  })()}
                  <small>{new Date(record.created_at).toLocaleString()} · {humanBytes(record.bytes)}</small>
                </div>
                <div className="record-actions">
                  {record.kind === 'text' && (
                    <button className="icon-action" onClick={() => startEditText(record)} title={t('editText')}>
                      <Pencil size={16} />
                    </button>
                  )}
                  <button className={record.is_pinned ? 'icon-action active' : 'icon-action'} onClick={() => toggleFavorite(record)} title={record.is_pinned ? t('unmarkFavorite') : t('markFavorite')}>
                    <Star size={16} />
                  </button>
                  <button className="icon-action pin-action" onClick={() => handlePinPopup(record)} title={t('quickPin')}>
                    <Pin size={16} />
                  </button>
                  <button className="icon-action danger-icon" onClick={() => removeRecord(record)} title={t('deleteOne')}>
                    <Trash size={16} />
                  </button>
                  <button className="pill-action" onClick={() => handleCopy(record)}>
                    <Copy size={16} /> {t('copy')}
                  </button>
                </div>
              </article>
            ))}
          </section>
        ))}
      </div>

      {showBackTop ? (
        <button className="floating-back-top" onClick={scrollToTop} title={t('backToTop')} aria-label={t('backToTop')}>
          <ArrowUp size={17} />
        </button>
      ) : null}

      <ConfirmDialog config={confirmConfig} onClose={() => setConfirmConfig(null)} />

      <TextRecordDialog
        mode={dialogMode}
        value={draftText}
        t={t}
        onChange={setDraftText}
        onCancel={closeDialog}
        onSave={saveTextRecord}
      />
    </section>
  );
}
