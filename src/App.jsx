import { useEffect, useMemo, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { Activity, CheckCircle2, ClipboardList, Download, Info, Loader2, LogOut, Maximize2, Minus, Moon, RefreshCw, Settings, Sparkles, Star, Sun, X } from 'lucide-react';
import { api } from './api.js';
import { createTranslator, getReferenceMessages } from './i18n.js';
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
    <header className="native-titlebar" data-tauri-drag-region>
      <div className="native-titlebar-drag" data-tauri-drag-region />
      <div className="native-window-controls" aria-label="Window controls">
        {/* 主窗口顶部不再展示应用名称，避免形成突兀的系统标题栏感；右上角仅保留一组轻量悬浮控制按钮。 */}
        {/* The main window no longer shows the app name at the top, avoiding a harsh system-titlebar feel while keeping only lightweight floating controls. */}
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
  const [isWindowMaximized, setIsWindowMaximized] = useState(false);
  const [languagePacks, setLanguagePacks] = useState([]);
  const systemPrefersDark = useSystemThemePreference();
  const activeThemeClass = resolveThemeClass(boot?.settings || { theme: 'system' }, systemPrefersDark);
  useDocumentThemeSync(activeThemeClass);

  useEffect(() => {
    const requiredLanguageMessages = getReferenceMessages('en');
    Promise.all([api.bootstrap(), api.listLanguagePacks(requiredLanguageMessages).catch(() => [])]).then(([payload, packs]) => {
      setBoot(payload);
      setLanguagePacks(Array.isArray(packs) ? packs : []);
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
        // 只有事件确实抵达可见主界面后才消费提示位，是为了既不丢失后台检查结果，也不让同一提示在以后每次唤醒时重复出现。
        // The prompt bit is consumed only after the event reaches a visible main window, preserving missed background results while preventing the same prompt from reopening on every later wake.
        api.dismissUpdatePrompt()
          .then((persisted) => setUpdateStatus((current) => ({ ...(persisted || {}), ...(current || {}), prompt_on_main_open: false })))
          .catch((error) => console.error('ClipAnchor update prompt consumption failed:', error));
      }
    });
    const activationUnlistenPromise = listen('clipanchor-main-window-activated', async () => {
      try {
        // 主界面从长时间轻量模式恢复时主动重新拉取基础状态，是为了避免隐藏期间的设置、更新和历史变更让界面停留在旧快照。
        // When the main UI returns from a long Lite-mode session, refreshing bootstrap state prevents settings, update, and history views from showing stale snapshots.
        const [payload, status, packs] = await Promise.all([api.bootstrap(), api.getUpdateStatus(), api.listLanguagePacks(requiredLanguageMessages).catch(() => [])]);
        setBoot(payload);
        setLanguagePacks(Array.isArray(packs) ? packs : []);
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
    if (route.view === 'popup') return undefined;
    let disposed = false;
    let resizeTimer = 0;
    const appWindow = getCurrentWindow();

    async function syncWindowMaximizedState() {
      try {
        const maximized = await appWindow.isMaximized();
        if (!disposed) setIsWindowMaximized(Boolean(maximized));
      } catch (error) {
        // 当桌面 API 在极少数运行时不可用时，使用视口尺寸兜底，避免最大化后仍保留透明圆角露出桌面。
        // When the desktop API is unavailable in rare runtimes, viewport size is a fallback so maximized windows do not keep transparent rounded corners.
        const nearFullScreen = window.innerWidth >= window.screen.availWidth - 2 && window.innerHeight >= window.screen.availHeight - 2;
        if (!disposed) setIsWindowMaximized(nearFullScreen);
      }
    }

    function scheduleWindowStateSync() {
      window.clearTimeout(resizeTimer);
      resizeTimer = window.setTimeout(syncWindowMaximizedState, 40);
    }

    syncWindowMaximizedState();
    window.addEventListener('resize', scheduleWindowStateSync);
    const resizedUnlistenPromise = typeof appWindow.onResized === 'function'
      ? appWindow.onResized(scheduleWindowStateSync).catch(() => null)
      : Promise.resolve(null);
    return () => {
      disposed = true;
      window.clearTimeout(resizeTimer);
      window.removeEventListener('resize', scheduleWindowStateSync);
      resizedUnlistenPromise.then((unlisten) => { if (unlisten) unlisten(); }).catch(() => {});
    };
  }, [route.view]);

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

  useEffect(() => {
    if (route.view === 'popup') return undefined;
    function handleCommandW(event) {
      const key = String(event.key || '').toLowerCase();
      if (!event.metaKey || event.ctrlKey || event.altKey || event.shiftKey || key !== 'w') return;
      // macOS 的 Command+W 是关闭当前窗口的系统习惯；这里将它固定为隐藏主界面，保留后台监听、托盘和全局快捷键。
      // Command+W is the macOS convention for closing the current window; ClipAnchor maps it to hiding the main UI while keeping monitoring, tray, and shortcuts alive.
      event.preventDefault();
      event.stopPropagation();
      api.closeMainWindow().catch((error) => console.error('ClipAnchor Command+W hide failed:', error));
    }
    window.addEventListener('keydown', handleCommandW, true);
    return () => {
      window.removeEventListener('keydown', handleCommandW, true);
    };
  }, [route.view]);

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

  const t = createTranslator(boot.settings.locale, languagePacks);
  const themeClass = activeThemeClass;
  const uiScale = Math.min(2, Math.max(0.5, Number(boot.settings.ui_scale_percent || 100) / 100));
  const motionClass = boot.settings.animation_mode === 'performance' ? 'motion-performance' : 'motion-elegant';
  const isDark = themeClass === 'theme-dark';

  async function toggleTheme() {
    // 主题切换直接写入共享设置，是为了让主窗口、弹窗和下次启动保持同一套视觉状态。
    // Theme changes are persisted through shared settings so the main window, popups, and next launch keep one visual state.
    const nextTheme = isDark ? 'light' : 'dark';
    const previousSettings = boot.settings;
    // 先更新 React 状态可让 Linux WebView 立即切换；保存失败时再回滚，避免系统调用延迟让按钮看似无效。
    // Updating React first makes the Linux WebView switch immediately; a failed save rolls back instead of making the button appear unresponsive during system calls.
    setBoot({ ...boot, settings: { ...boot.settings, theme: nextTheme } });
    try {
      const saved = await api.saveSettings({ ...boot.settings, theme: nextTheme });
      setBoot((current) => ({ ...current, settings: saved }));
    } catch (error) {
      setBoot((current) => ({ ...current, settings: previousSettings }));
      console.error('ClipAnchor theme switch failed:', error);
    }
  }

  async function checkUpdate() {
    const updateBusy = ['checking', 'downloading', 'installing'].includes(updateStatus?.status);
    if (updateBusy) {
      // 连续点击只重新打开当前进度，而不再发送新请求，是为了与后端单飞锁共同阻断重复检查和重复下载。
      // Repeated clicks only reopen the current progress instead of issuing another request, complementing the backend single-flight guard against duplicate checks and downloads.
      setUpdateDialogOpen(true);
      return;
    }
    // 手动检查先打开进度页，是为了让网络慢或 GitHub 响应慢时用户也能明确知道按钮已经生效。
    // Manual checks open the progress page first so users know the click worked even when network or GitHub responses are slow.
    setUpdateStatus({ status: 'checking', service_enabled: true, prompt_on_main_open: false, attention_required: false, current_version: updateStatus?.current_version || '' });
    setUpdateDialogOpen(true);
    try {
      const status = await api.checkUpdate('manual');
      setUpdateStatus(status);
    } catch (error) {
      console.error('ClipAnchor update check failed:', error);
      // 手动检查窗口已经可见，因此错误无需写入下次启动提示位，否则会把一次网络错误变成重复弹窗。
      // The manual-check window is already visible, so an error must not set a next-launch prompt bit that would turn one network failure into repeated dialogs.
      setUpdateStatus({ status: 'update_failed', update_failed: true, attention_required: false, prompt_on_main_open: false, message: String(error) });
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
  const PageIcon = tab === 'favorites' ? Star : (tab === 'clipboard' ? ClipboardList : Settings);
  const privacyMode = boot.settings.privacy_filter_mode || (boot.settings.privacy_mode ? 'light' : 'off');
  const privacyStatus = privacyMode === 'off' ? t('privacyOff') : `${t('privacyOn')} · ${privacyMode === 'smart' ? t('privacySmartMode') : t('privacyLightMode')}`;
  // 侧边栏快捷键徽标只在后端声明支持时显示，并使用平台化键名，避免 Linux 暴露无效入口或 macOS 用户误认 Ctrl。
  // The sidebar shortcut badge appears only when the backend declares support and uses platform-aware labels, avoiding an invalid Linux entry and Ctrl confusion on macOS.
  const globalShortcutsSupported = boot.capabilities?.global_shortcuts_supported !== false;
  const pinServiceShortcutTokens = globalShortcutsSupported
    ? shortcutDisplayTokens(boot.settings.shortcuts?.toggle_pin_service || 'Ctrl+Shift+P')
    : [];

  return (
    <main className={`app-shell codex-shell ${themeClass} ${motionClass} ${isWindowMaximized ? 'window-maximized' : ''}`} style={{ '--ui-scale': uiScale }}>
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
            {globalShortcutsSupported ? (
              <div className="shortcut-hint">{pinServiceShortcutTokens.map((token) => <kbd key={token}>{token}</kbd>)}<span>{t('pinService')}</span></div>
            ) : null}
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
          <header className="workspace-header compact-workspace-header">
            {/* 页面外壳只保留统一的图标与名词型标题，是为了消除重复说明并让三个主页面在相同位置形成稳定的导航锚点。 */}
            {/* The page shell keeps only a consistent icon and noun-style title so repeated explanations disappear and all three main views share one stable navigation anchor. */}
            <div className="workspace-heading">
              <span className="workspace-heading-icon" aria-hidden="true"><PageIcon size={18} strokeWidth={1.8} /></span>
              <h1>{pageTitle}</h1>
            </div>
          </header>

          <div className="content-surface">
            {tab === 'clipboard' ? (
              <ClipboardPage t={t} settings={boot.settings} refreshKey={refreshKey} mode="clipboard" />
            ) : tab === 'favorites' ? (
              <ClipboardPage t={t} settings={boot.settings} refreshKey={refreshKey} mode="favorites" />
            ) : (
              <SettingsPage t={t} boot={boot} onBootChange={setBoot} updateStatus={updateStatus} onCheckUpdate={checkUpdate} languagePacks={languagePacks} onLanguagePacksChange={setLanguagePacks} />
            )}
          </div>
        </section>
      </section>
      {updateDialogOpen ? <UpdateStatusDialog t={t} status={updateStatus} onClose={dismissUpdateDialog} onInstall={installUpdate} /> : null}
    </main>
  );
}
