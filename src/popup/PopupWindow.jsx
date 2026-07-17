import { useEffect, useRef, useState } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { listen } from '@tauri-apps/api/event';
import { Copy, FileIcon, ImageIcon, Pencil, Pin, Star, Type, X } from 'lucide-react';
import { api } from '../api.js';
import { createTranslator, getReferenceMessages } from '../i18n.js';
import { resolveThemeClass, useDocumentThemeSync, useSystemThemePreference } from '../theme.js';
import { useTransientScrollbar } from '../useTransientScrollbar.js';

function kindIcon(kind) {
  if (kind === 'image') return <ImageIcon size={15} />;
  if (kind === 'file') return <FileIcon size={15} />;
  return <Type size={15} />;
}

function fileNameFromPath(path) {
  const parts = String(path || '').split(/[\\/]/).filter(Boolean);
  return parts.length ? parts[parts.length - 1] : String(path || 'file');
}

function previewsFromPaths(paths = []) {
  return paths.map((path) => {
    const name = fileNameFromPath(path);
    const extension = name.split('.').pop()?.toLowerCase() || '';
    const isImage = ['png', 'jpg', 'jpeg', 'webp', 'bmp', 'gif', 'tif', 'tiff'].includes(extension);
    return { name, path, is_image: isImage, thumbnail_data_url: null };
  });
}

