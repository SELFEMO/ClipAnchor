import { useEffect, useRef } from 'react';

export function useTransientScrollbar(externalRef = null, hideDelay = 720) {
  const internalRef = useRef(null);
  const targetRef = externalRef || internalRef;

  useEffect(() => {
    const element = targetRef.current;
    if (!element) return undefined;
    let timer = 0;
    function revealDuringScroll() {
      element.classList.add('is-scrolling');
      window.clearTimeout(timer);
      // 中文：只在真实 scroll 事件后短暂显示滚动条，是为了避免鼠标进入内容区就出现视觉噪声。
      // English: Reveal the scrollbar briefly only after a real scroll event so merely entering the content area does not add visual noise.
      timer = window.setTimeout(() => element.classList.remove('is-scrolling'), hideDelay);
    }
    element.addEventListener('scroll', revealDuringScroll, { passive: true });
    return () => {
      window.clearTimeout(timer);
      element.classList.remove('is-scrolling');
      element.removeEventListener('scroll', revealDuringScroll);
    };
  });

  return targetRef;
}
