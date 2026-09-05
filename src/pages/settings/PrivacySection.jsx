import { Shield } from 'lucide-react';
import { Segmented, SettingName, Switch } from './widgets.jsx';

export function PrivacySection({ t, settings, onPauseChange, onPrivacyModeChange, onFilterChange }) {
  return (
    <div className="settings-card wide hero-card">
      <h2><Shield size={18} /> {t('privacySection')}</h2>
      <div className="setting-stack service-grid">
        <label className="setting-row"><SettingName help={t('helpClipboardPause')}>{t('pauseClipboard')}</SettingName><Switch checked={Boolean(settings.clipboard_paused)} onChange={onPauseChange} /></label>
        <label className="setting-row setting-row-segmented privacy-filter-row"><SettingName help={t('helpPrivacy')}>{t('privacyMode')}</SettingName><Segmented value={settings.privacy_filter_mode || (settings.privacy_mode ? 'light' : 'off')} options={[{ value: 'off', label: t('privacyOffMode') }, { value: 'light', label: t('privacyLightMode') }]} onChange={onPrivacyModeChange} /></label>
        <div className="capture-type-row">
          <SettingName help={t('helpFilterTypes')}>{t('filterCapturedTypes')}</SettingName>
          <div className="capture-type-grid">
            <label className="capture-type-item"><span>{t('filterText')}</span><Switch checked={settings.filter_text !== false} onChange={(value) => onFilterChange({ filter_text: value })} /></label>
            <label className="capture-type-item"><span>{t('filterImage')}</span><Switch checked={settings.filter_image !== false} onChange={(value) => onFilterChange({ filter_image: value })} /></label>
            <label className="capture-type-item"><span>{t('filterFile')}</span><Switch checked={settings.filter_file !== false} onChange={(value) => onFilterChange({ filter_file: value })} /></label>
          </div>
        </div>
      </div>
    </div>
  );
}