export default function PopupWindow({ id }) {
  const fileListRef = useTransientScrollbar();
  const [item, setItem] = useState(null);
  const [settings, setSettings] = useState(null);
  const [pinned, setPinned] = useState(false);
  const [hover, setHover] = useState(false);
  const [loadError, setLoadError] = useState('');
  const [imageSrc, setImageSrc] = useState('');
  const [filePreviews, setFilePreviews] = useState([]);
  const [editOpen, setEditOpen] = useState(false);
  const [confirmUnpinOpen, setConfirmUnpinOpen] = useState(false);
  const [draftText, setDraftText] = useState('');
  const [favorited, setFavorited] = useState(false);
  const [compactActions, setCompactActions] = useState(false);
  const [popupSize, setPopupSize] = useState(() => ({ width: window.innerWidth, height: window.innerHeight }));
  const [languagePacks, setLanguagePacks] = useState([]);
  const cardRef = useRef(null);
  const metricsFrameRef = useRef(null);
  const lastMetricsRef = useRef({ width: 0, height: 0 });
  const shapeRefreshRef = useRef({ inFlight: false, queued: false, timer: null, lastRun: 0 });
  const timerRef = useRef(null);
  const leaveTimerRef = useRef(null);
  const systemPrefersDark = useSystemThemePreference();
  const themeClass = resolveThemeClass(settings || { theme: 'system' }, systemPrefersDark);
  useDocumentThemeSync(themeClass);

  useEffect(() => {
    document.body.classList.add('clipanchor-popup-body');
    document.documentElement.classList.add('clipanchor-popup-root');
    // 给弹窗窗口添加专用根类，是为了让全局样式只保留卡片圆角并去掉窗口外层的重复圆角。
    // Popup windows get dedicated root classes so global styles keep only the card radius and remove the duplicate outer corner.
    return () => {
      document.body.classList.remove('clipanchor-popup-body');
      document.documentElement.classList.remove('clipanchor-popup-root');
    };
  }, []);

  useEffect(() => {
    let unlisten = null;
    listen('clipanchor-settings-changed', (event) => {
      // 弹窗独立于主窗口存在，必须监听设置广播才能在用户切换深浅模式时同步更新。
      // Popups live outside the main window, so they listen for settings broadcasts to stay in sync with theme changes.
      setSettings(event.payload);
    }).then((off) => { unlisten = off; }).catch(() => {});
    return () => { if (unlisten) unlisten(); };
  }, []);

  useEffect(() => {
    const updatePopupMetrics = () => {
      if (metricsFrameRef.current) return;
      metricsFrameRef.current = window.requestAnimationFrame(() => {
        metricsFrameRef.current = null;
        const width = window.innerWidth;
        const height = window.innerHeight;
        const previous = lastMetricsRef.current;
        if (Math.abs(previous.width - width) < 1 && Math.abs(previous.height - height) < 1) return;
        lastMetricsRef.current = { width, height };
        // 直接读取视口尺寸，是为了让系统原生 resize 过程中内容布局跟着窗口实时变化，而不是等待卡片测量回流。
        // Reading the viewport directly lets content relayout during native resizing instead of waiting for a card measurement reflow.
        setCompactActions(width < 330);
        setPopupSize({ width, height });
      });
    };
    updatePopupMetrics();
    window.addEventListener('resize', updatePopupMetrics, { passive: true });
    return () => {
      window.removeEventListener('resize', updatePopupMetrics);
      if (metricsFrameRef.current) window.cancelAnimationFrame(metricsFrameRef.current);
    };
  }, []);

  useEffect(() => {
    const flushNativeShape = () => {
      const state = shapeRefreshRef.current;
      const now = performance.now();
      const wait = Math.max(0, 28 - (now - state.lastRun));
      window.clearTimeout(state.timer);
      state.timer = window.setTimeout(async () => {
        if (state.inFlight) {
          state.queued = true;
          return;
        }
        state.inFlight = true;
        state.lastRun = performance.now();
        try {
          // 缩放时以受控频率刷新 Windows Region，是为了让新尺寸边缘立即可见，同时避免每个鼠标事件都调用 Win32 导致卡顿。
          // During resizing the Windows region is refreshed at a controlled rate so new edges appear immediately without calling Win32 for every mouse event.
          await api.refreshPopupShape(id);
        } catch (_) {}
        state.inFlight = false;
        if (state.queued) {
          state.queued = false;
          flushNativeShape();
        }
      }, wait);
    };
    window.addEventListener('resize', flushNativeShape, { passive: true });
    return () => {
      window.removeEventListener('resize', flushNativeShape);
      window.clearTimeout(shapeRefreshRef.current.timer);
    };
  }, [id]);

  useEffect(() => {
    const requiredLanguageMessages = getReferenceMessages('en');
    Promise.all([api.bootstrap(), api.listLanguagePacks(requiredLanguageMessages).catch(() => [])]).then(([boot, packs]) => {
      setSettings(boot.settings);
      setLanguagePacks(Array.isArray(packs) ? packs : []);
    }).catch((error) => setLoadError(String(error)));
    api.getPopupItem(id).then((payload) => {
      setItem(payload);
      setPinned(Boolean(payload.is_pinned));
      setFavorited(Boolean(payload.is_pinned));
      setDraftText(payload.text_content || payload.summary || '');
      if (payload.kind === 'image') {
        api.readImageDataUrl(id).then((src) => setImageSrc(src || '')).catch(() => setImageSrc(''));
      }
      if (payload.kind === 'file') {
        const fallbackPreviews = previewsFromPaths(payload.file_paths || []);
        // 先用弹窗载荷里的完整路径列表渲染，是为了即使后端预览命令慢或失败，也不会把 25 个文件误显示成只有少数几个。
        // The popup renders from the full path list first so a slow or failed preview command cannot make 25 files look like only a few items.
        setFilePreviews(fallbackPreviews);
        api.readFilePreviews(id).then((items) => {
          const previews = Array.isArray(items) ? items : [];
          // 后端预览只允许增强图标/缩略图，不能缩短完整文件列表；否则用户会误以为复制内容丢失。
          // Backend previews may enhance icons or thumbnails, but must never shorten the complete file list because that looks like lost clipboard data.
          setFilePreviews(previews.length >= fallbackPreviews.length ? previews : fallbackPreviews);
        }).catch(() => setFilePreviews(fallbackPreviews));
      }
    }).catch((error) => {
      setLoadError(String(error));
      window.setTimeout(() => api.closePopup(id), 2200);
    });
  }, [id]);

  useEffect(() => {
    const hardClose = window.setTimeout(() => {
      if (!item && !loadError) setLoadError('Popup item loading timed out.');
    }, 3200);
    return () => window.clearTimeout(hardClose);
  }, [item, loadError]);


  useEffect(() => {
    const onKeyDown = (event) => {
      if (event.key !== 'Escape') return;
      event.preventDefault();
      if (editOpen) {
        setEditOpen(false);
        return;
      }
      if (confirmUnpinOpen) {
        setConfirmUnpinOpen(false);
        return;
      }
      if (pinned) {
        // 小尺寸置顶弹窗在显示二次确认前先保证最小可点区域，是为了避免确认按钮被底部操作栏或窗口裁剪遮住。
        // Small pinned popups ensure a minimum clickable area before showing confirmation so buttons are not hidden by the action bar or window clipping.
        openUnpinConfirmation();
        return;
      }
      // 弹窗不会主动抢焦点；只有用户让弹窗获得焦点后，Esc 才关闭它，避免打断当前工作的应用。
      // Popups never steal focus; Esc closes the popup only after the user has focused it, so the active app is not interrupted.
      api.closePopup(id);
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [id, editOpen, confirmUnpinOpen, pinned]);

  useEffect(() => {
    if (!settings || pinned || hover) return;
    timerRef.current = window.setTimeout(() => api.closePopup(id), settings.auto_destroy_seconds * 1000);
    return () => window.clearTimeout(timerRef.current);
  }, [settings, pinned, hover, id]);

  useEffect(() => {
    if (!pinned && confirmUnpinOpen) {
      // 取消置顶后立即收起确认层，是为了避免弹窗状态切换时留下一个不可交互的悬浮面板。
      // The confirmation layer is collapsed as soon as the popup is no longer pinned so state changes never leave a stale floating panel behind.
      setConfirmUnpinOpen(false);
    }
  }, [pinned, confirmUnpinOpen]);

  if (loadError) {
    return (
      <main tabIndex={0} className={`popup-shell ${themeClass} actions-visible popup-error-shell`} onMouseEnter={() => setHover(true)} onMouseMove={() => setHover(true)} onMouseLeave={() => setHover(false)}>
        <section className="popup-card popup-error-card">
          <div className="popup-content">
            <div className="popup-meta">
              <span>ClipAnchor</span>
              <button className="popup-close" onClick={() => api.closePopup(id)} title="Close" aria-label="Close"><X size={13} /></button>
            </div>
            <strong>Popup failed to load</strong>
            <p title={loadError}>{loadError}</p>
          </div>
        </section>
      </main>
    );
  }

  if (!item || !settings) return (
    <main tabIndex={0} className={`popup-shell ${themeClass}`} onMouseEnter={() => setHover(true)} onMouseMove={() => setHover(true)} onMouseLeave={() => setHover(false)}>
      <section className="popup-card popup-loading-card">
        <div className="popup-content">
          <div className="popup-meta">
            <span>ClipAnchor</span>
            <button className="popup-close" onClick={() => api.closePopup(id)} title="Close" aria-label="Close"><X size={13} /></button>
          </div>
          <strong>Loading clipboard item…</strong>
        </div>
      </section>
    </main>
  );

  const t = createTranslator(settings.locale, languagePacks);
  const motionClass = settings.animation_mode === 'performance' ? 'motion-performance' : 'motion-elegant';
  const actionsVisible = !confirmUnpinOpen && (!settings.auto_hide_actions || hover);
  const isTextItem = item.kind === 'text';
  const hasFilePreview = item.kind === 'file' && filePreviews.length > 0;
  const visibleFilePreviews = hasFilePreview ? filePreviews : [];
  const fileCount = item.kind === 'file' ? (item.file_paths?.length || filePreviews.length || 0) : 0;
  // 文件列表不再按弹窗尺寸截断，是为了复制 25 个或更多文件时仍能确认完整内容；滚动交给样式层处理。
  // The file list is no longer truncated by popup size so copying 25 or more files still shows the full selection; CSS handles scrolling.

  async function pin() {
    setPinned(true);
    await api.pinPopup(id);
  }

  async function toggleFavorite() {
    const next = !favorited;
    setFavorited(next);
    await api.togglePopupFavorite(id, next);
  }

  async function savePopupText() {
    const text = draftText.trim();
    if (!text) return;
    const sourceId = id.split('-pinned-')[0];
    await api.updateTextRecord(sourceId, text);
    setItem({ ...item, text_content: text, summary: text.slice(0, 200) });
    setEditOpen(false);
  }

  function drag(event) {
    if (!pinned || event.button !== 0 || event.target.closest('button, a, input, textarea, select, .popup-unpin-confirm')) return;
    // 拖动请求不等待 Promise 结束，避免原生窗口拖拽期间阻塞 React 事件队列造成弹窗假死。
    // The drag request is fire-and-forget so native window dragging cannot block React's event queue and make the popup appear frozen.
    getCurrentWindow().startDragging().catch(() => {});
  }

  async function beginResize(event) {
    if (!pinned || event.button !== 0) return;
    event.preventDefault();
    event.stopPropagation();
    setHover(true);
    const popupWindow = getCurrentWindow();
    try {
      // ResizeDirection 在 Tauri v2 中只是类型别名并非运行时导出，直接导入会让整个 React 入口模块加载失败而白屏。
      // ResizeDirection is only a Tauri v2 type alias, not a runtime export, so importing it breaks the React entry module and causes a blank white app.
      await popupWindow.startResizeDragging('SouthEast');
    } catch (error) {
      // 某些 API 文档示例允许不传方向；失败时再尝试无参调用，避免不同 Tauri 小版本造成右下角缩放完全不可用。
      // Some API examples allow omitting the direction; retrying without it prevents minor Tauri version differences from disabling corner resizing entirely.
      await popupWindow.startResizeDragging().catch(() => console.error('ClipAnchor popup native resize failed:', error));
    }
  }

  function openUnpinConfirmation() {
    // 确认层打开时强制收起底部操作栏，是为了保留用户手动缩小后的窗口尺寸，同时避免操作按钮再次被 hover 唤起并遮挡确认按钮。
    // When the confirmation layer opens, the action bar is forced closed so the user's compact popup size is preserved while hover cannot bring actions back and cover the confirm buttons.
    setHover(false);
    setConfirmUnpinOpen(true);
  }

  function requestHeaderClose() {
    if (pinned) {
      // 钉住后的右上角关闭按钮仍走二次确认，是为了恢复关闭入口的同时避免误关重要内容。
      // The pinned top-right close button still opens confirmation so restoring the close affordance does not make important clips easy to dismiss by accident.
      openUnpinConfirmation();
      return;
    }
    api.closePopup(id);
  }

  return (
    <main
      tabIndex={0}
      className={`popup-shell ${themeClass} ${motionClass} ${pinned ? 'pinned' : ''} ${actionsVisible ? 'actions-visible' : ''} ${compactActions ? 'compact-popup-actions' : ''}`}
      onMouseEnter={() => {
        window.clearTimeout(leaveTimerRef.current);
        setHover(true);
      }}
      onMouseMove={() => {
        // 加载阶段鼠标已经停在弹窗内时，mousemove 兜底记录 hover，避免内容加载完成后按钮仍保持隐藏。
        // When the pointer is already inside during loading, mousemove preserves hover so actions are visible after content mounts.
        window.clearTimeout(leaveTimerRef.current);
        setHover(true);
      }}
      onMouseLeave={() => {
        leaveTimerRef.current = window.setTimeout(() => setHover(false), 1000);
      }}
      onPointerDown={drag}
    >
      <section className={`popup-card ${confirmUnpinOpen ? 'confirm-unpin-open' : ''}`} ref={cardRef}>
        <div className="popup-glow" />
        <div className="popup-content">
          <div className="popup-meta">
            <span>{kindIcon(item.kind)} {isTextItem ? t('text') : 'ClipAnchor'}</span>
            <div className="popup-meta-actions">
              <button className="popup-close" onClick={requestHeaderClose} title={t('close') || 'Close'} aria-label={t('close') || 'Close'}><X size={13} /></button>
            </div>
          </div>
          {item.kind === 'image' && imageSrc ? <img src={imageSrc} alt="clipboard" /> : null}
          {hasFilePreview ? (
            <>
              <div className="popup-file-count">{item.summary || `${fileCount} ${t('file')}`} · {fileCount} {t('itemCount')}</div>
              <div ref={fileListRef} className={`popup-file-list scroll-area ${filePreviews.length > 1 ? 'multi' : ''}`}>
                {visibleFilePreviews.map((file, index) => (
                  <div className="popup-file-row" key={`${file.path}-${index}`} title={file.path || file.name}>
                    {file.thumbnail_data_url ? <img src={file.thumbnail_data_url} alt={file.name} /> : <span>{file.is_image ? <ImageIcon size={17} /> : <FileIcon size={17} />}</span>}
                    <em>{file.name}</em>
                  </div>
                ))}
              </div>
            </>
          ) : null}
          {isTextItem ? (
            <div className="popup-text-body" title={item.text_content || item.summary}>{item.text_content || item.summary}</div>
          ) : item.kind !== 'file' ? (
            <strong title={item.summary}>{item.summary}</strong>
          ) : (
            !hasFilePreview ? <strong className="popup-file-summary" title={item.summary}>{item.summary}</strong> : null
          )}
        </div>
        <footer className="popup-actions">
          {!pinned ? (
            <div className="split-actions unpinned-actions">
              <button className="pin-button" onClick={pin}><Pin size={15} /><span className="popup-action-text">{t('pin')}</span></button>
              <button onClick={() => api.closePopup(id)}><X size={15} /><span className="popup-action-text">{t('close')}</span></button>
            </div>
          ) : (
            <div className={`split-actions popup-pinned-actions ${isTextItem ? 'text-actions' : 'media-actions'}`}>
              {isTextItem ? <button className="icon-only" onClick={() => setEditOpen(true)} title={t('edit')} aria-label={t('edit')}><Pencil size={15} /></button> : null}
              <button className="icon-only" onClick={toggleFavorite} title={favorited ? t('unmarkFavorite') : t('markFavorite')} aria-label={favorited ? t('unmarkFavorite') : t('markFavorite')}><Star size={15} /></button>
              <button onClick={() => api.copyItem(id)}><Copy size={15} /><span className="popup-action-text">{t('copy')}</span></button>
              <button className="unpin-action-button" onClick={() => api.closePopup(id)}><X size={15} /><span className="popup-action-text">{t('unpin')}</span></button>
            </div>
          )}
        </footer>
        {pinned ? <button className="popup-resize-handle" onPointerDown={beginResize} title="Resize" aria-label="Resize popup" /> : null}
        {pinned ? (
          <aside className={`popup-unpin-confirm ${confirmUnpinOpen ? 'open' : ''}`} role="dialog" aria-hidden={!confirmUnpinOpen} aria-label={t('confirmUnpinTitle')}>
            <strong>{t('confirmUnpinTitle')}</strong>
            <p>{t('confirmUnpinMessage')}</p>
            <div>
              <button onClick={() => setConfirmUnpinOpen(false)} tabIndex={confirmUnpinOpen ? 0 : -1}>{t('cancel')}</button>
              <button className="danger" onClick={() => api.closePopup(id)} tabIndex={confirmUnpinOpen ? 0 : -1}>{t('confirm')}</button>
            </div>
          </aside>
        ) : null}
        {editOpen ? (
          <div className="popup-editor-overlay">
            <textarea value={draftText} onChange={(event) => setDraftText(event.target.value)} />
            <div>
              <button onClick={() => setEditOpen(false)}>{t('cancel')}</button>
              <button onClick={savePopupText}>{t('save')}</button>
            </div>
          </div>
        ) : null}
      </section>
    </main>
  );
}
