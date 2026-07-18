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
  // Language pack filenames and API targets use standard BCP-47 casing so
  // zh-Hant/zh-TW remain distinct from the built-in Simplified Chinese locale.
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
  const integrity = source.integrity || (Object.keys(messages).length ? 'complete' : 'corrupt');
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
    modifiedKeys: source.modifiedKeys || source.modified_keys || [],
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
    // Runtime choices retain their message/status payload so the Settings refresh action can
    // perform a true incremental update instead of translating the whole pack again.
    merged.set(normalized.code, { ...normalized });
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
    // Older, partial packs remain usable. createTranslator already falls back to a
    // same-family built-in catalog and then English for keys added by newer releases.
    if (normalized.integrity === 'corrupt' || !Object.keys(normalized.messages || {}).length) continue;
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
  const catalog = catalogs.get(lang) || catalogs.get('en');
  const localized = catalog?.messages || catalogs.get('en')?.messages || {};
  const fallback = catalogs.get('en')?.messages || localized;
  const outdated = new Set(catalog?.outdatedKeys || []);
  return (key) => {
    // Compatibility is evaluated per message key. Missing or source-outdated entries fall back
    // to English, while every unaffected translation in an older pack remains active.
    // Empty strings are valid translations used to intentionally hide redundant copy.
    if (outdated.has(key) && Object.prototype.hasOwnProperty.call(fallback, key)) return fallback[key];
    if (Object.prototype.hasOwnProperty.call(localized, key)) return localized[key];
    if (Object.prototype.hasOwnProperty.call(fallback, key)) return fallback[key];
    return '';
  };
}
