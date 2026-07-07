import { useEffect, useMemo, useRef, useState } from 'react';
import { BadgeCheck, Clock3, Database, Download, FolderOpen, HelpCircle, Keyboard, MapPinned, Minus, Palette, Plus, Power, RefreshCw, RotateCcw, Trash2, Upload } from 'lucide-react';
import { api } from '../api.js';
import { captureShortcutValue, formatShortcutForDisplay, normalizeShortcutForStorage } from '../shortcutDisplay.js';

function Switch({ checked, onChange }) {
  return <button className={`switch ${checked ? 'on' : ''}`} onClick={() => onChange(!checked)}><span /></button>;
}

function captureShortcut(event, setter) {
  event.preventDefault();
  const shortcut = captureShortcutValue(event);
  if (shortcut) setter(shortcut);
}

function Segmented({ value, options, onChange }) {
  return (
    <div className="segmented">
      {options.map((option) => (
        <button key={option.value} className={value === option.value ? 'active' : ''} onClick={() => onChange(option.value)}>{option.label}</button>
      ))}
    </div>
  );
}

function HelpTip({ text }) {
  if (!text) return null;
  return (
    <span className="help-tip" tabIndex="0" aria-label={text}>
      <HelpCircle size={14} />
      <span className="help-bubble">{text}</span>
    </span>
  );
}

function SettingName({ children, help }) {
  return <span className="setting-name"><span>{children}</span><HelpTip text={help} /></span>;
}

function Stepper({ value, min, max, step = 5, suffix = '', onChange, onReset, resetLabel = 'Reset' }) {
  const current = Number(value);
  const clamp = (next) => Math.min(max, Math.max(min, next));
  const update = (next) => onChange(clamp(next));
  return (
    <div className="stepper-control">
      <button type="button" aria-label="Decrease" disabled={current <= min} onClick={() => update(current - step)}><Minus size={14} /></button>
      <strong>{current}{suffix}</strong>
      <button type="button" aria-label="Increase" disabled={current >= max} onClick={() => update(current + step)}><Plus size={14} /></button>
      {onReset ? <button type="button" className="reset-stepper" aria-label={resetLabel} title={resetLabel} onClick={() => onReset()}><RotateCcw size={13} /></button> : null}
    </div>
  );
}

