export const LANGUAGE_PACK_PROGRESS_INTERVAL_MS = 200;

// Flush on the first item, the last item, or when the throttle interval has elapsed so the settings page is not redrawn for every translated string.
export function shouldFlushLanguageProgress(nowMs, lastFlushMs, current, total, minIntervalMs = LANGUAGE_PACK_PROGRESS_INTERVAL_MS) {
  if (!total || current <= 0) return false;
  if (current === 1 || current >= total) return true;
  return nowMs - lastFlushMs >= minIntervalMs;
}

export function yieldLanguagePackFrame() {
  return new Promise((resolve) => {
    if (typeof requestAnimationFrame !== 'function') {
      setTimeout(resolve, 0);
      return;
    }
    requestAnimationFrame(() => requestAnimationFrame(resolve));
  });
}
