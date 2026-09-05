import { Database, Download, FolderOpen, RotateCcw, Trash2, Upload } from 'lucide-react';
import { selectPortablePath, SettingName, Stepper, Switch } from './widgets.jsx';

export function DataSection({
  t,
  boot,
  dataUsage,
  historyLimit,
  setHistoryLimit,
  cleanupDays,
  setCleanupDays,
  cleanupPreservePinned,
  setCleanupPreservePinned,
  deleteBeforeDays,
  exportHistory,
  importHistory,
  clearData,
  logManagementHintLines,
  logStatus,
  updateLogRetentionDays,
  openLogFolder,
  refreshUsage,
  clearLogFiles
}) {
  const logRetentionDays = Number(logStatus?.retention_days || boot.settings?.log_retention_days || 7);
  return (
    <>
      <div className="settings-card wide data-card full-data-card">
        <h2><Database size={18} /> {t('data')}</h2>
        <div className="data-management-primary-row">
          <div className="data-summary-strip"><span>{t('dataUsage')}</span><strong>{dataUsage?.display || '...'}</strong></div>
          <label className="scale-step-row history-limit-row"><SettingName help={t('helpHistoryLimit')}>{t('historyLimit')}</SettingName><Stepper value={historyLimit} min={0} max={10000} step={100} suffix={historyLimit === 0 ? ` ${t('unlimited')}` : ''} onChange={setHistoryLimit} onReset={() => setHistoryLimit(0)} resetLabel={t('resetScale')} /></label>
        </div>
        <label className="vertical database-path-field"><SettingName>{t('dbPath')}</SettingName><input className="portable-path-input" readOnly dir="ltr" spellCheck="false" value={boot.paths.database} title={boot.paths.database} onFocus={selectPortablePath} onDoubleClick={selectPortablePath} /></label>
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
            <label className="vertical log-path-field"><SettingName>{t('logPath')}</SettingName><input className="portable-path-input" readOnly dir="ltr" spellCheck="false" value={logStatus?.directory || boot.paths.logs || ''} title={logStatus?.directory || boot.paths.logs || ''} onFocus={selectPortablePath} onDoubleClick={selectPortablePath} /></label>
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
    </>
  );
}
