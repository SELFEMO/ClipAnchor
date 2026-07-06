import { useEffect, useMemo, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { Activity, ClipboardList, Info, LogOut, Maximize2, Minus, Moon, RefreshCw, Settings, Sparkles, Star, Sun, X } from 'lucide-react';
import { api } from './api.js';
import { createTranslator } from './i18n.js';
import { resolveThemeClass, useDocumentThemeSync, useSystemThemePreference } from './theme.js';
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

function updateDialogCopy(t, status) {
  const key = status?.status || 'service_unavailable';
  if (key === 'update_available') {
    return { title: t('updateAvailableTitle'), message: t('updateAvailableMessage'), detail: t('updateAvailableDetail') };
  }
  if (key === 'update_failed') {
    return { title: t('updateFailedTitle'), message: t('updateFailedMessage'), detail: t('updateFailedDetail') };
  }
  return { title: t('updatePlaceholderTitle'), message: t('updatePlaceholderMessage') };
}

function UpdatePlaceholderDialog({ t, status, onClose }) {
  const copy = updateDialogCopy(t, status);
  return (
    <div className="soft-modal-backdrop update-placeholder-backdrop" role="presentation" onMouseDown={onClose}>
      <section className="soft-modal-card update-placeholder-card" role="dialog" aria-modal="true" onMouseDown={(event) => event.stopPropagation()}>
        <span className="settings-dialog-icon update-placeholder-icon"><Info size={19} /></span>
        <div className="settings-dialog-copy update-placeholder-copy">
          <strong>{copy.title}</strong>
          <p>{copy.message}</p>
          {copy.detail ? <p className="update-placeholder-note">{copy.detail}</p> : null}
        </div>
        <div className="settings-dialog-actions update-placeholder-actions">
          <button className="primary-button" onClick={onClose}>{t('ok')}</button>
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
    return () => {
      historyUnlistenPromise.then((unlisten) => unlisten()).catch(() => {});
      settingsUnlistenPromise.then((unlisten) => unlisten()).catch(() => {});
      updateUnlistenPromise.then((unlisten) => unlisten()).catch(() => {});
    };
  }, []);

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
    // 手动检查更新仍走后端命令，是为了让占位状态、未来真实更新检查和操作日志保持同一条链路。
    // Manual update checks still go through the backend so placeholder status, future real checks, and operation logs share one path.
    try {
      const status = await api.checkUpdate('manual');
      setUpdateStatus(status);
    } catch (error) {
      console.error('ClipAnchor update placeholder failed:', error);
      setUpdateStatus({ status: 'update_failed', update_failed: true, attention_required: true, prompt_on_main_open: true });
    } finally {
      setUpdateDialogOpen(true);
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
            <div className="shortcut-hint"><kbd>Ctrl</kbd><kbd>Shift</kbd><kbd>P</kbd><span>{t('pinService')}</span></div>
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
              <SettingsPage t={t} boot={boot} onBootChange={setBoot} />
            )}
          </div>
        </section>
      </section>
      {updateDialogOpen ? <UpdatePlaceholderDialog t={t} status={updateStatus} onClose={() => setUpdateDialogOpen(false)} /> : null}
    </main>
  );
}