function PositionMap({ settings, t, onSave }) {
  const mapRef = useRef(null);
  const screenWidth = Math.max(800, window.screen?.availWidth || window.screen?.width || 1920);
  const screenHeight = Math.max(600, window.screen?.availHeight || window.screen?.height || 1080);
  const popupScale = Math.min(200, Math.max(50, Number(settings.popup_scale_percent || 100))) / 100;
  const popupWidth = Math.round(Math.min(520, Math.max(280, Number(settings.popup_width || 340))) * popupScale);
  const popupHeight = Math.round(Math.min(360, Math.max(160, Number(settings.popup_height || 220))) * popupScale);
  const mockMaxWidth = 150;
  const mockPopupWidth = Math.round(mockMaxWidth);
  const mockPopupHeight = Math.round(Math.min(104, Math.max(62, mockMaxWidth * (popupHeight / popupWidth))));
  const maxX = Math.max(0, screenWidth - popupWidth);
  const maxY = Math.max(0, screenHeight - popupHeight);
  const clamp = (value, min, max) => Math.min(max, Math.max(min, value));
  const [draft, setDraft] = useState({
    x: clamp(settings.popup_x ?? 24, 0, maxX),
    y: clamp(settings.popup_y ?? 24, 0, maxY)
  });
  const [saving, setSaving] = useState(false);

  function updateFromPointer(event) {
    const rect = mapRef.current?.getBoundingClientRect();
    if (!rect) return;
    const localX = clamp(event.clientX - rect.left, 0, rect.width);
    const localY = clamp(event.clientY - rect.top, 0, rect.height);
    const usableWidth = Math.max(1, rect.width - mockPopupWidth);
    const usableHeight = Math.max(1, rect.height - mockPopupHeight);
    const ratioX = clamp((localX - mockPopupWidth / 2) / usableWidth, 0, 1);
    const ratioY = clamp((localY - mockPopupHeight / 2) / usableHeight, 0, 1);
    const nextX = Math.round(ratioX * maxX);
    const nextY = Math.round(ratioY * maxY);
    setDraft({ x: nextX, y: nextY });
  }

  function beginDrag(event) {
    event.preventDefault();
    updateFromPointer(event);
    event.currentTarget.setPointerCapture?.(event.pointerId);
  }

  async function save() {
    setSaving(true);
    try {
      // 定位器用“屏幕尺寸减去弹窗尺寸”作为最大坐标，是为了让真实弹窗和预览弹窗都不会越出屏幕边界。
      // The locator uses screen size minus popup size as the maximum coordinate so both the real popup and preview popup stay inside screen bounds.
      await api.savePopupPosition(draft.x, draft.y);
      onSave(draft.x, draft.y);
    } finally {
      setSaving(false);
    }
  }

  const ratioX = maxX > 0 ? clamp(draft.x / maxX, 0, 1) : 0;
  const ratioY = maxY > 0 ? clamp(draft.y / maxY, 0, 1) : 0;
  const left = `calc(${ratioX * 100}% - ${ratioX * mockPopupWidth}px)`;
  const top = `calc(${ratioY * 100}% - ${ratioY * mockPopupHeight}px)`;

  return (
    <div className="position-map-card">
      <div className="position-map-copy compact-title-help">
        <strong>{t('positionMapTitle')}</strong>
        <HelpTip text={t('positionMapHint')} />
      </div>
      <div
        ref={mapRef}
        className="position-map-canvas"
        style={{ aspectRatio: `${Math.max(1, Math.round(maxX))} / ${Math.max(1, Math.round(maxY))}` }}
        onPointerDown={beginDrag}
        onPointerMove={(event) => event.buttons === 1 && updateFromPointer(event)}
      >
        <div className="position-map-grid" />
        <div className="position-map-safe-area" />
        <div className="position-map-axis x-axis">max X {Math.round(maxX)}px</div>
        <div className="position-map-axis y-axis">max Y {Math.round(maxY)}px</div>
        <div className="position-map-popup" style={{ left, top, '--mock-popup-width': `${mockPopupWidth}px`, '--mock-popup-height': `${mockPopupHeight}px` }}>
          <b>ClipAnchor</b>
          <span>{t('dragHint')}</span>
        </div>
      </div>
      <div className="position-map-footer">
        <code>X {draft.x}px · Y {draft.y}px · max X {Math.round(maxX)}px · max Y {Math.round(maxY)}px</code>
        <div className="button-row compact-actions">
          <button className="soft-button" onClick={() => setDraft({ x: 24, y: 24 })}>{t('resetPosition')}</button>
          <button className="primary-button" disabled={saving} onClick={save}>{saving ? '...' : t('confirmPosition')}</button>
        </div>
      </div>
    </div>
  );
}

const shortcutLabels = {
  toggle_pin_service: 'shortcutPinService',
  toggle_history_service: 'shortcutHistoryService',
  toggle_main_window: 'shortcutMainWindow',
  enter_light_mode: 'shortcutLiteMode',
  toggle_theme_mode: 'shortcutThemeMode'
};

const defaultShortcuts = {
  toggle_pin_service: 'Ctrl+Shift+P',
  toggle_history_service: 'Ctrl+Shift+H',
  toggle_main_window: 'Ctrl+Shift+X',
  enter_light_mode: 'Ctrl+Shift+L',
  toggle_theme_mode: 'Ctrl+Shift+T'
};

const shortcutOrder = [
  'toggle_pin_service',
  'toggle_history_service',
  'toggle_main_window',
  'enter_light_mode',
  'toggle_theme_mode'
];

function normalizeSettings(value) {
  // 旧版 settings.json 可能缺少新增字段；前端补齐默认值，是为了让升级后的设置页不因历史配置文件而失去控制项。
  // Older settings.json files may miss new fields; the frontend fills defaults so upgraded settings pages do not lose controls because of historical config files.
  return {
    ...value,
    auto_update_enabled: value?.auto_update_enabled !== false,
    log_retention_days: Number(value?.log_retention_days || 7),
    shortcuts: {
      ...defaultShortcuts,
      ...(value?.shortcuts || {})
    }
  };
}

