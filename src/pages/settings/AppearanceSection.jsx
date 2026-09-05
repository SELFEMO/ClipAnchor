import { Clock3, FolderOpen, Palette, RefreshCw, RotateCcw, Trash2, TriangleAlert } from 'lucide-react';
import { DropdownSelect, Segmented, selectPortablePath, SettingName, Stepper } from './widgets.jsx';

export function AppearanceSection({
  t,
  settings,
  update,
  chooseLocale,
  coreLanguageOptions,
  extraLanguageOptions,
  languageGenerationState,
  languageCodeDraft,
  setLanguageCodeDraft,
  generateLanguagePack,
  regenerateLanguagePack,
  chooseExtraLanguage,
  deleteLanguagePack,
  normalizeTranslationProvider,
  saveTranslationProvider,
  activeTranslationProvider,
  translationApiKeyDraft,
  setTranslationApiKeyDraft,
  applyPastedTranslationApiKey,
  saveTranslationApiKey,
  pasteTranslationApiKey,
  clearTranslationApiKey,
  resetTranslationProvider,
  languagePackFolderPath,
  openLanguagePackFolder,
  languageReloadState,
  reloadLanguagePacks,
  uiScale,
  setUiScale,
  setPopupScale,
  showSettingsAlert
}) {
  return (
    <>
      <div className="settings-card wide accent-card">
        <h2><Palette size={18} /> {t('appearance')}</h2>
        <div className="appearance-controls">
          <div className="appearance-basic-grid">
            <div className="control-row language-control-row appearance-language-card"><SettingName help={t('helpLanguage')}>{t('language')}</SettingName><Segmented className="language-segmented" value={['auto', 'en', 'zh'].includes(settings.locale) ? settings.locale : ''} onChange={chooseLocale} options={coreLanguageOptions} /></div>
            <label className="control-row appearance-theme-card"><SettingName help={t('helpTheme')}>{t('theme')}</SettingName><Segmented value={settings.theme} onChange={(v) => update({ theme: v }).catch((error) => showSettingsAlert(t('theme'), String(error)))} options={[{ value: 'system', label: t('system') }, { value: 'dark', label: t('dark') }, { value: 'light', label: t('light') }]} /></label>
            <label className="control-row appearance-animation-card"><SettingName help={t('helpAnimation')}>{t('animation')}</SettingName><Segmented value={settings.animation_mode} onChange={(v) => update({ animation_mode: v })} options={[{ value: 'elegant', label: t('elegant') }, { value: 'performance', label: t('performance') }]} /></label>
          </div>
          <div className="language-extension-panel">
            <div className="language-pack-heading">
              <span>{t('languagePackOther')}</span>
              <small>{t('translationApiNotice')}</small>
            </div>
            <p className="language-pack-warning">{t('languagePackUnofficialUserNotice')}</p>
            {extraLanguageOptions.length ? (
              <div className="language-pack-grid" role="radiogroup" aria-label={t('languagePackOther')}>
                {/* 扩展语言卡片把名称、代号和切换状态拆成独立层级，是为了避免操作按钮挤压主要信息。 */}
                {/* Extra-language cards separate the name, code, and switch state so action buttons cannot compress the primary information. */}
                {extraLanguageOptions.map((language) => {
                  const active = settings.locale === language.code;
                  const displayName = language.nativeName || language.label || language.code;
                  const integrity = language.integrity || 'complete';
                  const unavailable = integrity === 'corrupt';
                  const updateAvailable = ['incomplete', 'update_available'].includes(integrity);
                  return (
                    <div key={language.code} className={`language-pack-option ${active ? 'active' : ''} ${unavailable ? 'has-warning' : ''} ${updateAvailable ? 'has-update' : ''}`}>
                      <button type="button" className="language-pack-select" role="radio" aria-checked={active} title={displayName} onClick={() => chooseExtraLanguage(language)}>
                        <span className="language-pack-check" aria-hidden="true" />
                        <span className="language-pack-main">
                          <span className="language-pack-title-row">
                            <strong>{displayName}</strong>
                            <small className={`language-pack-code ${unavailable ? 'error-state' : updateAvailable ? 'update-state' : ''}`}>
                              {language.code}
                              {unavailable ? (
                                <TriangleAlert size={12} title={t('languageIntegrityCorrupt').replace('{language}', displayName)} aria-label={t('languagePackErrorWarning')} />
                              ) : updateAvailable ? (
                                <RefreshCw size={12} title={t('languageIntegrityWarning')} aria-label={t('languagePackUpdateWarning')} />
                              ) : null}
                            </small>
                          </span>
                          <span className="language-pack-state">{active ? t('languagePackActive') : t('languagePackClickToUse')}</span>
                        </span>
                      </button>
                      <span className="language-pack-actions">
                        <button type="button" className="language-pack-refresh" disabled={languageGenerationState.busy} title={t('languageRefreshAction')} aria-label={t('languageRefreshAction')} onClick={() => regenerateLanguagePack(language)}>
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
              <section className="translation-service-panel" aria-label={t('translationApiSettingsTitle')}>
                <div className="translation-service-panel__heading">
                  <strong>{t('translationApiSettingsTitle')}</strong>
                  <small>{t('translationApiSettingsHint')}</small>
                </div>
                <div className="translation-service-panel__controls">
                  <div className="translation-service-panel__field translation-service-panel__provider">
                    <span className="translation-service-panel__label">{t('translationProviderField')}</span>
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
                  </div>
                  <div className="translation-service-panel__field translation-service-panel__key">
                    <span className="translation-service-panel__label">{t('translationApiKeyField')}</span>
                    <div className="translation-service-panel__key-row">
                      <input
                        type="password"
                        aria-label={t('translationApiKeyField')}
                        value={activeTranslationProvider.supportsApiKey ? translationApiKeyDraft : ''}
                        disabled={languageGenerationState.busy || !activeTranslationProvider.supportsApiKey}
                        placeholder={activeTranslationProvider.supportsApiKey ? t('translationApiKeyPlaceholder') : t('translationApiKeyUnavailable')}
                        autoComplete="off"
                        spellCheck="false"
                        onChange={(event) => setTranslationApiKeyDraft(event.target.value)}
                        onPaste={(event) => {
                          const pasted = event.clipboardData?.getData('text');
                          if (typeof pasted !== 'string') return;
                          event.preventDefault();
                          applyPastedTranslationApiKey(pasted);
                        }}
                        onBlur={() => saveTranslationApiKey()}
                        onKeyDown={(event) => {
                          if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'v') {
                            event.preventDefault();
                            pasteTranslationApiKey();
                            return;
                          }
                          if (event.key === 'Enter') event.currentTarget.blur();
                        }}
                      />
                      <button className="soft-button paste-api-key-button" type="button" disabled={languageGenerationState.busy || !activeTranslationProvider.supportsApiKey} onClick={pasteTranslationApiKey}>{t('translationApiKeyPaste')}</button>
                      <button className="soft-button clear-api-key-button" type="button" disabled={languageGenerationState.busy || !activeTranslationProvider.supportsApiKey || !translationApiKeyDraft} onClick={clearTranslationApiKey}>{t('translationApiKeyClear')}</button>
                    </div>
                  </div>
                </div>
              </section>
              <div className="language-generator-actions">
                <input value={languageCodeDraft} onChange={(event) => setLanguageCodeDraft(event.target.value)} placeholder={t('languageCodePlaceholder')} />
                <button className="primary-button" type="button" disabled={languageGenerationState.busy} onClick={generateLanguagePack}>{languageGenerationState.busy ? t('generatingLanguage') : t('generateLanguage')}</button>
                <button className="soft-button reset-api-button" type="button" disabled={languageGenerationState.busy} onClick={resetTranslationProvider}><RotateCcw size={13} />{t('translationApiReset')}</button>
              </div>
              {languageGenerationState.busy && languageGenerationState.total ? (
                <div className="language-progress" role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-valuenow={languageGenerationState.percent}>
                  <div className="language-progress-meta">
                    <span>{languageGenerationState.message}</span>
                    <b>{t('languageProgressPercent').replace('{percent}', String(languageGenerationState.percent))}</b>
                  </div>
                  <span className="language-progress-track"><i style={{ width: `${languageGenerationState.percent}%` }} /></span>
                </div>
              ) : null}
              {!languageGenerationState.busy && languageGenerationState.message ? (
                <p className={`language-reload-status ${languageGenerationState.error ? 'error' : ''}`} role="status">
                  {languageGenerationState.message}
                </p>
              ) : null}
              <div className="language-folder-block">
                <label className="vertical language-folder-field">
                  <SettingName>{t('languagePackFolderLabel')}</SettingName>
                  <input
                    className="portable-path-input"
                    readOnly
                    dir="ltr"
                    spellCheck="false"
                    value={languagePackFolderPath}
                    title={languagePackFolderPath}
                    onFocus={selectPortablePath}
                    onDoubleClick={selectPortablePath}
                  />
                </label>
                <div className="language-folder-actions">
                  <button className="soft-button open-language-folder-button" type="button" onClick={openLanguagePackFolder}><FolderOpen size={15} /> {t('openLanguagePackFolder')}</button>
                  <button
                    className="soft-button reload-language-folder-button"
                    type="button"
                    disabled={languageReloadState.busy || languageGenerationState.busy}
                    title={t('reloadLanguagePacks')}
                    aria-label={t('reloadLanguagePacks')}
                    onClick={reloadLanguagePacks}
                  >
                    <RefreshCw className={languageReloadState.busy ? 'is-spinning' : ''} size={15} />
                    {languageReloadState.busy ? t('reloadingLanguagePacks') : t('reloadLanguagePacks')}
                  </button>
                </div>
                {languageReloadState.message ? (
                  <p className={`language-reload-status ${languageReloadState.error ? 'error' : ''}`} role="status">
                    {languageReloadState.message}
                  </p>
                ) : null}
              </div>
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
    </>
  );
}
