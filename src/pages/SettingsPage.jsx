import { useEffect, useMemo, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { BadgeCheck, Clock3, Database, Download, FolderOpen, HelpCircle, Keyboard, MapPinned, Minus, Palette, Plus, Power, RefreshCw, RotateCcw, Trash2, TriangleAlert, Upload } from 'lucide-react';
import { api } from '../api.js';
import { detectSystemLanguageCode, getReferenceMessages, inferLanguageLabel, listLanguageChoices, normalizeLocaleCode } from '../i18n.js';
import { captureShortcutValue, formatShortcutForDisplay, normalizeShortcutForStorage } from '../shortcutDisplay.js';
import { useTransientScrollbar } from '../useTransientScrollbar.js';

function Switch({ checked, onChange }) {
  return <button className={`switch ${checked ? 'on' : ''}`} onClick={() => onChange(!checked)}><span /></button>;
}

function captureShortcut(event, setter) {
  event.preventDefault();
  const shortcut = captureShortcutValue(event);
  if (shortcut) setter(shortcut);
}

function Segmented({ value, options, onChange, className = '' }) {
  const classes = ['segmented', className].filter(Boolean).join(' ');
  return (
    <div className={classes}>
      {options.map((option) => (
        <button
          key={option.value}
          type="button"
          className={value === option.value ? 'active' : ''}
          title={option.label}
          onClick={() => onChange(option.value)}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}

function DropdownSelect({ value, options, onChange, disabled = false, ariaLabel = '' }) {
  const [open, setOpen] = useState(false);
  const current = options.find((option) => option.value === value) || options[0];

  function choose(optionValue) {
    setOpen(false);
    if (optionValue !== value) onChange(optionValue);
  }

  return (
    <div
      className={`codex-dropdown ${open ? 'open' : ''}`}
      onBlur={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget)) setOpen(false);
      }}
    >
      <button
        type="button"
        className="codex-dropdown-button"
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-label={ariaLabel}
        disabled={disabled}
        onClick={() => setOpen((next) => !next)}
      >
        <span>{current?.label || value}</span>
        <i aria-hidden="true">⌄</i>
      </button>
      {open ? (
        <div className="codex-dropdown-menu" role="listbox">
          {options.map((option) => (
            <button
              key={option.value}
              type="button"
              role="option"
              aria-selected={option.value === value}
              className={option.value === value ? 'selected' : ''}
              onMouseDown={(event) => event.preventDefault()}
              onClick={() => choose(option.value)}
            >
              <span>{option.label}</span>
              {option.value === value ? <em>✓</em> : null}
            </button>
          ))}
        </div>
      ) : null}
    </div>
  );
}

function estimateHelpBubbleWidth(text) {
  const content = Array.from(String(text || '').trim());
  const weightedLength = content.reduce((sum, char) => {
    if (/\s/.test(char)) return sum + 0.32;
    if (/[\u2e80-\u9fff\uff00-\uffef]/.test(char)) return sum + 1.05;
    if (/[A-Z0-9]/.test(char)) return sum + 0.72;
    return sum + 0.56;
  }, 0);
  const hasCjk = content.some((char) => /[\u2e80-\u9fff\uff00-\uffef]/.test(char));
  const targetLineUnits = hasCjk ? 18 : 34;
  const estimatedLines = Math.max(1, Math.ceil(weightedLength / targetLineUnits));
  const balancedUnits = Math.ceil(weightedLength / Math.min(3, estimatedLines));
  const preferred = Math.round(42 + balancedUnits * (hasCjk ? 13.5 : 8.2));
  const viewportMax = Math.max(180, window.innerWidth - 36);
  // 气泡按“预计行数 + 文本长度”估算宽度，而不是固定宽度，避免短文案过宽、长文案最后一行只剩一两个字。
  // Bubble width is estimated from expected line count plus text length instead of a fixed value, avoiding oversized short hints and one-word trailing lines.
  return Math.min(Math.max(168, preferred), Math.min(360, viewportMax));
}

function calculateHelpBubblePlacement(rect, width) {
  const margin = 18;
  const center = rect.left + rect.width / 2;
  const viewportWidth = window.innerWidth;
  const normalized = Math.max(-1, Math.min(1, (center - viewportWidth / 2) / Math.max(1, viewportWidth / 2)));
  const anchorRatio = Math.max(0.42, Math.min(0.58, 0.5 + normalized * 0.13));
  const unclampedLeft = center - width * anchorRatio;
  const left = Math.min(Math.max(margin, unclampedLeft), viewportWidth - width - margin);
  const actualAnchor = Math.max(20, Math.min(width - 20, center - left));
  const align = actualAnchor < width * 0.45 ? 'left' : actualAnchor > width * 0.55 ? 'right' : 'center';
  return { left, actualAnchor, align };
}

function HelpTip({ text }) {
  const tipRef = useRef(null);
  const [bubble, setBubble] = useState(null);
  if (!text) return null;

  function showBubble() {
    const rect = tipRef.current?.getBoundingClientRect();
    if (!rect) return;
    const width = estimateHelpBubbleWidth(text);
    const { left, actualAnchor, align } = calculateHelpBubblePlacement(rect, width);
    const fitsAbove = rect.top > 92;
    setBubble({
      left,
      top: fitsAbove ? rect.top - 10 : rect.bottom + 10,
      width,
      anchorX: actualAnchor,
      align,
      placement: fitsAbove ? 'top' : 'bottom'
    });
  }

  function hideBubble() {
    setBubble(null);
  }

  return (
    <>
      <span
        ref={tipRef}
        className="help-tip"
        tabIndex="0"
        aria-label={text}
        onMouseEnter={showBubble}
        onMouseLeave={hideBubble}
        onFocus={showBubble}
        onBlur={hideBubble}
      >
        <HelpCircle size={14} />
      </span>
      {bubble ? createPortal(
        <span
          className={`help-bubble floating-help-bubble ${bubble.placement === 'bottom' ? 'below' : 'above'} align-${bubble.align}`}
          style={{ left: `${bubble.left}px`, top: `${bubble.top}px`, width: `${bubble.width}px`, '--help-anchor-x': `${bubble.anchorX}px` }}
        >
          {text}
        </span>,
        document.body
      ) : null}
    </>
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
  const provider = normalizeTranslationProvider(value?.translation_api_provider, value?.translation_api_url);
  const storedKeys = { ...(value?.translation_api_keys || {}) };
  if (!Object.prototype.hasOwnProperty.call(storedKeys, provider) && value?.translation_api_key) {
    storedKeys[provider] = String(value.translation_api_key);
  }
  const activeKey = String(storedKeys[provider] || '');
  // 旧版 settings.json 可能缺少新增字段；前端补齐默认值，是为了让升级后的设置页不因历史配置文件而失去控制项。
  // Older settings.json files may miss new fields; the frontend fills defaults so upgraded settings pages do not lose controls because of historical config files.
  return {
    ...value,
    locale: value?.locale === 'auto' ? 'auto' : (normalizeLocaleCode(value?.locale || 'auto') || 'auto'),
    auto_update_enabled: value?.auto_update_enabled !== false,
    translation_api_provider: provider,
    translation_api_url: getTranslationProvider(provider).endpoint,
    translation_api_key: activeKey,
    translation_api_keys: storedKeys,
    auto_destroy_seconds: Number(value?.auto_destroy_seconds ?? 3),
    log_retention_days: Number(value?.log_retention_days || 7),
    shortcuts: {
      ...defaultShortcuts,
      ...(value?.shortcuts || {})
    }
  };
}


function formatAutostartError(error, t) {
  const detail = String(error || '').replace(/^MACOS_LOGIN_ITEM_FAILED:/, '').trim();
  const isMacLoginItemError = String(error || '').includes('MACOS_LOGIN_ITEM_FAILED')
    || detail.includes('System Events')
    || detail.includes('login item')
    || detail.includes('AppleEvent')
    || detail.includes('not authorized')
    || detail.includes('-1743')
    || detail.includes('-1728');
  if (!isMacLoginItemError) return detail || String(error || '');
  const template = t('macosLoginItemError');
  return template.replace('{detail}', detail || t('unknownError'));
}

function SettingsSoftDialog({ dialog, t, onClose }) {
  if (!dialog) return null;
  const DialogIcon = dialog.icon === 'warning' ? TriangleAlert : (dialog.icon === 'update' ? RefreshCw : HelpCircle);
  const iconClass = dialog.icon === 'warning' ? 'warning' : (dialog.icon === 'update' ? 'update' : '');
  async function runConfirm() {
    const action = dialog.onConfirm;
    onClose();
    if (action) await action();
  }
  async function runCancel() {
    const action = dialog.onCancel;
    onClose();
    // “稍后处理”只在用户明确按下该按钮时执行后续动作，点击遮罩仍保持纯关闭，避免意外切换语言。
    // “Handle later” runs its follow-up only from the explicit button; backdrop dismissal remains a plain close to avoid accidental locale switches.
    if (action) await action();
  }
  return (
    <div className="soft-modal-backdrop settings-dialog-backdrop" role="presentation" onClick={onClose}>
      <section className={`soft-modal-card settings-dialog-card ${dialog.danger ? 'danger' : ''}`} role="dialog" aria-modal="true" onClick={(event) => event.stopPropagation()}>
        <span className={`settings-dialog-icon ${iconClass}`}><DialogIcon size={19} /></span>
        <div className="settings-dialog-copy">
          <strong>{dialog.title}</strong>
          <p>{dialog.message}</p>
        </div>
        <div className="settings-dialog-actions">
          {dialog.kind === 'confirm' ? <button className="soft-button" onClick={runCancel}>{dialog.cancelLabel || t('cancel')}</button> : null}
          <button className={dialog.danger ? 'soft-button danger-line' : 'primary-button'} onClick={dialog.kind === 'confirm' ? runConfirm : onClose}>{dialog.confirmLabel || t('ok')}</button>
        </div>
      </section>
    </div>
  );
}


const defaultTranslationProvider = 'uapis';
const translationProviders = {
  mymemory: {
    id: 'mymemory',
    endpoint: 'https://api.mymemory.translated.net/get',
    logName: 'MyMemory public translation API',
    supportsApiKey: false
  },
  uapis: {
    id: 'uapis',
    endpoint: 'https://uapis.cn/api/v1/translate/text',
    logName: 'UAPI translate API',
    supportsApiKey: true
  }
};

function normalizeTranslationProvider(value, legacyUrl = '') {
  const normalized = String(value || '').trim().toLowerCase();
  if (translationProviders[normalized]) return normalized;
  const legacy = String(legacyUrl || '').toLowerCase();
  if (legacy.includes('uapis.cn')) return 'uapis';
  return defaultTranslationProvider;
}

function getTranslationProvider(value, legacyUrl = '') {
  return translationProviders[normalizeTranslationProvider(value, legacyUrl)] || translationProviders[defaultTranslationProvider];
}

function providerNameFromId(value, legacyUrl = '') {
  return getTranslationProvider(value, legacyUrl).logName;
}

function mapTranslationTargetCode(code, providerId = defaultTranslationProvider) {
  const normalized = normalizeLocaleCode(code);
  if (normalized === 'zh-Hant' || normalized === 'zh-TW' || normalized.startsWith('zh-Hant-')) return 'zh-TW';
  if (normalized === 'zh-Hans' || normalized === 'zh-CN' || normalized.startsWith('zh-Hans-')) return providerId === 'uapis' ? 'zh' : 'zh-CN';
  return normalized;
}

function isBuiltInLanguageCode(code) {
  const normalized = normalizeLocaleCode(code);
  return normalized === 'en'
    || normalized.startsWith('en-')
    || normalized === 'zh'
    || normalized === 'zh-CN'
    || normalized === 'zh-Hans'
    || normalized.startsWith('zh-Hans-');
}

function languageSourceFingerprint(value) {
  // 中文：前后端使用相同的 FNV-1a 指纹，是为了让语言文件可离线检查，并准确判断哪一条英文源文案发生了变化。
  // English: Frontend and backend share the same FNV-1a fingerprint so language files can be inspected offline and each changed English source string is identified precisely.
  let hash = 0x811c9dc5;
  const bytes = new TextEncoder().encode(String(value || ''));
  for (const byte of bytes) {
    hash ^= byte;
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash.toString(16).padStart(8, '0');
}

export default function SettingsPage({ t, boot, onBootChange, updateStatus, onCheckUpdate, languagePacks = [], onLanguagePacksChange = () => {} }) {
  const [settings, setSettings] = useState(() => normalizeSettings(boot.settings));
  const [dataUsage, setDataUsage] = useState(null);
  const [logStatus, setLogStatus] = useState(null);
  const [smartNoticeOpen, setSmartNoticeOpen] = useState(false);
  const [cleanupDays, setCleanupDays] = useState(30);
  const [cleanupPreservePinned, setCleanupPreservePinned] = useState(true);
  const [settingsDialog, setSettingsDialog] = useState(null);
  const [languageCodeDraft, setLanguageCodeDraft] = useState('');
  const [translationApiKeyDraft, setTranslationApiKeyDraft] = useState(() => String(settings.translation_api_key || ''));
  const [languageGenerationState, setLanguageGenerationState] = useState({ busy: false, message: '', error: false, current: 0, total: 0, percent: 0 });
  const settingsScrollRef = useTransientScrollbar();
  const languagePackGridRef = useTransientScrollbar();

  useEffect(() => {
    // 设置页存在本地编辑态；当快捷键从后端改变服务开关时，需要用最新 boot 设置覆盖本地态。
    // The settings page has local edit state; when shortcuts change service switches in the backend, it must mirror the newest boot settings.
    const normalized = normalizeSettings(boot.settings);
    setSettings(normalized);
    setTranslationApiKeyDraft(String(normalized.translation_api_key || ''));
  }, [boot.settings]);

  useEffect(() => {
    api.getDataUsage().then(setDataUsage).catch(() => setDataUsage(null));
    api.getLogStatus().then(setLogStatus).catch(() => setLogStatus(null));
  }, [boot.paths.data]);

  const conflicts = useMemo(() => {
    const values = Object.values(settings.shortcuts || {}).map(normalizeShortcutForStorage);
    return new Set(values.filter((value, index) => values.indexOf(value) !== index));
  }, [settings.shortcuts]);

  const languageChoices = useMemo(() => listLanguageChoices(languagePacks), [languagePacks]);
  const coreLanguageOptions = useMemo(() => ([
    { value: 'auto', label: t('autoLanguage') },
    { value: 'en', label: 'English' },
    { value: 'zh', label: '简体中文' }
  ]), [t]);
  const extraLanguageOptions = useMemo(() => languageChoices.filter((item) => !['en', 'zh'].includes(item.code)), [languageChoices]);
  const activeTranslationProvider = getTranslationProvider(settings.translation_api_provider, settings.translation_api_url);
  const referenceLanguageMessages = useMemo(() => getReferenceMessages('en'), []);

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
      showSettingsAlert(t('autoStart'), formatAutostartError(error, t));
    }
  }

  const update = (patch) => persist({ ...settings, ...patch });

  async function chooseLocale(locale) {
    const normalized = locale === 'auto' ? 'auto' : normalizeLocaleCode(locale);
    const provider = ['auto', 'en', 'zh'].includes(normalized) ? 'built-in' : 'runtime-pack';
    // 切换语言前后都写入轻量日志，是为了让“语言包是否被激活”可排查，同时不记录任何实际界面文案。
    // Lightweight logs are written before and after locale switching so activation can be diagnosed without storing any UI copy.
    await api.logLanguagePackEvent('activate_requested', normalized, provider, true, 'settings-ui').catch(() => {});
    const saved = await update({ locale: normalized });
    await api.logLanguagePackEvent('activate_saved', saved.locale, provider, true, 'settings-ui').catch(() => {});
    return saved;
  }

  async function saveTranslationProvider(providerId) {
    const previousProvider = getTranslationProvider(settings.translation_api_provider, settings.translation_api_url);
    const normalized = normalizeTranslationProvider(providerId, settings.translation_api_url);
    const provider = getTranslationProvider(normalized);
    const storedKeys = {
      ...(settings.translation_api_keys || {}),
      [previousProvider.id]: previousProvider.supportsApiKey ? String(translationApiKeyDraft || '').trim() : ''
    };
    const nextKey = provider.supportsApiKey ? String(storedKeys[provider.id] || '') : '';
    setTranslationApiKeyDraft(nextKey);
    // 服务商切换时同时切换对应密钥，是为了避免把 UAPI 凭据继续显示或发送给无需密钥的 MyMemory 接口。
    // Switching providers also switches the matching key so UAPI credentials are never left visible or sent to keyless MyMemory requests.
    const saved = await update({
      translation_api_provider: normalized,
      translation_api_url: provider.endpoint,
      translation_api_keys: storedKeys,
      translation_api_key: nextKey
    });
    await api.logLanguagePackEvent('translation_provider_saved', '', provider.logName, true, normalized === defaultTranslationProvider ? 'default-provider' : 'selected-provider').catch(() => {});
    return saved;
  }

  async function resetTranslationProvider() {
    const provider = getTranslationProvider(defaultTranslationProvider);
    setTranslationApiKeyDraft('');
    const saved = await update({
      translation_api_provider: defaultTranslationProvider,
      translation_api_url: provider.endpoint,
      translation_api_keys: {},
      translation_api_key: ''
    });
    await api.logLanguagePackEvent('translation_provider_reset', '', provider.logName, true, 'provider-and-keys-reset').catch(() => {});
    return saved;
  }

  async function saveTranslationApiKey(nextKey = translationApiKeyDraft) {
    const provider = getTranslationProvider(settings.translation_api_provider, settings.translation_api_url);
    if (!provider.supportsApiKey) return settings;
    const normalizedKey = String(nextKey || '').trim();
    const storedKeys = { ...(settings.translation_api_keys || {}), [provider.id]: normalizedKey };
    if (normalizedKey === String(settings.translation_api_key || '')
      && normalizedKey === String(settings.translation_api_keys?.[provider.id] || '')) return settings;
    // 密钥保存只记录“是否存在”而不记录内容，是为了保留排错能力，同时避免把用户私密凭据写进日志。
    // Key saving logs only whether a key exists, not its content, preserving diagnostics without writing private credentials to logs.
    const saved = await update({ translation_api_key: normalizedKey, translation_api_keys: storedKeys });
    await api.logLanguagePackEvent('translation_api_key_saved', '', provider.logName, true, normalizedKey ? 'key-present' : 'key-empty').catch(() => {});
    return saved;
  }

  async function clearTranslationApiKey() {
    const provider = getTranslationProvider(settings.translation_api_provider, settings.translation_api_url);
    setTranslationApiKeyDraft('');
    const storedKeys = { ...(settings.translation_api_keys || {}), [provider.id]: '' };
    const saved = await update({ translation_api_key: '', translation_api_keys: storedKeys });
    await api.logLanguagePackEvent('translation_api_key_cleared', '', provider.logName, true, 'settings-ui').catch(() => {});
    return saved;
  }

  const updateShortcuts = (key, value) => update({ shortcuts: { ...defaultShortcuts, ...(settings.shortcuts || {}), [key]: value } });
  const isMac = /Mac|iPhone|iPad|iPod/i.test(window.navigator?.platform || '');

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

  function showSettingsConfirm(title, message, onConfirm, danger = false, labels = {}) {
    // 数据管理确认统一使用软件内弹窗，是为了避免原生 Windows 提示框破坏自绘界面的视觉一致性。
    // Data-management confirmations use an in-app dialog so native Windows alerts do not break the custom-drawn UI language.
    setSettingsDialog({ kind: 'confirm', title, message, onConfirm, danger, ...labels });
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

  async function refreshLanguagePacks() {
    await api.logLanguagePackEvent('scan_requested', '', 'local-pack-store', true, 'settings-ui').catch(() => {});
    const packs = await api.listLanguagePacks(referenceLanguageMessages).catch((error) => {
      api.logLanguagePackEvent('scan_failed', '', 'local-pack-store', false, String(error?.message || error)).catch(() => {});
      return [];
    });
    const normalized = Array.isArray(packs) ? packs : [];
    const updateCount = normalized.filter((pack) => pack.integrity === 'update_available').length;
    const errorCount = normalized.filter((pack) => ['corrupt', 'incomplete'].includes(pack.integrity)).length;
    await api.logLanguagePackEvent('scan_finished', '', 'local-pack-store', true, `${normalized.length} pack(s), ${updateCount} update(s), ${errorCount} error(s)`).catch(() => {});
    onLanguagePacksChange(normalized);
    return normalized;
  }

  function preservePlaceholders(text) {
    const placeholders = [];
    const safe = String(text || '').replace(/\{[^}]+\}/g, (match) => {
      const token = `CLIPANCHOR_PLACEHOLDER_${placeholders.length}`;
      placeholders.push([token, match]);
      return token;
    });
    return { safe, placeholders };
  }

  function restorePlaceholders(text, placeholders) {
    return placeholders.reduce((value, [token, original]) => value.replaceAll(token, original), String(text || ''));
  }

  async function translateUiString(text, targetCode, providerId = settings.translation_api_provider, apiKey = translationApiKeyDraft) {
    if (!String(text || '').trim()) return text || '';
    const { safe, placeholders } = preservePlaceholders(text);
    const provider = getTranslationProvider(providerId, settings.translation_api_url);
    const apiTargetCode = mapTranslationTargetCode(targetCode, provider.id);
    // 翻译请求统一交给 Tauri 后端，是为了绕开 WebView 的 CORS/fetch 限制，并让每个 Provider 的请求格式与返回字段集中适配。
    // Translation requests are routed through the Tauri backend to avoid WebView CORS/fetch limits and keep provider-specific request/response adapters centralized.
    const translated = await api.translateText(provider.id, apiTargetCode, safe, apiKey);
    return restorePlaceholders(translated || safe, placeholders);
  }

  async function runLanguagePackGeneration(
    requestedCode,
    { activateAfterSave = false, regenerated = false, existingPack = null } = {}
  ) {
    const rawCode = String(requestedCode || '').trim();
    const targetCode = normalizeLocaleCode(rawCode || detectSystemLanguageCode());
    if (!/^[a-z]{2,3}(?:-[A-Za-z0-9]{2,8}){0,2}$/.test(targetCode)) {
      await api.logLanguagePackEvent('generate_rejected', targetCode, providerNameFromId(settings.translation_api_provider, settings.translation_api_url), false, 'invalid-code').catch(() => {});
      setLanguageGenerationState({ busy: false, message: t('languageCodeInvalid'), error: true, current: 0, total: 0, percent: 0 });
      return false;
    }
    if (isBuiltInLanguageCode(targetCode)) {
      await api.logLanguagePackEvent('generate_rejected', targetCode, providerNameFromId(settings.translation_api_provider, settings.translation_api_url), false, 'built-in-language').catch(() => {});
      setLanguageGenerationState({ busy: false, message: t('languageBuiltInNotice'), error: true, current: 0, total: 0, percent: 0 });
      return false;
    }

    const effectiveProvider = getTranslationProvider(settings.translation_api_provider, settings.translation_api_url);
    const activeApiKey = effectiveProvider.supportsApiKey ? String(translationApiKeyDraft || '').trim() : '';
    if (effectiveProvider.supportsApiKey && activeApiKey !== String(settings.translation_api_key || '')) {
      await saveTranslationApiKey(activeApiKey);
    }

    const currentPack = existingPack
      || languagePacks.find((pack) => normalizeLocaleCode(pack?.code || '') === targetCode)
      || null;
    const label = currentPack?.native_name || currentPack?.nativeName || currentPack?.label || inferLanguageLabel(targetCode);
    const reference = referenceLanguageMessages;
    const entries = Object.entries(reference);
    const previousMessages = currentPack?.messages && typeof currentPack.messages === 'object' ? currentPack.messages : {};
    const previousStatus = currentPack?.message_status || currentPack?.messageStatus || {};
    const backendOutdated = new Set(currentPack?.outdated_keys || currentPack?.outdatedKeys || []);
    const translated = {};
    const messageStatus = {};
    const pendingEntries = [];
    let reusedCount = 0;

    for (const [key, sourceText] of entries) {
      const hasPrevious = Object.prototype.hasOwnProperty.call(previousMessages, key);
      const previousText = hasPrevious ? String(previousMessages[key] ?? '') : '';
      const status = previousStatus[key] || {};
      const sourceHash = languageSourceFingerprint(sourceText);
      const translationHash = languageSourceFingerprint(previousText);
      const storedTranslationHash = String(status.translation_hash || status.translationHash || '');
      const manuallyEdited = Boolean(status.modified)
        || Boolean(storedTranslationHash && storedTranslationHash !== translationHash);
      const sourceChanged = Boolean(status.source_hash || status.sourceHash)
        && String(status.source_hash || status.sourceHash) !== sourceHash;
      const needsTranslation = !hasPrevious || ((sourceChanged || backendOutdated.has(key)) && !manuallyEdited);

      if (needsTranslation) {
        pendingEntries.push([key, sourceText]);
        continue;
      }

      // 中文：未变化的译文直接复用，人工修改项也优先保留，是为了把 API 调用严格限制在新增或确实需要适配的界面文本上。
      // English: Unchanged translations are reused and manually edited entries are preserved, limiting API calls strictly to new or genuinely outdated UI copy.
      translated[key] = previousText;
      messageStatus[key] = {
        source_hash: sourceHash,
        translation_hash: translationHash,
        modified: manuallyEdited
      };
      reusedCount += 1;
    }

    const providerName = effectiveProvider.logName;
    const total = pendingEntries.length;
    setLanguageGenerationState({
      busy: true,
      message: total
        ? t('languageProgressLabel').replace('{current}', '0').replace('{total}', String(total))
        : t('languageCleaningObsolete'),
      error: false,
      current: 0,
      total,
      percent: total ? 0 : 100
    });

    try {
      await api.logLanguagePackEvent(
        regenerated ? 'regenerate_started' : 'generate_started',
        targetCode,
        providerName,
        true,
        `${total} translation request(s), ${reusedCount} reused`
      ).catch(() => {});

      if (total) {
        await api.logLanguagePackEvent('translation_api_started', mapTranslationTargetCode(targetCode, effectiveProvider.id), providerName, true, `source en, pack ${targetCode}`).catch(() => {});
      }

      let lastLoggedProgress = 0;
      // 中文：只顺序翻译待更新项，是为了兼容免费接口限流，同时避免软件更新后重复请求整份语言包。
      // English: Only pending entries are translated sequentially to respect free-service rate limits and avoid retranslating an entire pack after application updates.
      for (let index = 0; index < pendingEntries.length; index += 1) {
        const [key, value] = pendingEntries[index];
        const translatedValue = await translateUiString(value, targetCode, effectiveProvider.id, activeApiKey);
        translated[key] = translatedValue;
        messageStatus[key] = {
          source_hash: languageSourceFingerprint(value),
          translation_hash: languageSourceFingerprint(translatedValue),
          modified: false
        };
        const current = index + 1;
        const percent = Math.round((current / total) * 100);
        setLanguageGenerationState({
          busy: true,
          message: t('languageProgressLabel').replace('{current}', String(current)).replace('{total}', String(total)),
          error: false,
          current,
          total,
          percent
        });
        if (percent >= lastLoggedProgress + 25 || current === total) {
          lastLoggedProgress = percent;
          await api.logLanguagePackEvent('translation_progress', mapTranslationTargetCode(targetCode, effectiveProvider.id), providerName, true, `${percent}% (${current}/${total})`).catch(() => {});
        }
      }

      if (total) {
        await api.logLanguagePackEvent('translation_api_finished', mapTranslationTargetCode(targetCode, effectiveProvider.id), providerName, true, `${total} message(s)`).catch(() => {});
      }

      const saved = await api.saveLanguagePack({
        code: targetCode,
        label: currentPack?.label || label,
        native_name: currentPack?.native_name || currentPack?.nativeName || label,
        source: `${providerName} (${mapTranslationTargetCode(targetCode, effectiveProvider.id)})`,
        generated_at: new Date().toISOString(),
        format: 'clipanchor-language-pack',
        source_locale: 'en',
        messages: translated,
        message_status: messageStatus
      });
      const packs = await refreshLanguagePacks();
      const nextLocale = saved?.code || targetCode;
      if (activateAfterSave) {
        await chooseLocale(nextLocale);
      }
      await api.logLanguagePackEvent(
        regenerated ? 'regenerate_finished' : 'generate_finished',
        nextLocale,
        providerName,
        true,
        `${total} translated, ${reusedCount} reused`
      ).catch(() => {});
      setLanguageCodeDraft('');
      const finalMessage = total === 0
        ? t('languageNoUpdates').replace('{language}', label)
        : t(regenerated ? 'languageIncrementalUpdateDone' : 'languageGenerateDone')
          .replace('{language}', label)
          .replace('{translated}', String(total))
          .replace('{reused}', String(reusedCount));
      setLanguageGenerationState({
        busy: false,
        message: finalMessage,
        error: false,
        current: total,
        total,
        percent: 100
      });
      onLanguagePacksChange(packs);
      return true;
    } catch (error) {
      const rawError = String(error?.message || error);
      const userMessage = rawError === 'TRANSLATION_RATE_LIMITED'
        ? t('languageGenerateRateLimited')
        : t('languageGenerateFailed').replace('{error}', rawError);
      await api.logLanguagePackEvent(regenerated ? 'regenerate_failed' : 'generate_failed', targetCode, effectiveProvider.logName, false, rawError === 'TRANSLATION_RATE_LIMITED' ? '429 rate-limited' : rawError).catch(() => {});
      setLanguageGenerationState({ busy: false, message: userMessage, error: true, current: 0, total: 0, percent: 0 });
      return false;
    }
  }

  async function generateLanguagePack() {
    await runLanguagePackGeneration(languageCodeDraft, { activateAfterSave: true });
  }

  async function regenerateLanguagePack(language) {
    const targetCode = normalizeLocaleCode(language?.code || '');
    if (!targetCode || languageGenerationState.busy) return;
    const existingPack = languagePacks.find((pack) => normalizeLocaleCode(pack?.code || '') === targetCode) || language;
    await runLanguagePackGeneration(targetCode, {
      activateAfterSave: settings.locale === targetCode,
      regenerated: true,
      existingPack
    });
  }

  async function chooseExtraLanguage(language) {
    const integrity = language?.integrity || 'complete';
    if (integrity === 'complete') {
      await chooseLocale(language.code);
      return;
    }

    const label = language?.nativeName || language?.label || language?.code;
    if (integrity === 'update_available') {
      // 中文：可更新语言包仍允许立即使用，缺失文本由内置语言回退；随后提示增量更新，避免把普通更新误报成文件损坏。
      // English: An updatable pack remains usable immediately with built-in fallback for missing text; an incremental-update prompt then keeps normal updates distinct from file corruption.
      await chooseLocale(language.code);
      const missing = language?.missingKeys?.length || 0;
      const changed = language?.outdatedKeys?.length || 0;
      const removed = language?.removedKeys?.length || 0;
      const message = t('languageUpdateWarning')
        .replace('{language}', label)
        .replace('{missing}', String(missing))
        .replace('{changed}', String(changed))
        .replace('{removed}', String(removed));
      showSettingsConfirm(
        t('languageUpdateTitle'),
        message,
        () => regenerateLanguagePack(language),
        false,
        { confirmLabel: t('languageIncrementalUpdateAction'), cancelLabel: t('languageLaterAction'), icon: 'update' }
      );
      return;
    }

    const message = integrity === 'corrupt'
      ? t('languageIntegrityCorrupt').replace('{language}', label)
      : t('languageIntegrityIncomplete')
        .replace('{language}', label)
        .replace('{count}', String(language?.missingKeys?.length || 0));
    showSettingsConfirm(
      t('languageIntegrityTitle'),
      message,
      () => regenerateLanguagePack(language),
      false,
      { confirmLabel: t('languageRegenerateAction'), cancelLabel: t('languageLaterAction'), icon: 'warning' }
    );
  }


  async function deleteLanguagePack(language) {
    const targetCode = normalizeLocaleCode(language?.code || '');
    if (!targetCode) return;
    showSettingsConfirm(
      t('languageDeleteTitle'),
      t('languageDeleteConfirm').replace('{language}', language?.nativeName || language?.label || targetCode),
      async () => {
        try {
          await api.logLanguagePackEvent('delete_requested', targetCode, 'local-pack-store', true, 'settings-ui').catch(() => {});
          if (settings.locale === targetCode) {
            // 当前语言被删除前先切回自动，是为了避免设置继续指向一个已经不存在的本地语言文件。
            // The active locale is switched back to Auto before deletion so settings never point at a local pack that no longer exists.
            await chooseLocale('auto');
          }
          const removed = await api.deleteLanguagePack(targetCode);
          await api.logLanguagePackEvent('delete_finished', targetCode, 'local-pack-store', Boolean(removed), removed ? 'file removed' : 'file missing').catch(() => {});
          const packs = await refreshLanguagePacks();
          onLanguagePacksChange(packs);
          setLanguageGenerationState({ busy: false, message: t('languageDeleteDone').replace('{language}', language?.nativeName || language?.label || targetCode), error: false, current: 0, total: 0, percent: 0 });
        } catch (error) {
          const detail = String(error?.message || error);
          await api.logLanguagePackEvent('delete_failed', targetCode, 'local-pack-store', false, detail).catch(() => {});
          setLanguageGenerationState({ busy: false, message: t('languageDeleteFailed').replace('{error}', detail), error: true, current: 0, total: 0, percent: 0 });
        }
      },
      true
    );
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
    <section ref={settingsScrollRef} className="settings-scroll scroll-area">
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
            <div className="appearance-basic-grid">
              <div className="control-row language-control-row appearance-language-card"><SettingName help={t('helpLanguage')}>{t('language')}</SettingName><Segmented className="language-segmented" value={['auto', 'en', 'zh'].includes(settings.locale) ? settings.locale : ''} onChange={chooseLocale} options={coreLanguageOptions} /></div>
              <label className="control-row appearance-theme-card"><SettingName help={t('helpTheme')}>{t('theme')}</SettingName><Segmented value={settings.theme} onChange={(v) => update({ theme: v })} options={[{ value: 'system', label: t('system') }, { value: 'dark', label: t('dark') }, { value: 'light', label: t('light') }]} /></label>
              <label className="control-row appearance-animation-card"><SettingName help={t('helpAnimation')}>{t('animation')}</SettingName><Segmented value={settings.animation_mode} onChange={(v) => update({ animation_mode: v })} options={[{ value: 'elegant', label: t('elegant') }, { value: 'performance', label: t('performance') }]} /></label>
            </div>
            <div className="language-extension-panel">
              <div className="language-pack-heading">
                <span>{t('languagePackOther')}</span>
                <small>{t('translationApiNotice')}</small>
              </div>
              <p className="language-pack-warning">{t('languagePackUnofficialUserNotice')}</p>
              {extraLanguageOptions.length ? (
                <div ref={languagePackGridRef} className="language-pack-grid scroll-area">
                  {/* 扩展语言卡片把名称、代号和切换状态拆成独立层级，是为了避免操作按钮挤压主要信息。 */}
                  {/* Extra-language cards separate the name, code, and switch state so action buttons cannot compress the primary information. */}
                  {extraLanguageOptions.map((language) => {
                    const active = settings.locale === language.code;
                    const displayName = language.nativeName || language.label || language.code;
                    const needsUpdate = language.integrity === 'update_available';
                    const damaged = ['corrupt', 'incomplete'].includes(language.integrity);
                    return (
                      <div key={language.code} className={`language-pack-option ${active ? 'active' : ''} ${needsUpdate ? 'has-update' : ''} ${damaged ? 'has-error' : ''}`}>
                        <button type="button" className="language-pack-select" aria-pressed={active} title={displayName} onClick={() => chooseExtraLanguage(language)}>
                          <span className="language-pack-check" aria-hidden="true" />
                          <span className="language-pack-main">
                            <span className="language-pack-title-row">
                              <strong>{displayName}</strong>
                              <small className="language-pack-code">
                                {language.code}
                                {needsUpdate ? <RefreshCw className="language-pack-update-icon" size={12} title={t('languagePackNeedsUpdate')} aria-label={t('languagePackNeedsUpdate')} /> : null}
                                {damaged ? <TriangleAlert className="language-pack-error-icon" size={12} title={t('languagePackDamaged')} aria-label={t('languagePackDamaged')} /> : null}
                              </small>
                            </span>
                            <span className="language-pack-state">{active
                              ? (needsUpdate ? `${t('languagePackActive')} · ${t('languagePackNeedsUpdate')}` : t('languagePackActive'))
                              : (damaged ? t('languagePackDamaged') : (needsUpdate ? t('languagePackNeedsUpdate') : t('languagePackClickToUse')))}</span>
                          </span>
                        </button>
                        <span className="language-pack-actions">
                          <button type="button" className="language-pack-refresh" disabled={languageGenerationState.busy} title={needsUpdate ? t('languageIncrementalUpdateAction') : t('languageRefreshAction')} aria-label={needsUpdate ? t('languageIncrementalUpdateAction') : t('languageRefreshAction')} onClick={() => regenerateLanguagePack(language)}>
                            <RefreshCw size={14} />
                          </button>
                          <button type="button" className="language-pack-delete" disabled={languageGenerationState.busy} title={t('languageDeleteAction')} aria-label={t('languageDeleteAction')} onClick={() => deleteLanguagePack(language)}>
                            <Trash2 size={14} />
                          </button>
                        </span>
                      </div>
                    );
                  })}
                </div>
              ) : (
                <div className="language-pack-empty" aria-disabled="true">{t('languagePackNone')}</div>
              )}
              <div className="language-generator-box">
                <div>
                  <strong>{t('languageGeneratorTitle')}</strong>
                  <p>{t('languageGeneratorHint')}</p>
                </div>
                <div className="translation-api-settings">
                  <div className="translation-api-copy">
                    <strong>{t('translationApiSettingsTitle')}</strong>
                    <small>{t('translationApiSettingsHint')}</small>
                  </div>
                  <div className="translation-provider-actions">
                    <label className="translation-provider-select-wrap">
                      <span>{t('translationProviderField')}</span>
                      <DropdownSelect
                        value={normalizeTranslationProvider(settings.translation_api_provider, settings.translation_api_url)}
                        disabled={languageGenerationState.busy}
                        ariaLabel={t('translationProviderField')}
                        onChange={saveTranslationProvider}
                        options={[
                          { value: 'mymemory', label: t('translationProviderMyMemory') },
                          { value: 'uapis', label: t('translationProviderUapis') }
                        ]}
                      />
                    </label>
                    <label className="translation-api-key-wrap">
                      <span>{t('translationApiKeyField')}</span>
                      <input
                        type="password"
                        value={activeTranslationProvider.supportsApiKey ? translationApiKeyDraft : ''}
                        disabled={languageGenerationState.busy || !activeTranslationProvider.supportsApiKey}
                        placeholder={activeTranslationProvider.supportsApiKey ? t('translationApiKeyPlaceholder') : t('translationApiKeyUnavailable')}
                        autoComplete="off"
                        onChange={(event) => setTranslationApiKeyDraft(event.target.value)}
                        onBlur={() => saveTranslationApiKey()}
                        onKeyDown={(event) => { if (event.key === 'Enter') event.currentTarget.blur(); }}
                      />
                      <button className="soft-button clear-api-key-button" type="button" disabled={languageGenerationState.busy || !activeTranslationProvider.supportsApiKey || !translationApiKeyDraft} onClick={clearTranslationApiKey}>{t('translationApiKeyClear')}</button>
                    </label>
                  </div>
                </div>
                <div className="language-generator-actions">
                  <input value={languageCodeDraft} onChange={(event) => setLanguageCodeDraft(event.target.value)} placeholder={t('languageCodePlaceholder')} />
                  <button className="primary-button" type="button" disabled={languageGenerationState.busy} onClick={generateLanguagePack}>{languageGenerationState.busy ? t('generatingLanguage') : t('generateLanguage')}</button>
                  <button className="soft-button reset-api-button" type="button" disabled={languageGenerationState.busy} onClick={resetTranslationProvider}><RotateCcw size={13} />{t('translationApiReset')}</button>
                </div>
                {languageGenerationState.busy ? (
                  <div className={`language-progress ${languageGenerationState.total ? '' : 'without-track'}`} role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-valuenow={languageGenerationState.percent}>
                    <div className="language-progress-meta">
                      <span>{languageGenerationState.message}</span>
                      {languageGenerationState.total ? <b>{t('languageProgressPercent').replace('{percent}', String(languageGenerationState.percent))}</b> : null}
                    </div>
                    {languageGenerationState.total ? <span className="language-progress-track"><i style={{ width: `${languageGenerationState.percent}%` }} /></span> : null}
                  </div>
                ) : languageGenerationState.message ? (
                  <p className={`language-generation-result ${languageGenerationState.error ? 'error' : 'success'}`}>{languageGenerationState.message}</p>
                ) : null}
                <p className="language-generator-folder">{t('languagePackFolderHint').replace('{path}', `${boot.paths.data}/locales`)}</p>
              </div>
            </div>
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
            {isMac ? (
              <label className="builtin-shortcut-row">
                <SettingName help={t('helpShortcutCommandW')}>{t('shortcutCommandW')}</SettingName>
                <input readOnly value="Command+W" />
              </label>
            ) : null}
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
