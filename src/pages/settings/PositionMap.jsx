import { useRef, useState } from 'react';
import { api } from '../../api.js';
import { HelpTip } from './widgets.jsx';

export function PositionMap({ settings, t, onSave }) {
  const mapRef = useRef(null);
  const screenWidth = Math.max(800, window.screen?.availWidth || window.screen?.width || 1920);
  const screenHeight = Math.max(600, window.screen?.availHeight || window.screen?.height || 1080);
  const popupScale = Math.min(200, Math.max(50, Number(settings.popup_scale_percent || 100))) / 100;
  const popupWidth = Math.round(Math.min(520, Math.max(280, Number(settings.popup_width || 340))) * popupScale);
  const popupHeight = Math.round(Math.min(360, Math.max(160, Number(settings.popup_height || 220))) * popupScale);
  const mockMaxWidth = 150;
  const mockPopupWidth = Math.round(mockMaxWidth);
  const mockPopupHeight = Math.round(Math.min(104, Math.max(62, mockMaxWidth * (popupHeight / popupWidth))));
  const maxX = Math.max(0, screenWidth - popupWidth);
  const maxY = Math.max(0, screenHeight - popupHeight);
  const clamp = (value, min, max) => Math.min(max, Math.max(min, value));
  const [draft, setDraft] = useState({
    x: clamp(settings.popup_x ?? 24, 0, maxX),
    y: clamp(settings.popup_y ?? 24, 0, maxY)
  });
  const [saving, setSaving] = useState(false);

  function updateFromPointer(event) {
    const rect = mapRef.current?.getBoundingClientRect();
    if (!rect) return;
    const localX = clamp(event.clientX - rect.left, 0, rect.width);
    const localY = clamp(event.clientY - rect.top, 0, rect.height);
    const usableWidth = Math.max(1, rect.width - mockPopupWidth);
    const usableHeight = Math.max(1, rect.height - mockPopupHeight);
    const ratioX = clamp((localX - mockPopupWidth / 2) / usableWidth, 0, 1);
    const ratioY = clamp((localY - mockPopupHeight / 2) / usableHeight, 0, 1);
    const nextX = Math.round(ratioX * maxX);
    const nextY = Math.round(ratioY * maxY);
    setDraft({ x: nextX, y: nextY });
  }

  function beginDrag(event) {
    event.preventDefault();
    updateFromPointer(event);
    event.currentTarget.setPointerCapture?.(event.pointerId);
  }

  async function save() {
    setSaving(true);
    try {
      // 定位器用“屏幕尺寸减去弹窗尺寸”作为最大坐标，是为了让真实弹窗和预览弹窗都不会越出屏幕边界。
      // The locator uses screen size minus popup size as the maximum coordinate so both the real popup and preview popup stay inside screen bounds.
      await api.savePopupPosition(draft.x, draft.y);
      onSave(draft.x, draft.y);
    } finally {
      setSaving(false);
    }
  }

  const ratioX = maxX > 0 ? clamp(draft.x / maxX, 0, 1) : 0;
  const ratioY = maxY > 0 ? clamp(draft.y / maxY, 0, 1) : 0;
  const left = `calc(${ratioX * 100}% - ${ratioX * mockPopupWidth}px)`;
  const top = `calc(${ratioY * 100}% - ${ratioY * mockPopupHeight}px)`;

  return (
    <div className="position-map-card">
      <div className="position-map-copy compact-title-help">
        <strong>{t('positionMapTitle')}</strong>
        <HelpTip text={t('positionMapHint')} />
      </div>
      <div
        ref={mapRef}
        className="position-map-canvas"
        style={{ aspectRatio: `${Math.max(1, Math.round(maxX))} / ${Math.max(1, Math.round(maxY))}` }}
        onPointerDown={beginDrag}
        onPointerMove={(event) => event.buttons === 1 && updateFromPointer(event)}
      >
        <div className="position-map-grid" />
        <div className="position-map-safe-area" />
        <div className="position-map-axis x-axis">max X {Math.round(maxX)}px</div>
        <div className="position-map-axis y-axis">max Y {Math.round(maxY)}px</div>
        <div className="position-map-popup" style={{ left, top, '--mock-popup-width': `${mockPopupWidth}px`, '--mock-popup-height': `${mockPopupHeight}px` }}>
          <b>ClipAnchor</b>
          <span>{t('dragHint')}</span>
        </div>
      </div>
      <div className="position-map-footer">
        <code>X {draft.x}px · Y {draft.y}px · max X {Math.round(maxX)}px · max Y {Math.round(maxY)}px</code>
        <div className="button-row compact-actions">
          <button className="soft-button" onClick={() => setDraft({ x: 24, y: 24 })}>{t('resetPosition')}</button>
          <button className="primary-button" disabled={saving} onClick={save}>{saving ? '...' : t('confirmPosition')}</button>
        </div>
      </div>
    </div>
  );
}
