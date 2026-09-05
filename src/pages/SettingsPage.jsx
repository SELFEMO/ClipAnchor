import { useEffect, useMemo, useState } from 'react';
import { BadgeCheck, Keyboard, MapPinned, Power, RefreshCw, TriangleAlert } from 'lucide-react';
import { api } from '../api.js';
import { detectSystemLanguageCode, getReferenceMessages, inferLanguageLabel, listLanguageChoices, normalizeLocaleCode } from '../i18n.js';
import { shouldFlushLanguageProgress, yieldLanguagePackFrame } from '../languagePackProgress.js';
import { formatShortcutForDisplay, normalizeShortcutForStorage } from '../shortcutDisplay.js';
import { AppearanceSection } from './settings/AppearanceSection.jsx';
import { DataSection } from './settings/DataSection.jsx';
import { PositionMap } from './settings/PositionMap.jsx';
import { PrivacySection } from './settings/PrivacySection.jsx';
import { captureShortcut, HelpTip, SettingName, SettingsSoftDialog, Switch } from './settings/widgets.jsx';

const shortcutLabels = {
  toggle_pin_service: 'shortcutPinService',
  toggle_history_service: 'shortcutHistoryService',
  toggle_main_window: 'shortcutMainWindow',
  enter_light_mode: 'shortcutLiteMode',
  toggle_theme_mode: 'shortcutThemeMode',
  toggle_clipboard_pause: 'shortcutClipboardPause'
};

const defaultShortcuts = {
  toggle_pin_service: 'Ctrl+Shift+P',
  toggle_history_service: 'Ctrl+Shift+H',
  toggle_main_window: 'Ctrl+Shift+X',
  enter_light_mode: 'Ctrl+Shift+L',
  toggle_theme_mode: 'Ctrl+Shift+T',
  toggle_clipboard_pause: 'Ctrl+Shift+S'
};

const shortcutOrder = [
  'toggle_pin_service',
  'toggle_history_service',
  'toggle_main_window',
  'enter_light_mode',
  'toggle_theme_mode',
  'toggle_clipboard_pause'
];

function shortcutConflictMessage(conflict, t) {
  if (!conflict) return '';
  if (conflict.kind === 'duplicate') return t('shortcutConflictDuplicate');
  if (conflict.kind === 'invalid') return t('shortcutConflictInvalid');
  if (conflict.kind === 'system') {
    return t('shortcutConflictSystem').replace('{source}', conflict.source || t('shortcutConflictUnknownSource'));
  }
  return '';
}

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
    clipboard_paused: Boolean(value?.clipboard_paused),
    privacy_filter_mode: value?.privacy_filter_mode === 'off' || value?.privacy_filter_mode === 'light'
      ? value.privacy_filter_mode
      : (value?.privacy_mode ? 'light' : 'off'),
    privacy_mode: (value?.privacy_filter_mode === 'off' || value?.privacy_filter_mode === 'light' ? value.privacy_filter_mode : (value?.privacy_mode ? 'light' : 'off')) !== 'off',
    filter_text: value?.filter_text !== false,
    filter_image: value?.filter_image !== false,
    filter_file: value?.filter_file !== false,
    translation_api_provider: provider,
    translation_api_url: getTranslationProvider(provider).endpoint,
    translation_api_key: activeKey,
    translation_api_keys: storedKeys,
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