function SettingsSoftDialog({ dialog, t, onClose }) {
  if (!dialog) return null;
  async function runConfirm() {
    const action = dialog.onConfirm;
    onClose();
    if (action) await action();
  }
  return (
    <div className="soft-modal-backdrop settings-dialog-backdrop" role="presentation" onClick={onClose}>
      <section className={`soft-modal-card settings-dialog-card ${dialog.danger ? 'danger' : ''}`} role="dialog" aria-modal="true" onClick={(event) => event.stopPropagation()}>
        <span className="settings-dialog-icon"><HelpCircle size={19} /></span>
        <div className="settings-dialog-copy">
          <strong>{dialog.title}</strong>
          <p>{dialog.message}</p>
        </div>
        <div className="settings-dialog-actions">
          {dialog.kind === 'confirm' ? <button className="soft-button" onClick={onClose}>{dialog.cancelLabel || t('cancel')}</button> : null}
          <button className={dialog.danger ? 'soft-button danger-line' : 'primary-button'} onClick={dialog.kind === 'confirm' ? runConfirm : onClose}>{dialog.confirmLabel || t('ok')}</button>
        </div>
      </section>
    </div>
  );
}

export default function SettingsPage({ t, boot, onBootChange, updateStatus, onCheckUpdate }) {
  const [settings, setSettings] = useState(() => normalizeSettings(boot.settings));
  const [dataUsage, setDataUsage] = useState(null);
  const [logStatus, setLogStatus] = useState(null);
  const [smartNoticeOpen, setSmartNoticeOpen] = useState(false);
  const [cleanupDays, setCleanupDays] = useState(30);
  const [cleanupPreservePinned, setCleanupPreservePinned] = useState(true);
  const [settingsDialog, setSettingsDialog] = useState(null);

  useEffect(() => {
    // 设置页存在本地编辑态；当快捷键从后端改变服务开关时，需要用最新 boot 设置覆盖本地态。
    // The settings page has local edit state; when shortcuts change service switches in the backend, it must mirror the newest boot settings.
    setSettings(normalizeSettings(boot.settings));
  }, [boot.settings]);

  useEffect(() => {
    api.getDataUsage().then(setDataUsage).catch(() => setDataUsage(null));
    api.getLogStatus().then(setLogStatus).catch(() => setLogStatus(null));
  }, [boot.paths.data]);

  const conflicts = useMemo(() => {
    const values = Object.values(settings.shortcuts || {}).map(normalizeShortcutForStorage);
    return new Set(values.filter((value, index) => values.indexOf(value) !== index));
  }, [settings.shortcuts]);

  async function persist(next) {
    const normalized = normalizeSettings(next);
    setSettings(normalized);
    const saved = await api.saveSettings(normalized);
    const normalizedSaved = normalizeSettings(saved);
    setSettings(normalizedSaved);
    onBootChange({ ...boot, settings: normalizedSaved });
    return normalizedSaved;
  }

  async function toggleService(name, enabled) {
    // 服务开关走专用命令，是为了和快捷键共享同一套后端状态更新与广播逻辑。
    // Service switches use dedicated commands so UI clicks and shortcuts share the same backend update and broadcast path.
    const saved = name === 'pin_service_enabled'
      ? await api.setPinService(enabled)
      : await api.setHistoryService(enabled);
    setSettings(normalizeSettings(saved));
    onBootChange({ ...boot, settings: normalizeSettings(saved) });
  }

  async function toggleAutostart(enabled) {
    const previous = settings;
    const optimistic = normalizeSettings({ ...settings, auto_start: enabled });
    // 先更新界面再调用系统接口，是为了避免 Windows 注册表写入期间让开关看起来卡住。
    // The UI updates before the system call so the switch never appears stuck while Windows writes the startup registry entry.
    setSettings(optimistic);
    try {
      const saved = await api.setAutostart(enabled);
      setSettings(normalizeSettings(saved));
      onBootChange({ ...boot, settings: normalizeSettings(saved) });
    } catch (error) {
      setSettings(normalizeSettings(previous));
      showSettingsAlert(t('autoStart'), String(error));
    }
  }

  const update = (patch) => persist({ ...settings, ...patch });
  const updateShortcuts = (key, value) => update({ shortcuts: { ...defaultShortcuts, ...(settings.shortcuts || {}), [key]: value } });

  async function updateLogRetentionDays(value) {
    const days = Math.min(90, Math.max(1, Math.floor(Number(value) || 7)));
    // 日志保留天数立即写入设置，是为了让后端下一次轮转/刷新时按用户选择清理旧归档。
    // The log retention days are saved immediately so the backend can prune old archives using the user's choice on the next rotation or refresh.
    await update({ log_retention_days: days });
    setLogStatus((previous) => previous ? { ...previous, retention_days: days } : previous);
    await refreshUsage();
  }

  async function refreshUsage() {
    api.getDataUsage().then(setDataUsage).catch(() => setDataUsage(null));
    api.getLogStatus().then(setLogStatus).catch(() => setLogStatus(null));
  }

  async function openLogFolder() {
    try {
      await api.openLogFolder();
    } catch (error) {
      showSettingsAlert(t('logManagement'), String(error));
    }
  }

  function clearLogFiles() {
    showSettingsConfirm(t('logManagement'), t('confirmClearLogs'), async () => {
      // 日志清理也走软件内确认弹窗，是为了保持数据管理区所有危险操作的交互一致性。
      // Log cleanup also uses the in-app confirmation dialog so every risky data-management action feels consistent.
      const nextStatus = await api.clearLogs();
      setLogStatus(nextStatus);
      await refreshUsage();
      showSettingsAlert(t('logManagement'), t('clearLogsDone'));
    }, true);
  }

  async function exportHistory(format) {
    const result = await api.exportHistory(format);
    if (result) await refreshUsage();
  }

  async function importHistory(format) {
    const result = await api.importHistory(format);
    if (result) {
      await refreshUsage();
    }
  }

  function showSettingsAlert(title, message) {
    setSettingsDialog({ kind: 'alert', title, message });
  }

  function showSettingsConfirm(title, message, onConfirm, danger = false) {
    // 数据管理确认统一使用软件内弹窗，是为了避免原生 Windows 提示框破坏自绘界面的视觉一致性。
    // Data-management confirmations use an in-app dialog so native Windows alerts do not break the custom-drawn UI language.
    setSettingsDialog({ kind: 'confirm', title, message, onConfirm, danger });
  }

  function clearData(preservePinned) {
    const message = preservePinned ? t('confirmClearNonPinned') : t('confirmForceClear');
    showSettingsConfirm(t('clear'), message, async () => {
      // 清空操作必须先确认再执行，是因为历史数据库位于便携 data 目录内且可能包含用户长期固定资料。
      // Clear actions require confirmation because the portable data database may hold long-lived favorite records.
      await api.clearAllData(preservePinned);
      showSettingsAlert(t('data'), t('clearDone'));
    }, true);
  }

  function deleteBeforeDays() {
    const rawDays = Number(cleanupDays);
    if (!Number.isFinite(rawDays) || rawDays < 1) {
      showSettingsAlert(t('invalidTitle'), t('cleanupDaysInvalid'));
      return;
    }
    const days = Math.floor(rawDays);
    const message = t('confirmDeleteBeforeDays').replace('{days}', String(days));
    showSettingsConfirm(t('deleteBeforeDays'), message, async () => {
      // 旧记录清理走后端按日期筛选，是为了避免前端一次性读取全部历史后再删除造成大库卡顿。
      // Old-record cleanup is filtered in the backend so the frontend does not load a large database just to delete stale rows.
      const count = await api.deleteHistoryBeforeDays(days, cleanupPreservePinned);
      await refreshUsage();
      showSettingsAlert(t('data'), t('deleteBeforeDaysDone').replace('{count}', String(count)));
    }, true);
  }

  const uiScale = Number(settings.ui_scale_percent ?? 100);
  const setUiScale = (value) => update({ ui_scale_percent: Number(value) });
  const setPopupScale = (value) => update({ popup_scale_percent: Number(value) });
  const historyLimit = Number(settings.history_limit ?? 0);
  const setHistoryLimit = (value) => update({ history_limit: Number(value) });
  const logRetentionDays = Number(settings.log_retention_days || logStatus?.retention_days || 7);
  // 日志说明按句子拆成独立行，是为了让轮转规则、保留天数和隐私边界分别被读清楚，避免长段文字误读。
  // The log hint is split into sentence lines so rotation, retention, and privacy boundaries stay readable instead of becoming one confusing paragraph.
  const logManagementHintLines = t('logManagementHint')
    .replace('{days}', String(logRetentionDays))
    .replace('{size}', String(logStatus?.max_current_file_mb || 2))
    .split('\n')
    .filter(Boolean);

  return (
    <section className="settings-scroll scroll-area">
      <div className="settings-grid refined-settings-grid compact-settings-grid">
        <div className="settings-card wide hero-card">
          <h2><Power size={18} /> {t('basic')}</h2>
          <div className="setting-stack service-grid">
            <label className="setting-row"><SettingName help={t('helpPinService')}>{t('pinService')}</SettingName><Switch checked={settings.pin_service_enabled} onChange={(v) => toggleService('pin_service_enabled', v)} /></label>
            <label className="setting-row"><SettingName help={t('helpHistoryService')}>{t('historyService')}</SettingName><Switch checked={settings.history_service_enabled} onChange={(v) => toggleService('history_service_enabled', v)} /></label>
            <label className="setting-row"><SettingName help={t('helpAutoHide')}>{t('autoHide')}</SettingName><Switch checked={settings.auto_hide_actions} onChange={(v) => update({ auto_hide_actions: v })} /></label>
            <label className="setting-row setting-row-segmented privacy-filter-row"><SettingName help={t('helpPrivacy')}>{t('privacyMode')}</SettingName><Segmented value={settings.privacy_filter_mode || (settings.privacy_mode ? 'light' : 'off')} options={[{ value: 'off', label: t('privacyOffMode') }, { value: 'light', label: t('privacyLightMode') }, { value: 'smart', label: t('privacySmartMode') }]} onChange={async (mode) => { const normalized = mode === 'smart' ? 'light' : mode; if (mode === 'smart') { setSmartNoticeOpen(true); } const saved = await api.setPrivacyFilterMode(normalized); setSettings(normalizeSettings(saved)); onBootChange({ ...boot, settings: normalizeSettings(saved) }); }} /></label>
            <label className="setting-row"><SettingName help={t('helpAutoStart')}>{t('autoStart')}</SettingName><Switch checked={settings.auto_start} onChange={toggleAutostart} /></label>
          </div>
        </div>

        <div className="settings-card wide accent-card">
          <h2><Palette size={18} /> {t('appearance')}</h2>
          <div className="appearance-controls">
            <label className="control-row"><SettingName help={t('helpLanguage')}>{t('language')}</SettingName><Segmented value={settings.locale} onChange={(v) => update({ locale: v })} options={[{ value: 'auto', label: t('autoLanguage') }, { value: 'en', label: 'English' }, { value: 'zh', label: '简体中文' }]} /></label>
            <label className="control-row"><SettingName help={t('helpTheme')}>{t('theme')}</SettingName><Segmented value={settings.theme} onChange={(v) => update({ theme: v })} options={[{ value: 'system', label: t('system') }, { value: 'dark', label: t('dark') }, { value: 'light', label: t('light') }]} /></label>
            <label className="control-row"><SettingName help={t('helpAnimation')}>{t('animation')}</SettingName><Segmented value={settings.animation_mode} onChange={(v) => update({ animation_mode: v })} options={[{ value: 'elegant', label: t('elegant') }, { value: 'performance', label: t('performance') }]} /></label>
          </div>
        </div>

        <div className="settings-card wide runtime-card">
          <h2><Clock3 size={18} /> {t('sizingAndTiming')}</h2>
          <div className="range-grid runtime-grid sizing-grid">
            <label className="scale-step-row"><SettingName help={t('helpUiScale')}>{t('scale')}</SettingName><Stepper value={uiScale} min={50} max={200} step={5} suffix="%" onChange={setUiScale} onReset={() => setUiScale(100)} resetLabel={t('resetScale')} /></label>
            <label className="scale-step-row"><SettingName help={t('helpPopupScale')}>{t('popupScale')}</SettingName><Stepper value={Number(settings.popup_scale_percent || 100)} min={50} max={200} step={5} suffix="%" onChange={setPopupScale} onReset={() => setPopupScale(100)} resetLabel={t('resetScale')} /></label>
            <label><SettingName help={t('helpAutoDestroy')}>{t('autoDestroy')} <b>{settings.auto_destroy_seconds}s</b></SettingName><input type="range" min="2" max="60" value={settings.auto_destroy_seconds} onChange={(e) => update({ auto_destroy_seconds: Number(e.target.value) })} /></label>
            <label><SettingName help={t('helpLiteDelay')}>{t('liteDelay')} <b>{settings.light_mode_minutes}m</b></SettingName><input type="range" min="1" max="180" value={settings.light_mode_minutes} onChange={(e) => update({ light_mode_minutes: Number(e.target.value) })} /></label>
          </div>
        </div>

        <div className="settings-card wide shortcut-card">
          <h2><Keyboard size={18} /> {t('shortcuts')}</h2>
          <div className="shortcut-grid">
            {shortcutOrder.map((key) => {
              const value = settings.shortcuts?.[key] || defaultShortcuts[key];
              const normalizedValue = normalizeShortcutForStorage(value);
              return (
                <label key={key}>
                  <SettingName>{t(shortcutLabels[key] || key)}</SettingName>
                  <input className={conflicts.has(normalizedValue) ? 'conflict' : ''} value={formatShortcutForDisplay(value)} onKeyDown={(e) => captureShortcut(e, (v) => updateShortcuts(key, v))} onChange={() => {}} />
                </label>
              );
            })}
          </div>
        </div>

        <div className="settings-card wide position-card">
          <h2><MapPinned size={18} /> {t('position')} <HelpTip text={t('helpPosition')} /></h2>
          <PositionMap
            settings={settings}
            t={t}
            onSave={(x, y) => {
              const next = { ...settings, popup_x: x, popup_y: y };
              setSettings(next);
              onBootChange({ ...boot, settings: next });
            }}
          />
        </div>

        <div className="settings-card wide data-card full-data-card">
          <h2><Database size={18} /> {t('data')}</h2>
          <div className="data-management-primary-row">
            <div className="data-summary-strip"><span>{t('dataUsage')}</span><strong>{dataUsage?.display || '...'}</strong></div>
            <label className="scale-step-row history-limit-row"><SettingName help={t('helpHistoryLimit')}>{t('historyLimit')}</SettingName><Stepper value={historyLimit} min={0} max={10000} step={100} suffix={historyLimit === 0 ? ` ${t('unlimited')}` : ''} onChange={setHistoryLimit} onReset={() => setHistoryLimit(0)} resetLabel={t('resetScale')} /></label>
          </div>
          <label className="vertical"><SettingName>{t('dbPath')}</SettingName><input readOnly value={boot.paths.database} /></label>
          <div className="old-history-cleanup">
            <label className="cleanup-days-field"><SettingName help={t('helpDeleteBeforeDays')}>{t('deleteBeforeDays')}</SettingName><input type="number" min="1" step="1" value={cleanupDays} onChange={(event) => setCleanupDays(event.target.value)} /></label>
            <label className="setting-row cleanup-preserve-toggle"><SettingName>{t('preserveFavorites')}</SettingName><Switch checked={cleanupPreservePinned} onChange={setCleanupPreservePinned} /></label>
            <button className="soft-button danger-line" onClick={deleteBeforeDays}>{t('deleteBeforeDaysAction').replace('{days}', String(Math.max(1, Math.floor(Number(cleanupDays) || 1))))}</button>
          </div>
          <div className="data-actions-layout">
            <div className="button-row data-actions-main import-export-actions">
              <button className="soft-button" onClick={() => exportHistory('json')}><Download size={16} /> {t('exportJson')}</button>
              <button className="soft-button" onClick={() => exportHistory('csv')}><Download size={16} /> {t('exportCsv')}</button>
              <button className="soft-button" onClick={() => importHistory('json')}><Upload size={16} /> {t('importJson')}</button>
              <button className="soft-button" onClick={() => importHistory('csv')}><Upload size={16} /> {t('importCsv')}</button>
            </div>
            <div className="button-row danger-actions compact-danger-actions">
              <button className="soft-button danger-line" title={t('helpClearNonPinned')} onClick={() => clearData(true)}>{t('clearNonPinned')}</button>
              <button className="soft-button danger-line force-clear" title={t('helpForceClear')} onClick={() => clearData(false)}>{t('clearIncludingPinned')}</button>
            </div>
          </div>
        </div>

        <div className="settings-card wide log-card">
          <h2><Database size={18} /> {t('logManagement')}</h2>
          <div className="log-management-panel">
            <div className="log-management-header">
              <div className="log-management-copy">
                <strong>{t('logManagementTitle')}</strong>
                <div className="log-management-hint-lines">
                  {logManagementHintLines.map((line, index) => <p key={`${index}-${line}`}>{line}</p>)}
                </div>
              </div>
              <span className="log-size-pill">{logStatus?.display_size || '...'}</span>
            </div>
            <div className="log-control-grid">
              <label className="scale-step-row log-retention-row"><SettingName help={t('helpLogRetentionDays')}>{t('logRetentionDays')}</SettingName><Stepper value={logRetentionDays} min={1} max={90} step={1} suffix={` ${t('days')}`} onChange={updateLogRetentionDays} onReset={() => updateLogRetentionDays(7)} resetLabel={t('resetScale')} /></label>
              <label className="vertical log-path-field"><SettingName>{t('logPath')}</SettingName><input readOnly value={logStatus?.directory || boot.paths.logs || ''} /></label>
            </div>
            <div className="log-file-strip">
              {(logStatus?.files || []).slice(0, 3).map((file) => (
                <span key={file.path} title={file.path}>{file.name} · {file.display_size}</span>
              ))}
              {(logStatus?.files || []).length === 0 ? <span>{t('noLogFiles')}</span> : null}
            </div>
            <div className="button-row log-actions">
              <button className="soft-button" onClick={openLogFolder}><FolderOpen size={16} /> {t('openLogFolder')}</button>
              <button className="soft-button" onClick={refreshUsage}><RotateCcw size={16} /> {t('refreshLogs')}</button>
              <button className="soft-button danger-line" onClick={clearLogFiles}><Trash2 size={16} /> {t('clearLogs')}</button>
            </div>
          </div>
        </div>


        <div className="settings-card wide version-card">
          <h2><BadgeCheck size={18} /> {t('versionAndUpdates')}</h2>
          <div className="version-update-panel">
            <div className="version-copy-block">
              <span>{t('softwareVersion')}</span>
              <strong>ClipAnchor v{boot.app_version || updateStatus?.current_version || ''}</strong>
              <p>{updateStatus?.attention_required ? t('updateAttentionHint') : t('updateQuietHint')}</p>
            </div>
            <label className="setting-row version-auto-row"><SettingName help={t('helpAutoUpdate')}>{t('autoUpdate')}</SettingName><Switch checked={settings.auto_update_enabled !== false} onChange={(v) => update({ auto_update_enabled: v })} /></label>
            <button className={`soft-button version-check-button ${updateStatus?.attention_required ? 'has-update-attention' : ''}`} onClick={onCheckUpdate}>
              <RefreshCw size={16} /> {t('checkUpdate')}
              {updateStatus?.attention_required ? <span className="update-attention-dot" aria-hidden="true" /> : null}
            </button>
          </div>
        </div>
      </div>
      <SettingsSoftDialog dialog={settingsDialog} t={t} onClose={() => setSettingsDialog(null)} />
      {smartNoticeOpen ? (
        <div className="soft-modal-backdrop" role="presentation" onClick={() => setSmartNoticeOpen(false)}>
          <section className="soft-modal-card privacy-smart-card" role="dialog" aria-modal="true" onClick={(event) => event.stopPropagation()}>
            <strong>{t('privacySmartTitle')}</strong>
            <p>{t('privacySmartUnavailable')}</p>
            <button className="primary-button" onClick={() => setSmartNoticeOpen(false)}>{t('ok')}</button>
          </section>
        </div>
      ) : null}
    </section>
  );
}
