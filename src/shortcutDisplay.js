function detectPlatform() {
  const platformSource = typeof navigator === 'undefined' ? {} : navigator;
  const platform = String(platformSource.platform || '').toLowerCase();
  const userAgent = String(platformSource.userAgent || '').toLowerCase();
  if (platform.includes('mac') || userAgent.includes('mac os x')) return 'macos';
  if (platform.includes('win') || userAgent.includes('windows')) return 'windows';
  return 'linux';
}

function normalizeKeyName(key) {
  const raw = String(key || '').trim();
  const lower = raw.toLowerCase();
  if (!raw) return '';
  if (lower === 'control' || lower === 'ctrl') return 'Ctrl';
  if (lower === 'shift') return 'Shift';
  if (lower === 'alt' || lower === 'option') return 'Alt';
  if (['meta', 'command', 'cmd', 'super', 'win', 'windows'].includes(lower)) return 'Meta';
  if (lower === 'escape') return 'Esc';
  if (lower === 'arrowup') return 'Up';
  if (lower === 'arrowdown') return 'Down';
  if (lower === 'arrowleft') return 'Left';
  if (lower === 'arrowright') return 'Right';
  if (raw.length === 1) return raw.toUpperCase();
  return raw.charAt(0).toUpperCase() + raw.slice(1);
}

export function normalizeShortcutForStorage(value) {
  return String(value || '')
    .split('+')
    .map(normalizeKeyName)
    .filter(Boolean)
    .join('+');
}

export function captureShortcutValue(event) {
  const parts = [];
  if (event.ctrlKey) parts.push('Ctrl');
  if (event.metaKey) parts.push('Meta');
  if (event.altKey) parts.push('Alt');
  if (event.shiftKey) parts.push('Shift');
  const key = normalizeKeyName(event.key);
  if (!['Ctrl', 'Shift', 'Alt', 'Meta'].includes(key)) parts.push(key);
  // 快捷键持久化使用跨平台规范名，是为了让同一份 settings.json 可以在 Windows、macOS 与 Linux 间迁移。
  // Shortcut persistence uses platform-neutral names so the same settings.json can move between Windows, macOS, and Linux.
  return parts.join('+');
}

function displayToken(token, platform = detectPlatform()) {
  const canonical = normalizeKeyName(token);
  if (platform === 'macos') {
    if (canonical === 'Ctrl') return 'Control';
    if (canonical === 'Alt') return 'Option';
    if (canonical === 'Meta') return 'Command';
    return canonical;
  }
  if (platform === 'windows') {
    if (canonical === 'Meta') return 'Win';
    return canonical;
  }
  if (canonical === 'Meta') return 'Super';
  return canonical;
}

export function shortcutDisplayTokens(value, platform = detectPlatform()) {
  return normalizeShortcutForStorage(value)
    .split('+')
    .filter(Boolean)
    .map((token) => displayToken(token, platform));
}

export function formatShortcutForDisplay(value, platform = detectPlatform()) {
  // 显示层按运行平台翻译键名，是为了让 macOS 用户看到 Control/Option/Command，而 Windows/Linux 保持各自熟悉的命名。
  // The display layer translates key names per platform so macOS users see Control/Option/Command while Windows/Linux keep familiar labels.
  return shortcutDisplayTokens(value, platform).join('+');
}
