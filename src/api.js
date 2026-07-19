import { invoke } from '@tauri-apps/api/core';
import { open, save } from '@tauri-apps/plugin-dialog';

export const api = {
  bootstrap: () => invoke('get_bootstrap'),
  listHistory: (query = '', kind = 'all') => invoke('list_history', { query, kind }),
  deleteRecords: (ids) => invoke('delete_records', { ids }),
  clearAllData: (preservePinned = true) => invoke('clear_all_data', { preservePinned }),
  deleteHistoryBeforeDays: (days, preservePinned = true) => invoke('delete_history_before_days', { days, preservePinned }),
  toggleRecordPin: (id, pinned) => invoke('toggle_record_pin', { id, pinned }),
  createTextRecord: (text, pinned = false) => invoke('create_text_record', { text, pinned }),
  updateTextRecord: (id, text) => invoke('update_text_record', { id, text }),
  pinHistoryItem: (id) => invoke('pin_history_item', { id }),
  validateRecord: (id) => invoke('validate_record', { id }),
  copyItem: (id) => invoke('copy_item', { id }),
  getPopupItem: (id) => invoke('get_popup_item', { id }),
  readImageDataUrl: (id) => invoke('read_image_data_url', { id }),
  readFilePreviews: (id) => invoke('read_file_previews', { id }),
  closePopup: (id) => invoke('close_popup', { id }),
  pinPopup: (id) => invoke('pin_popup', { id }),
  resizePopup: (id, width, height) => invoke('resize_popup', { id, width, height }),
  refreshPopupShape: (id) => invoke('refresh_popup_shape', { id }),
  saveSettings: (settings) => invoke('save_settings', { settingsValue: settings }),
  checkShortcutConflicts: (shortcuts) => invoke('check_shortcut_conflicts', { shortcuts }),
  setPinService: (enabled) => invoke('set_pin_service', { enabled }),
  setHistoryService: (enabled) => invoke('set_history_service', { enabled }),
  setPrivacyMode: (enabled) => invoke('set_privacy_mode', { enabled }),
  setPrivacyFilterMode: (mode) => invoke('set_privacy_filter_mode', { mode }),
  setAutostart: (enabled) => invoke('set_autostart', { enabled }),
  openPositionOverlay: () => invoke('open_position_overlay'),
  savePopupPosition: (x, y) => invoke('save_popup_position', { x, y }),
  exportHistory: async (format = 'json') => {
    const extension = format === 'csv' ? 'csv' : 'json';
    const selected = await save({
      title: format === 'csv' ? 'Export ClipAnchor CSV history' : 'Export ClipAnchor JSON history',
      defaultPath: `clipanchor-history.${extension}`,
      filters: [{ name: format === 'csv' ? 'ClipAnchor CSV history' : 'ClipAnchor JSON history', extensions: [extension] }]
    });
    if (!selected) return null;
    return invoke('export_history_to_path', { format, outputPath: selected });
  },
  importHistory: async (format = 'json') => {
    const extension = format === 'csv' ? 'csv' : 'json';
    const selected = await open({
      title: format === 'csv' ? 'Import ClipAnchor CSV history' : 'Import ClipAnchor JSON history',
      multiple: false,
      filters: [{ name: format === 'csv' ? 'ClipAnchor CSV history' : 'ClipAnchor JSON history', extensions: [extension] }]
    });
    if (!selected) return null;
    return invoke('import_history_from_path', { format, inputPath: selected });
  },
  getUpdateStatus: () => invoke('get_update_status'),
  checkUpdate: (source = 'manual') => invoke('check_update', { source }),
  installDownloadedUpdate: () => invoke('install_downloaded_update'),
  dismissUpdatePrompt: () => invoke('dismiss_update_prompt'),
  getDataUsage: () => invoke('get_data_usage'),
  listLanguagePacks: (referenceMessages = {}) => {
    // Pass the full English dictionary whenever available. The backend uses each source string's
    // lightweight hash to detect changed UI copy, while key-only arrays remain accepted for
    // compatibility with older frontend builds.
    const normalizedReference = Array.isArray(referenceMessages)
      ? referenceMessages.filter((key) => typeof key === 'string')
      : (referenceMessages && typeof referenceMessages === 'object' ? referenceMessages : {});
    return invoke('list_language_packs', { requiredKeys: normalizedReference });
  },
  openLanguagePackFolder: () => invoke('open_language_pack_folder'),
  readClipboardTextForInput: () => invoke('read_clipboard_text_for_input'),
  saveLanguagePack: (pack) => invoke('save_language_pack', { pack }),
  deleteLanguagePack: (code) => invoke('delete_language_pack', { code }),
  logLanguagePackEvent: (event, code = '', provider = '', success = null, detail = '') => invoke('log_language_pack_event', { event, code, provider, success, detail }),
  translateText: (provider, targetCode, text, apiKey = '') => invoke('translate_ui_text', { provider, targetCode, text, apiKey }),
  getLogStatus: () => invoke('get_log_status'),
  clearLogs: () => invoke('clear_logs'),
  openLogFolder: () => invoke('open_log_folder'),
  validateFavorites: () => invoke('validate_favorites'),
  deleteRecordsForce: (ids) => invoke('delete_records_force', { ids }),
  togglePopupFavorite: (id, pinned) => invoke('toggle_popup_favorite', { id, pinned }),
  quitApp: () => invoke('quit_app'),
  minimizeWindow: () => invoke('minimize_window'),
  toggleMaximizeWindow: () => invoke('toggle_maximize_window'),
  closeMainWindow: () => invoke('close_main_window'),
};
