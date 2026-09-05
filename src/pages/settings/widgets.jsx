import { useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { HelpCircle, Minus, Plus, RotateCcw, TriangleAlert } from 'lucide-react';
import { captureShortcutValue } from '../../shortcutDisplay.js';

export function Switch({ checked, onChange }) {
  return <button className={`switch ${checked ? 'on' : ''}`} onClick={() => onChange(!checked)}><span /></button>;
}

export function captureShortcut(event, setter) {
  event.preventDefault();
  const shortcut = captureShortcutValue(event);
  if (shortcut) setter(shortcut);
}

export function selectPortablePath(event) {
  // Read-only path fields keep native text selection so Cmd/Ctrl+C works on macOS, Windows, and Linux.
  event.currentTarget.select();
}

export function Segmented({ value, options, onChange, className = '' }) {
  const classes = ['segmented', className].filter(Boolean).join(' ');
  return (
    <div className={classes}>
      {options.map((option) => (
        <button
          key={option.value}
          type="button"
          className={value === option.value ? 'active' : ''}
          title={option.label}
          onClick={() => onChange(option.value)}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}

export function DropdownSelect({ value, options, onChange, disabled = false, ariaLabel = '' }) {
  const [open, setOpen] = useState(false);
  const current = options.find((option) => option.value === value) || options[0];

  function choose(optionValue) {
    setOpen(false);
    if (optionValue !== value) onChange(optionValue);
  }

  return (
    <div
      className={`codex-dropdown ${open ? 'open' : ''}`}
      onBlur={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget)) setOpen(false);
      }}
    >
      <button
        type="button"
        className="codex-dropdown-button"
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-label={ariaLabel}
        disabled={disabled}
        onClick={() => setOpen((next) => !next)}
      >
        <span>{current?.label || value}</span>
        <i aria-hidden="true">⌄</i>
      </button>
      {open ? (
        <div className="codex-dropdown-menu" role="listbox">
          {options.map((option) => (
            <button
              key={option.value}
              type="button"
              role="option"
              aria-selected={option.value === value}
              className={option.value === value ? 'selected' : ''}
              onMouseDown={(event) => event.preventDefault()}
              onClick={() => choose(option.value)}
            >
              <span>{option.label}</span>
              {option.value === value ? <em>✓</em> : null}
            </button>
          ))}
        </div>
      ) : null}
    </div>
  );
}

function estimateHelpBubbleWidth(text) {
  const content = Array.from(String(text || '').trim());
  const weightedLength = content.reduce((sum, char) => {
    if (/\s/.test(char)) return sum + 0.32;
    if (/[\u2e80-\u9fff\uff00-\uffef]/.test(char)) return sum + 1.05;
    if (/[A-Z0-9]/.test(char)) return sum + 0.72;
    return sum + 0.56;
  }, 0);
  const hasCjk = content.some((char) => /[\u2e80-\u9fff\uff00-\uffef]/.test(char));
  const targetLineUnits = hasCjk ? 18 : 34;
  const estimatedLines = Math.max(1, Math.ceil(weightedLength / targetLineUnits));
  const balancedUnits = Math.ceil(weightedLength / Math.min(3, estimatedLines));
  const preferred = Math.round(42 + balancedUnits * (hasCjk ? 13.5 : 8.2));
  const viewportMax = Math.max(180, window.innerWidth - 36);
  // 气泡按“预计行数 + 文本长度”估算宽度，而不是固定宽度，避免短文案过宽、长文案最后一行只剩一两个字。
  // Bubble width is estimated from expected line count plus text length instead of a fixed value, avoiding oversized short hints and one-word trailing lines.
  return Math.min(Math.max(168, preferred), Math.min(360, viewportMax));
}

function calculateHelpBubblePlacement(rect, width) {
  const margin = 18;
  const center = rect.left + rect.width / 2;
  const viewportWidth = window.innerWidth;
  const normalized = Math.max(-1, Math.min(1, (center - viewportWidth / 2) / Math.max(1, viewportWidth / 2)));
  const anchorRatio = Math.max(0.42, Math.min(0.58, 0.5 + normalized * 0.13));
  const unclampedLeft = center - width * anchorRatio;
  const left = Math.min(Math.max(margin, unclampedLeft), viewportWidth - width - margin);
  const actualAnchor = Math.max(20, Math.min(width - 20, center - left));
  const align = actualAnchor < width * 0.45 ? 'left' : actualAnchor > width * 0.55 ? 'right' : 'center';
  return { left, actualAnchor, align };
}

export function HelpTip({ text }) {
  const tipRef = useRef(null);
  const [bubble, setBubble] = useState(null);
  if (!text) return null;

  function showBubble() {
    const rect = tipRef.current?.getBoundingClientRect();
    if (!rect) return;
    const width = estimateHelpBubbleWidth(text);
    const { left, actualAnchor, align } = calculateHelpBubblePlacement(rect, width);
    const fitsAbove = rect.top > 92;
    setBubble({
      left,
      top: fitsAbove ? rect.top - 10 : rect.bottom + 10,
      width,
      anchorX: actualAnchor,
      align,
      placement: fitsAbove ? 'top' : 'bottom'
    });
  }

  function hideBubble() {
    setBubble(null);
  }

  return (
    <>
      <span
        ref={tipRef}
        className="help-tip"
        tabIndex="0"
        aria-label={text}
        onMouseEnter={showBubble}
        onMouseLeave={hideBubble}
        onFocus={showBubble}
        onBlur={hideBubble}
      >
        <HelpCircle size={14} />
      </span>
      {bubble ? createPortal(
        <span
          className={`help-bubble floating-help-bubble ${bubble.placement === 'bottom' ? 'below' : 'above'} align-${bubble.align}`}
          style={{ left: `${bubble.left}px`, top: `${bubble.top}px`, width: `${bubble.width}px`, '--help-anchor-x': `${bubble.anchorX}px` }}
        >
          {text}
        </span>,
        document.body
      ) : null}
    </>
  );
}

export function SettingName({ children, help }) {
  return <span className="setting-name"><span>{children}</span><HelpTip text={help} /></span>;
}

export function Stepper({ value, min, max, step = 5, suffix = '', onChange, onReset, resetLabel = 'Reset' }) {
  const current = Number(value);
  const clamp = (next) => Math.min(max, Math.max(min, next));
  const update = (next) => onChange(clamp(next));
  return (
    <div className="stepper-control">
      <button type="button" aria-label="Decrease" disabled={current <= min} onClick={() => update(current - step)}><Minus size={14} /></button>
      <strong>{current}{suffix}</strong>
      <button type="button" aria-label="Increase" disabled={current >= max} onClick={() => update(current + step)}><Plus size={14} /></button>
      {onReset ? <button type="button" className="reset-stepper" aria-label={resetLabel} title={resetLabel} onClick={() => onReset()}><RotateCcw size={13} /></button> : null}
    </div>
  );
}

export function SettingsSoftDialog({ dialog, t, onClose }) {
  if (!dialog) return null;
  const DialogIcon = dialog.icon === 'warning' ? TriangleAlert : HelpCircle;
  async function runConfirm() {
    const action = dialog.onConfirm;
    onClose();
    if (action) await action();
  }
  async function runCancel() {
    const action = dialog.onCancel;
    onClose();
    if (action) await action();
  }
  return (
    <div className="soft-modal-backdrop settings-dialog-backdrop" role="presentation" onClick={onClose}>
      <section className={`soft-modal-card settings-dialog-card ${dialog.danger ? 'danger' : ''} ${dialog.wide ? 'wide' : ''}`} role="dialog" aria-modal="true" onClick={(event) => event.stopPropagation()}>
        <span className={`settings-dialog-icon ${dialog.icon === 'warning' ? 'warning' : ''}`}><DialogIcon size={19} /></span>
        <div className="settings-dialog-copy">
          <strong>{dialog.title}</strong>
          <p>{dialog.message}</p>
        </div>
        <div className="settings-dialog-actions">
          {dialog.kind === 'confirm' ? <button className="soft-button" onClick={runCancel}>{dialog.cancelLabel || t('cancel')}</button> : null}
          <button className={dialog.danger ? 'soft-button danger-line' : 'primary-button'} onClick={dialog.kind === 'confirm' ? runConfirm : onClose}>{dialog.confirmLabel || t('ok')}</button>
        </div>
      </section>
    </div>
  );
}
