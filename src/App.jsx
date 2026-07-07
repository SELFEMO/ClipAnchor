import { useEffect, useMemo, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { Activity, CheckCircle2, ClipboardList, Download, Info, Loader2, LogOut, Maximize2, Minus, Moon, RefreshCw, Settings, Sparkles, Star, Sun, X } from 'lucide-react';
import { api } from './api.js';
import { createTranslator } from './i18n.js';
import { resolveThemeClass, useDocumentThemeSync, useSystemThemePreference } from './theme.js';
import { shortcutDisplayTokens } from './shortcutDisplay.js';
import ClipboardPage from './pages/ClipboardPage.jsx';
import SettingsPage from './pages/SettingsPage.jsx';
import PopupWindow from './popup/PopupWindow.jsx';
import appIcon from './assets/clipanchor-icon.png';

function viewFromUrl() {
  const params = new URLSearchParams(window.location.search);
  return {
    view: params.get('view') || 'main',
    id: params.get('id') || ''
  };
}

function humanBytes(value) {
  if (!value || Number(value) <= 0) return '';
  const units = ['B', 'KB', 'MB', 'GB'];
  let size = Number(value);
  let index = 0;
  while (size >= 1024 && index < units.length - 1) {
    size /= 1024;
    index += 1;
  }
  return `${size.toFixed(size >= 10 || index === 0 ? 0 : 1)} ${units[index]}`;
}


function updateDialogCopy(t, status) {
  const key = status?.status || 'checking';
  if (key === 'checking') {
    return { icon: Loader2, busy: true, title: t('updateCheckingTitle'), message: t('updateCheckingMessage'), detail: t('updateCheckingDetail') };
  }
  if (key === 'downloading') {
    return { icon: Download, busy: true, title: t('updateDownloadingTitle'), message: t('updateDownloadingMessage'), detail: t('updateDownloadingDetail') };
  }
  if (key === 'downloaded') {
    return { icon: CheckCircle2, title: t('updateReadyTitle'), message: t('updateReadyMessage'), detail: t('updateReadyDetail') };
  }
  if (key === 'installing') {
    return { icon: Loader2, busy: true, title: t('updateInstallingTitle'), message: t('updateInstallingMessage'), detail: t('updateInstallingDetail') };
  }
  if (key === 'no_update') {
    return { icon: CheckCircle2, title: t('updateNoUpdateTitle'), message: t('updateNoUpdateMessage'), detail: t('updateNoUpdateDetail') };
  }
  if (key === 'asset_unavailable') {
    return { icon: Info, title: t('updateAssetUnavailableTitle'), message: t('updateAssetUnavailableMessage'), detail: t('updateAssetUnavailableDetail') };
  }
  if (key === 'update_available') {
    return { icon: Download, title: t('updateAvailableTitle'), message: t('updateAvailableMessage'), detail: t('updateAvailableDetail') };
  }
  if (key === 'update_failed') {
    return { icon: Info, title: t('updateFailedTitle'), message: t('updateFailedMessage'), detail: t('updateFailedDetail') };
  }
  return { icon: Info, title: t('updatePlaceholderTitle'), message: t('updatePlaceholderMessage') };
}

function UpdateStatusDialog({ t, status, onClose, onInstall }) {
  const copy = updateDialogCopy(t, status);
  const Icon = copy.icon || Info;
  const downloaded = Number(status?.downloaded_bytes || 0);
  const total = Number(status?.total_bytes || 0);
  const progress = total > 0 ? Math.min(100, Math.round((downloaded / total) * 100)) : 0;
  const showProgress = ['checking', 'downloading', 'installing'].includes(status?.status);
  const showInstall = Boolean(status?.install_ready || status?.asset_url) && !['checking', 'downloading', 'installing', 'no_update'].includes(status?.status);
  const primaryLabel = status?.status === 'no_update' ? t('ok') : (showInstall ? t('updateInstallNow') : t('ok'));
  // 更新弹窗只保留必要的用户动作与结果，避免发布说明中的开发者链接或变更日志噪音直接暴露给用户。
  // The update dialog keeps only essential user actions and results so developer links or changelog noise from release notes are not exposed directly to users.
  const metaRows = [
    status?.current_version ? [t('updateCurrentVersion'), status.current_version] : null,
    status?.latest_version ? [t('updateLatestVersion'), status.latest_version] : null,
    status?.asset_name ? [t('updatePackage'), status.asset_name] : null,
    status?.checked_at ? [t('updateCheckedAt'), status.checked_at] : null,
  ].filter(Boolean);

  return (
    <div className="soft-modal-backdrop update-placeholder-backdrop" role="presentation" onMouseDown={copy.busy ? undefined : onClose}>
      <section className="soft-modal-card update-placeholder-card update-status-card" role="dialog" aria-modal="true" onMouseDown={(event) => event.stopPropagation()}>
        <span className={`settings-dialog-icon update-placeholder-icon ${copy.busy ? 'is-spinning' : ''}`}><Icon size={19} /></span>
        <div className="settings-dialog-copy update-placeholder-copy update-status-copy">
          <strong>{copy.title}</strong>
          <p>{copy.message}</p>
          {copy.detail ? <p className="update-placeholder-note">{copy.detail}</p> : null}
          {showProgress ? (
            <div className={`update-progress ${total > 0 ? 'is-determinate' : 'is-indeterminate'}`}>
              <span style={{ width: total > 0 ? `${progress}%` : undefined }} />
            </div>
          ) : null}
          {total > 0 ? <p className="update-size-line">{humanBytes(downloaded) || '0 B'} / {humanBytes(total)}</p> : null}
          {metaRows.length ? (
            <div className="update-meta-grid">
              {metaRows.map(([label, value]) => (
                <div key={label}>
                  <span>{label}</span>
                  <strong>{value}</strong>
                </div>
              ))}
            </div>
          ) : null}
        </div>
        <div className="settings-dialog-actions update-placeholder-actions">
          {copy.busy ? (
            <button className="secondary-button" disabled>{t('updatePleaseWait')}</button>
          ) : (
            <>
              {showInstall ? <button className="primary-button" onClick={onInstall}>{primaryLabel}</button> : <button className="primary-button" onClick={onClose}>{primaryLabel}</button>}
              {showInstall ? <button className="secondary-button" onClick={onClose}>{t('updateLater')}</button> : null}
            </>
          )}
        </div>
      </section>
    </div>
  );
}

function AppTitlebar({ t }) {
  const actionLockRef = useMemo(() => ({ active: false }), []);

  async function runWindowAction(action, fallback) {
    if (actionLockRef.active) return;
    actionLockRef.active = true;
    // 窗口控制按钮直接在 pointerup 阶段触发，是为了绕开 Windows 无边框拖拽区可能吞掉 click 事件的问题。
    // Window controls fire on pointerup to bypass Windows borderless drag regions that can swallow click events.
    try {
      await action();
    } catch (error) {
      console.error('ClipAnchor window action failed:', error);
      if (fallback) await fallback().catch((fallbackError) => console.error('ClipAnchor fallback window action failed:', fallbackError));
    } finally {
      window.setTimeout(() => { actionLockRef.active = false; }, 120);
    }
  }

  function stopWindowControlEvent(event) {
    event.preventDefault();
    event.stopPropagation();
  }

  function windowButtonProps(action, fallback) {
    return {
      type: 'button',
      onPointerDown: stopWindowControlEvent,
      onMouseDown: stopWindowControlEvent,
      onPointerUp: (event) => {
        stopWindowControlEvent(event);
        runWindowAction(action, fallback);
      },
      onClick: stopWindowControlEvent
    };
  }

  return (
    <header className="native-titlebar">
      <div className="native-titlebar-brand">
        <img src={appIcon} alt="" />
        <span>ClipAnchor</span>
      </div>
      <div className="native-titlebar-drag" data-tauri-drag-region />
      <div className="native-window-controls">
        <button {...windowButtonProps(() => api.minimizeWindow(), () => getCurrentWindow().minimize())} title={t('minimize') || 'Minimize'} aria-label={t('minimize') || 'Minimize'}><Minus size={14} /></button>
        <button {...windowButtonProps(() => api.toggleMaximizeWindow(), () => getCurrentWindow().toggleMaximize())} title={t('maximize') || 'Maximize'} aria-label={t('maximize') || 'Maximize'}><Maximize2 size={13} /></button>
        <button className="close-control" {...windowButtonProps(() => api.closeMainWindow(), () => getCurrentWindow().close())} title={t('close') || 'Close'} aria-label={t('close') || 'Close'}><X size={14} /></button>
      </div>
    </header>
  );
}

export default function App() {
  const route = useMemo(viewFromUrl, []);
  const [boot, setBoot] = useState(null);
  const [tab, setTab] = useState('clipboard');
  const [refreshKey, setRefreshKey] = useState(0);
  const [updateDialogOpen, setUpdateDialogOpen] = useState(false);
  const [updateStatus, setUpdateStatus] = useState(null);
  const systemPrefersDark = useSystemThemePreference();
  const activeThemeClass = resolveThemeClass(boot?.settings || { theme: 'system' }, systemPrefersDark);
  useDocumentThemeSync(activeThemeClass);

  useEffect(() => {
    api.bootstrap().then((payload) => {
      setBoot(payload);
      return api.getUpdateStatus();
    }).then((status) => {
      if (!status) return;
      setUpdateStatus(status);
      if (status.prompt_on_main_open) {
        setUpdateDialogOpen(true);
      }
    }).catch(console.error);
    const historyUnlistenPromise = listen('history-updated', () => setRefreshKey((value) => value + 1));
    const settingsUnlistenPromise = listen('clipanchor-settings-changed', (event) => {
      // 快捷键会直接修改后端设置，因此主窗口必须监听设置广播，确保“基础设置”里的开关实时同步。
      // Shortcuts mutate backend settings directly, so the main window listens for broadcasts to keep Basic Settings switches in sync.
      setBoot((current) => current ? { ...current, settings: event.payload } : current);
    });
    const updateUnlistenPromise = listen('clipanchor-update-status', (event) => {
      // 后台更新检查只记录状态，真正的提示延后到主界面可见时展示，避免自启动静默模式打断用户。
      // Background update checks only store status, and prompts are delayed until the main UI is visible so startup Lite mode stays silent.
      setUpdateStatus(event.payload);
      if (event.payload?.prompt_on_main_open) {
        setUpdateDialogOpen(true);
      }
    });
    const activationUnlistenPromise = listen('clipanchor-main-window-activated', async () => {
      try {
        // 主界面从长时间轻量模式恢复时主动重新拉取基础状态，是为了避免隐藏期间的设置、更新和历史变更让界面停留在旧快照。
        // When the main UI returns from a long Lite-mode session, refreshing bootstrap state prevents settings, update, and history views from showing stale snapshots.
        const [payload, status] = await Promise.all([api.bootstrap(), api.getUpdateStatus()]);
        setBoot(payload);
        setUpdateStatus(status);
        setRefreshKey((value) => value + 1);
        if (status?.prompt_on_main_open) {
          setUpdateDialogOpen(true);
        }
      } catch (error) {
        console.error('ClipAnchor wake refresh failed:', error);
      }
    });
    return () => {
      historyUnlistenPromise.then((unlisten) => unlisten()).catch(() => {});
      settingsUnlistenPromise.then((unlisten) => unlisten()).catch(() => {});
      updateUnlistenPromise.then((unlisten) => unlisten()).catch(() => {});
      activationUnlistenPromise.then((unlisten) => unlisten()).catch(() => {});
    };
  }, []);

  useEffect(() => {
    if (!updateDialogOpen || !['checking', 'downloading'].includes(updateStatus?.status)) return undefined;
    // 更新请求由后端后台线程执行，前端轮询状态文件是为了不阻塞 UI，同时让检查和下载阶段保持可见。
    // Update work runs in a backend thread; polling the status file keeps the UI responsive while checking and downloading stay visible.
    const timer = window.setInterval(async () => {
      try {
        const status = await api.getUpdateStatus();
        if (status) setUpdateStatus(status);
      } catch (error) {
        console.error('ClipAnchor update polling failed:', error);
      }
    }, 900);
    return () => window.clearInterval(timer);
  }, [updateDialogOpen, updateStatus?.status]);

  if (route.view === 'popup') {
    return <PopupWindow id={route.id} />;
  }


  if (!boot) {
    return (
      <main className={`boot-screen ${activeThemeClass}`}>
        <div className="boot-orbit">
          <span />
          <Sparkles size={30} />
        </div>
        <p>ClipAnchor</p>
      </main>
    );
  }

  const t = createTranslator(boot.settings.locale);
  const themeClass = activeThemeClass;
  const uiScale = Math.min(2, Math.max(0.5, Number(boot.settings.ui_scale_percent || 100) / 100));
  const motionClass = boot.settings.animation_mode === 'performance' ? 'motion-performance' : 'motion-elegant';
  const isDark = themeClass === 'theme-dark';

  async function toggleTheme() {
    // 主题切换直接写入共享设置，是为了让主窗口、弹窗和下次启动保持同一套视觉状态。
    // Theme changes are persisted through shared settings so the main window, popups, and next launch keep one visual state.
    const nextTheme = isDark ? 'light' : 'dark';
    const saved = await api.saveSettings({ ...boot.settings, theme: nextTheme });
    setBoot({ ...boot, settings: saved });
  }

  async function checkUpdate() {
    // 手动检查先打开进度页，是为了让网络慢或 GitHub 响应慢时用户也能明确知道按钮已经生效。
    // Manual checks open the progress page first so users know the click worked even when network or GitHub responses are slow.
    setUpdateStatus({ status: 'checking', service_enabled: true, prompt_on_main_open: true, current_version: updateStatus?.current_version || '' });
    setUpdateDialogOpen(true);
    try {
      const status = await api.checkUpdate('manual');
      setUpdateStatus(status);
    } catch (error) {
      console.error('ClipAnchor update check failed:', error);
      setUpdateStatus({ status: 'update_failed', update_failed: true, attention_required: true, prompt_on_main_open: true, message: String(error) });
    }
  }

  async function dismissUpdateDialog() {
    setUpdateDialogOpen(false);
    try {
      // 关闭更新弹窗时同步清除后端提示位，是为了避免同一条检查结果在下一次打开主界面时再次弹出。
      // Closing the update dialog also clears the backend prompt flag so the same check result does not pop up again on the next main-window open.
      const status = await api.dismissUpdatePrompt();
      setUpdateStatus(status);
    } catch (error) {
      console.error('ClipAnchor update prompt dismiss failed:', error);
    }
  }

  async function installUpdate() {
    // 安装按钮只调用后端统一入口，是为了避免前端按平台拼接安装命令导致 Windows、macOS、Linux 行为分叉。
    // The install button calls one backend entry so the frontend does not duplicate platform-specific installer commands.
    setUpdateStatus((current) => ({ ...(current || {}), status: 'installing' }));
    try {
      const status = await api.installDownloadedUpdate();
      setUpdateStatus(status);
      setUpdateDialogOpen(false);
    } catch (error) {
      console.error('ClipAnchor install handoff failed:', error);
      setUpdateStatus((current) => ({ ...(current || {}), status: 'update_failed', update_failed: true, attention_required: true, message: String(error) }));
    }
  }

  async function quitApp() {
    // 侧边栏退出是明确的一键退出入口，不再弹出二次确认，避免用户误以为按钮无效。
    // The sidebar exit is an explicit quit action, so it skips a second confirmation that could make the button feel broken.
    await api.quitApp();
  }

  const pageTitle = tab === 'favorites' ? t('favoritesTitle') : (tab === 'clipboard' ? t('clipboardTitle') : t('settingsTitle'));
  const pageSubtitle = tab === 'favorites' ? t('favoritesSubtitle') : (tab === 'clipboard' ? t('clipboardSubtitle') : t('settingsSubtitle'));
  const privacyMode = boot.settings.privacy_filter_mode || (boot.settings.privacy_mode ? 'light' : 'off');
  const privacyStatus = privacyMode === 'off' ? t('privacyOff') : `${t('privacyOn')} · ${privacyMode === 'smart' ? t('privacySmartMode') : t('privacyLightMode')}`;
  // 侧边栏快捷键徽标使用平台化显示，是为了避免 macOS 用户看到 Windows 式 Ctrl 命名而误按 Command。
  // The sidebar shortcut badge uses platform-aware labels so macOS users do not mistake Windows-style Ctrl naming for Command.
  const pinServiceShortcutTokens = shortcutDisplayTokens(boot.settings.shortcuts?.toggle_pin_service || 'Ctrl+Shift+P');

  return (
    <main className={`app-shell codex-shell ${themeClass} ${motionClass}`} style={{ '--ui-scale': uiScale }}>
      <AppTitlebar t={t} />
      <section className="codex-frame">
        <aside className="codex-sidebar">
          <div className="brand-lockup">
            <div className="brand-mark"><img src={appIcon} alt="ClipAnchor" /></div>
            <div>
              <strong>ClipAnchor</strong>
              <span>{t('portableNative')}</span>
            </div>
          </div>

          <div className="status-card">
            <div className="status-row">
              <Activity size={16} />
              <span>{boot.settings.pin_service_enabled ? t('pinRunning') : t('pinPaused')}</span>
              <i className={boot.settings.pin_service_enabled ? 'status-dot on' : 'status-dot'} />
            </div>
            <p>{privacyStatus}</p>
          </div>

          <nav className="rail-tabs" aria-label="Primary navigation">
            <button className={tab === 'clipboard' ? 'active' : ''} onClick={() => setTab('clipboard')}>
              <ClipboardList size={17} />
              <span>{t('clipboard')}</span>
            </button>
            <button className={tab === 'favorites' ? 'active' : ''} onClick={() => setTab('favorites')}>
              <Star size={17} />
              <span>{t('favoritesNav')}</span>
            </button>
            <button className={tab === 'settings' ? 'active' : ''} onClick={() => setTab('settings')}>
              <Settings size={17} />
              <span>{t('settings')}</span>
            </button>
          </nav>

          <div className="sidebar-footer">
            <div className="shortcut-hint">{pinServiceShortcutTokens.map((token) => <kbd key={token}>{token}</kbd>)}<span>{t('pinService')}</span></div>
            <div className="sidebar-action-row sidebar-action-row-split">
              <div className="sidebar-left-actions">
              <button className="square-icon-button" onClick={toggleTheme} title={isDark ? t('light') : t('dark')} aria-label={isDark ? t('light') : t('dark')}>
                {isDark ? <Sun size={17} /> : <Moon size={17} />}
              </button>
              <button className={`square-icon-button update-check-button ${updateStatus?.attention_required ? 'has-update-attention' : ''}`} onClick={checkUpdate} title={t('checkUpdate')} aria-label={t('checkUpdate')}>
                <RefreshCw size={17} />
                {updateStatus?.attention_required ? <span className="update-attention-dot" aria-hidden="true" /> : null}
              </button>
              </div>
              <button className="square-icon-button danger-exit" onClick={quitApp} title={t('quitApp')} aria-label={t('quitApp')}>
                <LogOut size={17} />
              </button>
            </div>
          </div>
        </aside>

        <section className="codex-main">
          <header className="workspace-header">
            <div className="title-block">
              <p className="eyebrow">{tab === 'favorites' ? t('favoritesEyebrow') : (tab === 'clipboard' ? t('clipboardEyebrow') : t('settingsEyebrow'))}</p>
              <h1>{pageTitle}</h1>
              <p className="page-subtitle">{pageSubtitle}</p>
            </div>
          </header>

          <div className="content-surface">
            {tab === 'clipboard' ? (
              <ClipboardPage t={t} settings={boot.settings} refreshKey={refreshKey} mode="clipboard" />
            ) : tab === 'favorites' ? (
              <ClipboardPage t={t} settings={boot.settings} refreshKey={refreshKey} mode="favorites" />
            ) : (
              <SettingsPage t={t} boot={boot} onBootChange={setBoot} updateStatus={updateStatus} onCheckUpdate={checkUpdate} />
            )}
          </div>
        </section>
      </section>
      {updateDialogOpen ? <UpdateStatusDialog t={t} status={updateStatus} onClose={dismissUpdateDialog} onInstall={installUpdate} /> : null}
    </main>
  );
}
