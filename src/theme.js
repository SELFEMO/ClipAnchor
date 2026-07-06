import { useEffect, useState } from 'react';

const THEME_QUERY = '(prefers-color-scheme: dark)';

export function systemPrefersDark() {
  if (typeof window === 'undefined' || !window.matchMedia) return true;
  return window.matchMedia(THEME_QUERY).matches;
}

export function resolveThemeClass(settings = {}, prefersDark = systemPrefersDark()) {
  const theme = settings.theme || 'system';
  return theme === 'light' || (theme === 'system' && !prefersDark) ? 'theme-light' : 'theme-dark';
}

export function useSystemThemePreference() {
  const [prefersDark, setPrefersDark] = useState(() => systemPrefersDark());

  useEffect(() => {
    if (typeof window === 'undefined' || !window.matchMedia) return undefined;
    const mediaQuery = window.matchMedia(THEME_QUERY);
    const syncPreference = () => {
      // 系统主题变化不一定会触发 React 状态更新，因此这里主动写入状态让“跟随系统”实时重绘。
      // System theme changes do not automatically trigger React renders, so this state update makes System mode repaint immediately.
      setPrefersDark(mediaQuery.matches);
    };
    syncPreference();
    if (mediaQuery.addEventListener) {
      mediaQuery.addEventListener('change', syncPreference);
      return () => mediaQuery.removeEventListener('change', syncPreference);
    }
    mediaQuery.addListener(syncPreference);
    return () => mediaQuery.removeListener(syncPreference);
  }, []);

  return prefersDark;
}

export function useDocumentThemeSync(themeClass) {
  useEffect(() => {
    if (typeof document === 'undefined') return undefined;
    const inverseTheme = themeClass === 'theme-light' ? 'theme-dark' : 'theme-light';
    document.documentElement.classList.remove(inverseTheme);
    document.body.classList.remove(inverseTheme);
    document.documentElement.classList.add(themeClass);
    document.body.classList.add(themeClass);
    document.documentElement.style.colorScheme = themeClass === 'theme-light' ? 'light' : 'dark';
    // 根节点同步主题类，是为了让弹窗窗口、系统滚动条和软弹窗背景在系统主题切换时一起更新。
    // Syncing the theme on root nodes keeps popup windows, native scrollbars, and soft-modal backdrops aligned during OS theme changes.
    return () => {
      document.documentElement.classList.remove(themeClass);
      document.body.classList.remove(themeClass);
    };
  }, [themeClass]);
}
