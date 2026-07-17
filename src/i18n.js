const builtinModules = import.meta.glob('./locales/*.js', { eager: true });
const coreLanguageCodes = new Set(['en', 'zh']);

function canonicalizeLocalePart(part, index) {
  const value = String(part || '').replace(/[^a-zA-Z0-9]/g, '');
  if (!value) return '';
  if (index === 0) return value.toLowerCase();
  if (/^[a-zA-Z]{4}$/.test(value)) return `${value.slice(0, 1).toUpperCase()}${value.slice(1).toLowerCase()}`;
  if (/^[a-zA-Z]{2}$/.test(value) || /^\d{3}$/.test(value)) return value.toUpperCase();
  return value.toLowerCase();
}

function normalizeLanguageCode(value) {
  const parts = String(value || '')
    .trim()
    .replace(/_/g, '-')
    .split('-')
    .map((part, index) => canonicalizeLocalePart(part, index))
    .filter(Boolean);
  // 语言包文件名和 API 目标语言都使用标准 BCP-47 大小写，是为了区分 zh-Hant/zh-TW 这类繁体中文与内置简体中文。
  // Language pack filenames and API target locales use standard BCP-47 casing so zh-Hant/zh-TW stay distinct from the built-in Simplified Chinese locale.
  return parts.join('-');
}

function languageCodeFromPath(path) {
  const name = String(path || '').split('/').pop() || '';
  return normalizeLanguageCode(name.replace(/\.(js|json)$/i, ''));
}

function normalizeLanguagePack(raw, path = '') {
  const source = raw?.default || raw || {};
  const code = normalizeLanguageCode(source.code || source.locale || languageCodeFromPath(path));
  const messages = source.messages || source.dictionary || {};
  if (!code || !messages || typeof messages !== 'object') return null;
  const integrity = source.integrity || (Object.keys(messages).length ? 'complete' : 'incomplete');
  return {
    code,
    label: source.label || source.name || inferLanguageLabel(code),
    nativeName: source.nativeName || source.native_name || source.label || source.name || inferLanguageLabel(code),
    builtin: Boolean(source.builtin),
    source: source.source || '',
    generatedAt: source.generatedAt || source.generated_at || '',
    fileName: source.fileName || source.file_name || '',
    integrity,
    missingKeys: source.missingKeys || source.missing_keys || [],
    integrityError: source.integrityError || source.integrity_error || '',
    messageStatus: source.messageStatus || source.message_status || {},
    outdatedKeys: source.outdatedKeys || source.outdated_keys || [],
    removedKeys: source.removedKeys || source.removed_keys || [],
    format: source.format || '',
    sourceLocale: source.sourceLocale || source.source_locale || 'en',
    messages
  };
}

const builtinLanguages = Object.entries(builtinModules)
  .map(([path, module]) => normalizeLanguagePack(module, path))
  .filter(Boolean)
  .sort((a, b) => (a.code === 'en' ? -1 : b.code === 'en' ? 1 : a.label.localeCompare(b.label)));

const builtinCatalogs = new Map(builtinLanguages.map((language) => [language.code, language]));

export function normalizeLocaleCode(value) {
  return normalizeLanguageCode(value);
}

export function inferLanguageLabel(code) {
  const normalized = normalizeLanguageCode(code);
  if (!normalized) return '';
  try {
    const display = new Intl.DisplayNames([normalized, 'en'], { type: 'language' });
    const label = display.of(normalized);
    if (label) return label;
  } catch (_) {}
  return normalized.toUpperCase();
}

export function detectSystemLanguageCode() {
  return normalizeLanguageCode(navigator.language || navigator.userLanguage || 'en') || 'en';
}

export function listBuiltinLanguages() {
  return builtinLanguages.map((language) => ({ ...language, messages: undefined }));
}

export function listLanguageChoices(runtimePacks = []) {
  const merged = new Map();
  for (const language of builtinLanguages) {
    merged.set(language.code, { ...language, messages: undefined });
  }
  for (const pack of runtimePacks || []) {
    const normalized = normalizeLanguagePack(pack);
    if (!normalized || coreLanguageCodes.has(normalized.code)) continue;
    merged.set(normalized.code, { ...normalized, messages: undefined });
  }
  return Array.from(merged.values()).sort((a, b) => {
    const leftCore = coreLanguageCodes.has(a.code);
    const rightCore = coreLanguageCodes.has(b.code);
    if (leftCore !== rightCore) return leftCore ? -1 : 1;
    return a.label.localeCompare(b.label);
  });
}

function buildCatalogMap(runtimePacks = []) {
  const catalogs = new Map(builtinCatalogs);
  for (const pack of runtimePacks || []) {
    const normalized = normalizeLanguagePack(pack);
    if (!normalized || coreLanguageCodes.has(normalized.code)) continue;
    if (['corrupt', 'incomplete'].includes(normalized.integrity) || !Object.keys(normalized.messages || {}).length) continue;
    // 可增量更新的语言包仍可继续使用，缺失的新文本会走内置回退；只有损坏、不完整或完全无内容的文件才被排除。
    // Packs with incremental updates available remain usable and fall back for newly missing text; only damaged, incomplete, or empty files are excluded.
    catalogs.set(normalized.code, normalized);
  }
  return catalogs;
}

function findCatalog(catalogs, code) {
  const normalized = normalizeLanguageCode(code);
  if (catalogs.has(normalized)) return catalogs.get(normalized);
  const base = normalized.split('-')[0];
  if (catalogs.has(base)) return catalogs.get(base);
  return catalogs.get('en');
}

function resolveLocale(locale, runtimePacks = []) {
  const catalogs = buildCatalogMap(runtimePacks);
  const requested = locale === 'auto' ? detectSystemLanguageCode() : locale;
  return findCatalog(catalogs, requested)?.code || 'en';
}

export function getReferenceMessages(code = 'en') {
  return { ...(builtinCatalogs.get(normalizeLanguageCode(code))?.messages || builtinCatalogs.get('en')?.messages || {}) };
}

export function createTranslator(locale, runtimePacks = []) {
  const catalogs = buildCatalogMap(runtimePacks);
  const lang = resolveLocale(locale, runtimePacks);
  const localized = catalogs.get(lang)?.messages || catalogs.get('en')?.messages || {};
  const baseCode = lang.split('-')[0];
  const baseFallback = catalogs.get(baseCode)?.messages;
  const fallback = catalogs.get('en')?.messages || localized;
  return (key) => {
    // 空字符串也是合法翻译，用于刻意隐藏冗余说明；不能用 || 回退，否则会把内部键名暴露给用户。
    // An empty string is a valid translation for intentionally hidden notes; avoid || fallback so internal keys are never shown to users.
    if (Object.prototype.hasOwnProperty.call(localized, key)) {
      return localized[key];
    }
    if (baseFallback && Object.prototype.hasOwnProperty.call(baseFallback, key)) {
      // 运行时语言包缺少新增文案时，先回退到同语族内置语言，是为了避免扩展中文包显示英文系统提示。
      // When a runtime pack misses newly added text, fallback to the built-in language with the same base code before English so Chinese extension packs do not show English system notices.
      return baseFallback[key];
    }
    if (Object.prototype.hasOwnProperty.call(fallback, key)) {
      return fallback[key];
    }
    return '';
  };
}