function languageTextHash(value) {
  let hash = 0x811c9dc5;
  const bytes = new TextEncoder().encode(String(value ?? ''));
  for (const byte of bytes) {
    hash ^= byte;
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash.toString(16).padStart(8, '0');
}

function readMessageStatus(status, key) {
  const value = status?.[key] || {};
  return {
    sourceHash: String(value.source_hash || value.sourceHash || ''),
    translationHash: String(value.translation_hash || value.translationHash || ''),
    modified: Boolean(value.modified)
  };
}

export default function SettingsPage({ t, boot, onBootChange, updateStatus, onCheckUpdate, languagePacks = [], onLanguagePacksChange = () => {} }) {
  const [settings, setSettings] = useState(() => normalizeSettings(boot.settings));
  const [dataUsage, setDataUsage] = useState(null);
  const [logStatus, setLogStatus] = useState(null);
  const [cleanupDays, setCleanupDays] = useState(30);
  const [cleanupPreservePinned, setCleanupPreservePinned] = useState(true);
  const [settingsDialog, setSettingsDialog] = useState(null);
  const [languageCodeDraft, setLanguageCodeDraft] = useState('');
  const [translationApiKeyDraft, setTranslationApiKeyDraft] = useState(() => String(settings.translation_api_key || ''));
  const [languageGenerationState, setLanguageGenerationState] = useState({ busy: false, message: '', error: false, current: 0, total: 0, percent: 0 });
  const [languageReloadState, setLanguageReloadState] = useState({ busy: false, message: '', error: false });
  const [shortcutConflictResults, setShortcutConflictResults] = useState([]);

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

  const globalShortcutsSupported = boot.capabilities?.global_shortcuts_supported !== false;

  const duplicateConflicts = useMemo(() => {
    if (!globalShortcutsSupported) return new Set();
    const values = Object.values(settings.shortcuts || {}).map(normalizeShortcutForStorage);
    return new Set(values.filter((value, index) => values.indexOf(value) !== index));
  }, [globalShortcutsSupported, settings.shortcuts]);

  useEffect(() => {
    let disposed = false;
    setShortcutConflictResults([]);
    if (!globalShortcutsSupported) return undefined;
    const timer = window.setTimeout(async () => {
      try {
        const results = await api.checkShortcutConflicts({ ...defaultShortcuts, ...(settings.shortcuts || {}) });
        if (!disposed) setShortcutConflictResults(Array.isArray(results) ? results : []);
      } catch (error) {
        // 冲突检测失败不应阻断设置编辑；后端会记录平台诊断，界面只保留可即时判断的应用内重复提示。
        // Conflict probing must not block editing; the backend records platform diagnostics while the UI keeps the immediately knowable in-app duplicate warning.
        if (!disposed) setShortcutConflictResults([]);
      }
    }, 240);
    return () => {
      disposed = true;
      window.clearTimeout(timer);
    };
  }, [globalShortcutsSupported, settings.shortcuts]);

  const shortcutWarnings = useMemo(() => {
    const warnings = new Map();
    if (!globalShortcutsSupported) return warnings;
    for (const key of shortcutOrder) {
      const value = settings.shortcuts?.[key] || defaultShortcuts[key];
      const normalized = normalizeShortcutForStorage(value);
      if (duplicateConflicts.has(normalized)) {
        warnings.set(key, { kind: 'duplicate', source: 'ClipAnchor' });
      }
    }
    for (const conflict of shortcutConflictResults) {
      if (!conflict?.shortcut_key || warnings.has(conflict.shortcut_key)) continue;
      warnings.set(conflict.shortcut_key, conflict);
    }
    return warnings;
  }, [duplicateConflicts, globalShortcutsSupported, settings.shortcuts, shortcutConflictResults]);

  const languageChoices = useMemo(() => listLanguageChoices(languagePacks), [languagePacks]);
  const coreLanguageOptions = useMemo(() => ([
    { value: 'auto', label: t('autoLanguage') },
    { value: 'en', label: 'English' },
    { value: 'zh', label: '简体中文' }
  ]), [t]);
  const extraLanguageOptions = useMemo(() => languageChoices.filter((item) => !['en', 'zh'].includes(item.code)), [languageChoices]);
  const activeTranslationProvider = getTranslationProvider(settings.translation_api_provider, settings.translation_api_url);
  const referenceLanguageMessages = useMemo(() => getReferenceMessages('en'), []);
  const languagePackFolderPath = boot.paths.locales || `${boot.paths.data}/locales`;
  const popupPositionSupported = boot.capabilities?.popup_position_supported !== false;

  useEffect(() => {
    let disposed = false;
    let refreshTimer = 0;

    async function rescanLocalLanguagePacks() {
      try {
        const packs = await api.listLanguagePacks(referenceLanguageMessages);
        if (!disposed) onLanguagePacksChange(Array.isArray(packs) ? packs : []);
      } catch (error) {
        // The normal settings scan writes detailed diagnostics; focus refresh stays silent.
        console.error('ClipAnchor language-pack focus refresh failed:', error);
      }
    }

    function scheduleRescan() {
      window.clearTimeout(refreshTimer);
      refreshTimer = window.setTimeout(rescanLocalLanguagePacks, 120);
    }

    // Scan once when the settings page mounts, then rescan after the user returns from
    // Finder/Explorer so manually copied JSON files appear without another app restart.
    scheduleRescan();
    window.addEventListener('focus', scheduleRescan);
    return () => {
      disposed = true;
      window.clearTimeout(refreshTimer);
      window.removeEventListener('focus', scheduleRescan);
    };
  }, [boot.paths.locales, referenceLanguageMessages, onLanguagePacksChange]);

  async function persist(next) {
    const previous = normalizeSettings(settings);
    const normalized = normalizeSettings(next);
    // 语言与主题先同步到父级状态，可让 React 立即重建翻译器和主题类；若后端拒绝保存，再统一回滚，避免 Linux 上出现“按钮已选中但界面不变化”。
    // Locale and theme are applied to parent state immediately so React can rebuild the translator and theme class; a backend rejection rolls everything back instead of leaving Linux controls selected without visual change.
    setSettings(normalized);
    onBootChange({ ...boot, settings: normalized });
    try {
      const saved = await api.saveSettings(normalized);
      const normalizedSaved = normalizeSettings(saved);
      setSettings(normalizedSaved);
      onBootChange({ ...boot, settings: normalizedSaved });
      return normalizedSaved;
    } catch (error) {
      setSettings(previous);
      onBootChange({ ...boot, settings: previous });
      console.error('ClipAnchor settings save failed:', error);
      throw error;
    }
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
    try {
      const saved = await update({ locale: normalized });
      await api.logLanguagePackEvent('activate_saved', saved.locale, provider, true, 'settings-ui').catch(() => {});
      return saved;
    } catch (error) {
      await api.logLanguagePackEvent('activate_failed', normalized, provider, false, String(error)).catch(() => {});
      showSettingsAlert(t('language'), String(error));
      return null;
    }
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

  function applyPastedTranslationApiKey(value) {
    const nextKey = String(value || '').replace(/[\r\n]+$/g, '').trim();
    setTranslationApiKeyDraft(nextKey);
    return nextKey;
  }

  async function pasteTranslationApiKey() {
    if (languageGenerationState.busy || !activeTranslationProvider.supportsApiKey) return;
    try {
      const nextKey = applyPastedTranslationApiKey(await api.readClipboardTextForInput());
      await saveTranslationApiKey(nextKey);
    } catch (error) {
      showSettingsAlert(t('translationApiSettingsTitle'), t('translationApiKeyPasteFailed').replace('{error}', String(error)));
    }
  }

  async function openLanguagePackFolder() {
    try {
      await api.openLanguagePackFolder();
    } catch (error) {
      showSettingsAlert(t('languagePackOther'), String(error));
    }
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

  async function refreshLanguagePacks({ throwOnError = false } = {}) {
    await api.logLanguagePackEvent('scan_requested', '', 'local-pack-store', true, 'settings-ui').catch(() => {});
    try {
      const packs = await api.listLanguagePacks(referenceLanguageMessages);
      const normalized = Array.isArray(packs) ? packs : [];
      const warningCount = normalized.filter((pack) => pack.integrity && pack.integrity !== 'complete').length;
      await api.logLanguagePackEvent('scan_finished', '', 'local-pack-store', true, `${normalized.length} pack(s), ${warningCount} warning(s)`).catch(() => {});
      onLanguagePacksChange(normalized);
      return normalized;
    } catch (error) {
      const detail = String(error?.message || error);
      await api.logLanguagePackEvent('scan_failed', '', 'local-pack-store', false, detail).catch(() => {});
      if (throwOnError) throw error;
      // 自动刷新失败时保留已经加载的选项，是为了避免一次临时文件锁或目录读取错误把整个扩展语言列表清空。
      // Automatic refresh keeps the already loaded options after a transient file-lock or directory-read failure instead of clearing the entire extension language list.
      return languagePacks;
    }
  }

  async function reloadLanguagePacks() {
    if (languageReloadState.busy || languageGenerationState.busy) return;
    setLanguageReloadState({ busy: true, message: t('reloadingLanguagePacks'), error: false });
    try {
      // 手动刷新直接重新扫描后端活动目录，是为了让用户复制 JSON 后无需重启即可看到并选择新的扩展语言。
      // Manual reload rescans the backend's active directory so a newly copied JSON file becomes selectable without restarting the application.
      const packs = await refreshLanguagePacks({ throwOnError: true });
      setLanguageReloadState({
        busy: false,
        message: t('reloadLanguagePacksDone').replace('{count}', String(packs.length)),
        error: false
      });
    } catch (error) {
      const detail = String(error?.message || error);
      setLanguageReloadState({
        busy: false,
        message: t('reloadLanguagePacksFailed').replace('{error}', detail),
        error: true
      });
    }
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

  async function runLanguagePackGeneration(requestedCode, { activateAfterSave = false, regenerated = false, existingPack = null } = {}) {
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

    const reference = getReferenceMessages('en');
    const previousMessages = existingPack?.messages && typeof existingPack.messages === 'object' ? existingPack.messages : {};
    const previousStatus = existingPack?.messageStatus && typeof existingPack.messageStatus === 'object' ? existingPack.messageStatus : {};
    const translated = {};
    const nextMessageStatus = {};
    const translateEntries = [];
    let reused = 0;
    let manuallyProtected = 0;

    for (const [key, sourceText] of Object.entries(reference)) {
      const sourceHash = languageTextHash(sourceText);
      const hasTranslation = Object.prototype.hasOwnProperty.call(previousMessages, key);
      if (!hasTranslation) {
        translateEntries.push([key, sourceText, sourceHash]);
        continue;
      }

      const translation = String(previousMessages[key] ?? '');
      const translationHash = languageTextHash(translation);
      const oldStatus = readMessageStatus(previousStatus, key);
      const manuallyModified = oldStatus.modified || Boolean(oldStatus.translationHash && oldStatus.translationHash !== translationHash);
      const sourceChanged = Boolean(oldStatus.sourceHash && oldStatus.sourceHash !== sourceHash);

      if (sourceChanged && !manuallyModified) {
        translateEntries.push([key, sourceText, sourceHash]);
        continue;
      }

      translated[key] = translation;
      nextMessageStatus[key] = {
        // Keep the previous source hash for a protected manual translation when the English
        // source changed. The scanner will continue to request human review without overwriting it.
        source_hash: sourceChanged && manuallyModified ? oldStatus.sourceHash : sourceHash,
        translation_hash: translationHash,
        modified: manuallyModified
      };
      reused += 1;
      if (sourceChanged && manuallyModified) manuallyProtected += 1;
    }

    const removed = Object.keys(previousMessages).filter((key) => !Object.prototype.hasOwnProperty.call(reference, key)).length;
    const totalToTranslate = translateEntries.length;
    const label = existingPack?.label || inferLanguageLabel(targetCode);
    const nativeName = existingPack?.nativeName || existingPack?.native_name || label;

    setLanguageGenerationState({
      busy: true,
      message: totalToTranslate
        ? t('languageProgressLabel').replace('{current}', '0').replace('{total}', String(totalToTranslate))
        : t('languageNoUpdates').replace('{language}', label),
      error: false,
      current: 0,
      total: totalToTranslate,
      percent: totalToTranslate ? 0 : 100
    });

    try {
      const providerName = effectiveProvider.logName;
      await api.logLanguagePackEvent(regenerated ? 'incremental_update_started' : 'generate_started', targetCode, providerName, true, `${totalToTranslate} translate, ${reused} reuse, ${removed} remove`).catch(() => {});

      if (totalToTranslate) {
        await api.logLanguagePackEvent('translation_api_started', mapTranslationTargetCode(targetCode, effectiveProvider.id), providerName, true, `source en, pack ${targetCode}, ${totalToTranslate} item(s)`).catch(() => {});
        let lastLoggedProgress = 0;
        let lastProgressFlushAt = 0;
        for (let index = 0; index < translateEntries.length; index += 1) {
          const [key, value, sourceHash] = translateEntries[index];
          const valueTranslated = await translateUiString(value, targetCode, effectiveProvider.id, activeApiKey);
          translated[key] = valueTranslated;
          nextMessageStatus[key] = {
            source_hash: sourceHash,
            translation_hash: languageTextHash(valueTranslated),
            modified: false
          };
          const current = index + 1;
          const percent = Math.round((current / totalToTranslate) * 100);
          const now = Date.now();
          if (shouldFlushLanguageProgress(now, lastProgressFlushAt, current, totalToTranslate)) {
            lastProgressFlushAt = now;
            setLanguageGenerationState({
              busy: true,
              message: t('languageProgressLabel').replace('{current}', String(current)).replace('{total}', String(totalToTranslate)),
              error: false,
              current,
              total: totalToTranslate,
              percent
            });
          }
          if (percent >= lastLoggedProgress + 25 || current === totalToTranslate) {
            lastLoggedProgress = percent;
            await api.logLanguagePackEvent('translation_progress', mapTranslationTargetCode(targetCode, effectiveProvider.id), providerName, true, `${percent}% (${current}/${totalToTranslate})`).catch(() => {});
          }
          await yieldLanguagePackFrame();
        }
        await api.logLanguagePackEvent('translation_api_finished', mapTranslationTargetCode(targetCode, effectiveProvider.id), providerName, true, `${totalToTranslate} item(s)`).catch(() => {});
      }

      const saved = await api.saveLanguagePack({
        code: targetCode,
        label,
        native_name: nativeName,
        source: existingPack?.source || `${providerName} (${mapTranslationTargetCode(targetCode, effectiveProvider.id)})`,
        generated_at: new Date().toISOString(),
        format: existingPack?.format || 'clipanchor-language-pack',
        source_locale: existingPack?.sourceLocale || existingPack?.source_locale || 'en',
        messages: translated,
        message_status: nextMessageStatus
      });
      const packs = await refreshLanguagePacks();
      const nextLocale = saved?.code || targetCode;
      if (activateAfterSave) await chooseLocale(nextLocale);

      await api.logLanguagePackEvent(regenerated ? 'incremental_update_finished' : 'generate_finished', nextLocale, providerName, true, `${totalToTranslate} translated, ${reused} reused, ${removed} removed, ${manuallyProtected} manual-review`).catch(() => {});
      setLanguageCodeDraft('');
      setLanguageGenerationState({
        busy: false,
        message: regenerated
          ? t(totalToTranslate || removed ? 'languageIncrementalUpdateDone' : 'languageNoUpdates')
            .replace('{language}', label)
            .replace('{translated}', String(totalToTranslate))
            .replace('{reused}', String(reused))
            .replace('{removed}', String(removed))
          : t('languageGenerateDone').replace('{language}', label),
        error: false,
        current: totalToTranslate,
        total: totalToTranslate,
        percent: 100
      });
      onLanguagePacksChange(packs);
      return true;
    } catch (error) {
      const rawError = String(error?.message || error);
      const userMessage = rawError === 'TRANSLATION_RATE_LIMITED'
        ? t('languageGenerateRateLimited')
        : t('languageGenerateFailed').replace('{error}', rawError);
      await api.logLanguagePackEvent(regenerated ? 'incremental_update_failed' : 'generate_failed', targetCode, effectiveProvider.logName, false, rawError === 'TRANSLATION_RATE_LIMITED' ? '429 rate-limited' : rawError).catch(() => {});
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
    await runLanguagePackGeneration(targetCode, { activateAfterSave: settings.locale === targetCode, regenerated: true, existingPack: language });
  }

  async function chooseExtraLanguage(language) {
    const integrity = language?.integrity || 'complete';
    const label = language?.nativeName || language?.label || language?.code;

    if (integrity === 'corrupt') {
      showSettingsConfirm(
        t('languageIntegrityTitle'),
        t('languageIntegrityCorrupt').replace('{language}', label),
        () => regenerateLanguagePack(language),
        false,
        { confirmLabel: t('languageRegenerateAction'), cancelLabel: t('languageLaterAction'), icon: 'warning' }
      );
      return;
    }

    if (['incomplete', 'update_available'].includes(integrity)) {
      // This confirmation is intentionally user-facing and concise. Detailed key names,
      // source hashes, and incremental-update statistics remain internal diagnostics only.
      const message = t('languageUpdatePrompt').replace('{language}', label);
      showSettingsConfirm(
        t('languageUpdatePromptTitle'),
        message,
        () => regenerateLanguagePack(language),
        false,
        {
          confirmLabel: t('languageUpdateNowAction'),
          cancelLabel: t('languageUseCurrentAction'),
          onCancel: () => chooseLocale(language.code),
          icon: 'warning'
        }
      );
      return;
    }

    await chooseLocale(language.code);
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
    <section className="settings-scroll scroll-area">
      <div className="settings-grid refined-settings-grid compact-settings-grid">
        <div className="settings-card wide hero-card">
          <h2><Power size={18} /> {t('basic')}</h2>
          <div className="setting-stack service-grid">
            <label className="setting-row"><SettingName help={t('helpPinService')}>{t('pinService')}</SettingName><Switch checked={settings.pin_service_enabled} onChange={(v) => toggleService('pin_service_enabled', v)} /></label>
            <label className="setting-row"><SettingName help={t('helpHistoryService')}>{t('historyService')}</SettingName><Switch checked={settings.history_service_enabled} onChange={(v) => toggleService('history_service_enabled', v)} /></label>
            <label className="setting-row"><SettingName help={t('helpAutoHide')}>{t('autoHide')}</SettingName><Switch checked={settings.auto_hide_actions} onChange={(v) => update({ auto_hide_actions: v })} /></label>
            <label className="setting-row"><SettingName help={t('helpAutoStart')}>{t('autoStart')}</SettingName><Switch checked={settings.auto_start} onChange={toggleAutostart} /></label>
          </div>
        </div>

        <PrivacySection
          t={t}
          settings={settings}
          onPauseChange={async (paused) => { const saved = await api.setClipboardPaused(paused); setSettings(normalizeSettings(saved)); onBootChange({ ...boot, settings: normalizeSettings(saved) }); }}
          onPrivacyModeChange={async (mode) => { const saved = await api.setPrivacyFilterMode(mode); setSettings(normalizeSettings(saved)); onBootChange({ ...boot, settings: normalizeSettings(saved) }); }}
          onFilterChange={(patch) => update(patch)}
        />

        <AppearanceSection
          t={t}
          settings={settings}
          update={update}
          chooseLocale={chooseLocale}
          coreLanguageOptions={coreLanguageOptions}
          extraLanguageOptions={extraLanguageOptions}
          languageGenerationState={languageGenerationState}
          languageCodeDraft={languageCodeDraft}
          setLanguageCodeDraft={setLanguageCodeDraft}
          generateLanguagePack={generateLanguagePack}
          regenerateLanguagePack={regenerateLanguagePack}
          chooseExtraLanguage={chooseExtraLanguage}
          deleteLanguagePack={deleteLanguagePack}
          normalizeTranslationProvider={normalizeTranslationProvider}
          saveTranslationProvider={saveTranslationProvider}
          activeTranslationProvider={activeTranslationProvider}
          translationApiKeyDraft={translationApiKeyDraft}
          setTranslationApiKeyDraft={setTranslationApiKeyDraft}
          applyPastedTranslationApiKey={applyPastedTranslationApiKey}
          saveTranslationApiKey={saveTranslationApiKey}
          pasteTranslationApiKey={pasteTranslationApiKey}
          clearTranslationApiKey={clearTranslationApiKey}
          resetTranslationProvider={resetTranslationProvider}
          languagePackFolderPath={languagePackFolderPath}
          openLanguagePackFolder={openLanguagePackFolder}
          languageReloadState={languageReloadState}
          reloadLanguagePacks={reloadLanguagePacks}
          uiScale={uiScale}
          setUiScale={setUiScale}
          setPopupScale={setPopupScale}
          showSettingsAlert={showSettingsAlert}
        />

        {globalShortcutsSupported ? (
          <div className="settings-card wide shortcut-card">
            <h2><Keyboard size={18} /> {t('shortcuts')}</h2>
            <div className="shortcut-grid">
              {shortcutOrder.map((key) => {
                const value = settings.shortcuts?.[key] || defaultShortcuts[key];
                const warning = shortcutWarnings.get(key);
                const warningMessage = shortcutConflictMessage(warning, t);
                return (
                  <label key={key}>
                    <SettingName>{t(shortcutLabels[key] || key)}</SettingName>
                    <span className={`shortcut-input-shell ${warning ? 'has-warning' : ''}`}>
                      <input
                        className={warning ? 'conflict' : ''}
                        value={formatShortcutForDisplay(value)}
                        onKeyDown={(event) => captureShortcut(event, (nextValue) => updateShortcuts(key, nextValue))}
                        onChange={() => {}}
                        aria-invalid={warning ? 'true' : 'false'}
                        aria-describedby={warning ? `shortcut-warning-${key}` : undefined}
                      />
                      {warning ? (
                        <span
                          id={`shortcut-warning-${key}`}
                          className="shortcut-conflict-indicator"
                          role="status"
                          tabIndex="0"
                          aria-label={warningMessage}
                          data-tooltip={warningMessage}
                        >
                          <TriangleAlert size={15} />
                          <span className="visually-hidden">{warningMessage}</span>
                        </span>
                      ) : null}
                    </span>
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
        ) : null}

        {popupPositionSupported ? (
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
        ) : null}

        <DataSection
          t={t}
          boot={boot}
          dataUsage={dataUsage}
          historyLimit={historyLimit}
          setHistoryLimit={setHistoryLimit}
          cleanupDays={cleanupDays}
          setCleanupDays={setCleanupDays}
          cleanupPreservePinned={cleanupPreservePinned}
          setCleanupPreservePinned={setCleanupPreservePinned}
          deleteBeforeDays={deleteBeforeDays}
          exportHistory={exportHistory}
          importHistory={importHistory}
          clearData={clearData}
          logManagementHintLines={logManagementHintLines}
          logStatus={logStatus}
          updateLogRetentionDays={updateLogRetentionDays}
          openLogFolder={openLogFolder}
          refreshUsage={refreshUsage}
          clearLogFiles={clearLogFiles}
        />

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
    </section>
  );
}
