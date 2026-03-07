/**
 * Param cell inputs used in the Params grid.
 *
 * Exposes specialised inputs so the grid can mix numeric, select, and toggle controls
 * while keeping visual and interaction behaviour consistent.
 *
 * @keyframes dirtyFlash - Subtle flash animation when input becomes dirty
 */

import { type RefObject, useEffect, useMemo, useRef, useState, KeyboardEvent, memo } from 'react';
import type { ParamDef } from '../../types';

// Inject keyframe animation for dirty flash effect
if (typeof document !== 'undefined') {
  const styleId = 'param-cell-animations';
  if (!document.getElementById(styleId)) {
    const style = document.createElement('style');
    style.id = styleId;
    style.textContent = `
      @keyframes dirtyFlash {
        from { background-color: rgba(16, 185, 129, 0.1); }
        to { background-color: transparent; }
      }
    `;
    document.head.appendChild(style);
  }
}

type Direction = 'up' | 'down' | 'left' | 'right';
type DensityMode = 'dense' | 'relaxed';

export type ParamCellBaseProps = {
  value: string;
  paramDef: ParamDef;
  dirty: boolean;
  error?: string;
  saving: boolean;
  onChange: (value: string) => void;
  onBlur?: () => void;
  onFocus?: () => void;
  onSave?: () => void;
  onNavigate?: (direction: Direction) => void;
  dataAttrs?: Record<string, string | number | undefined>;
  density?: DensityMode;
};

type NumericProps = ParamCellBaseProps & {
  min?: number;
  max?: number;
  step?: number;
  inputType?: 'number' | 'text';
};

type SelectProps = ParamCellBaseProps & {
  options: Array<[string, string]>;
};

type ToggleProps = ParamCellBaseProps & {
  options?: Array<[string, string]>;
};

const NUMBER_WITH_DECIMAL_CHARS = /^[+-]?(?:\d+\.?\d*|\.\d*)$/;

function normalizeNumericInput(value: string): string {
  return value.replace(/,/g, '');
}

function formatNumericDisplay(value: string): string {
  const trimmed = value.trim();
  if (!trimmed) return '';

  const normalized = normalizeNumericInput(trimmed);
  if (!NUMBER_WITH_DECIMAL_CHARS.test(normalized)) {
    return value;
  }

  const sign = normalized.startsWith('-') ? '-' : '';
  const unsigned = sign ? normalized.slice(1) : normalized;
  const [rawInt, rawFrac = ''] = unsigned.split('.', 2);
  if (!/^\d*$/.test(rawInt) || !/^\d*$/.test(rawFrac)) {
    return value;
  }

  const intPart = rawInt === '' ? '0' : Number(rawInt).toLocaleString();
  if (!normalized.includes('.')) {
    return `${sign}${intPart}`;
  }

  return `${sign}${intPart}.${rawFrac}`;
}

const navKeyMap: Record<string, Direction> = {
  ArrowUp: 'up',
  ArrowDown: 'down',
  ArrowLeft: 'left',
  ArrowRight: 'right'
};

// Revised styling constants using new system
const focusRingBase = 'outline-none ring-0 focus:border-emerald-500/50 focus:ring-1 focus:ring-emerald-500/20';
const dirtyRing = 'border-amber-500/50 ring-1 ring-amber-500/20';
const errorRing = 'border-red-500/50 ring-1 ring-red-500/20';

// Base input styles - clean, minimal, dark
const inputBase =
  'w-full rounded bg-zinc-900/50 border border-zinc-800 hover:bg-zinc-900 font-mono tabular-nums text-zinc-100 transition-all duration-150 placeholder:text-zinc-700';
const selectBase =
  'w-full rounded bg-zinc-900/50 border border-zinc-800 hover:bg-zinc-900 font-mono tabular-nums text-zinc-100 transition-all duration-150 appearance-none';
const toggleBase =
  'inline-flex min-w-[76px] items-center rounded bg-zinc-900/50 border border-zinc-800 hover:bg-zinc-900 transition-colors duration-150 overflow-hidden';

const densityInputClasses: Record<DensityMode, string> = {
  dense: 'h-6 px-1.5 text-[11px]',
  relaxed: 'h-7 px-2 text-[12px]',
};

const densitySelectClasses: Record<DensityMode, string> = {
  dense: 'h-6 px-1.5 text-[11px]',
  relaxed: 'h-7 px-2 text-[12px]',
};

const densityToggleClasses: Record<DensityMode, string> = {
  dense: 'h-6 text-[11px]',
  relaxed: 'h-7 text-[12px]',
};

function useSyncedValue(value: string, syncFromParent = true): [string, (val: string) => void] {
  const [local, setLocal] = useState(value);
  useEffect(() => {
    if (syncFromParent) {
      setLocal(value);
    }
  }, [value, syncFromParent]);
  return [local, setLocal];
}

function mergeDataAttrs(dataAttrs?: Record<string, string | number | undefined>) {
  if (!dataAttrs) return undefined;
  const entries = Object.entries(dataAttrs).filter(([, v]) => v !== undefined);
  return Object.fromEntries(entries);
}

function applyDirectionalNavigation<T extends HTMLInputElement | HTMLSelectElement>(
  event: KeyboardEvent<T>,
  ref: RefObject<T>,
  onNavigate?: (direction: Direction) => void
) {
  if (!onNavigate) return;
  const direction = navKeyMap[event.key];
  if (!direction) return;
  if (event.metaKey || event.ctrlKey || event.altKey) return;

  const target = ref.current;

  if (!target) return;

  if (direction === 'left' || direction === 'right') {
    if ('selectionStart' in target && 'selectionEnd' in target) {
      const selStart = target.selectionStart ?? 0;
      const selEnd = target.selectionEnd ?? 0;
      if (selEnd - selStart > 0) return; // don't hijack while selecting text
      if (direction === 'left' && selStart > 0) return;
      if (direction === 'right' && selStart < (target.value?.length ?? 0)) return;
    }
  }

  if (target instanceof HTMLSelectElement) {
    return;
  }

  event.preventDefault();
  onNavigate(direction);
}

function composeStateClasses(dirty: boolean, error?: string, saving?: boolean) {
  let classes = focusRingBase;
  if (error) {
    classes += ` ${errorRing}`;
  } else if (dirty) {
    classes += ` ${dirtyRing} animate-[dirtyFlash_.6s_ease-out]`;
  }
  if (saving) {
    classes += ' pr-6 opacity-70 cursor-progress';
  }
  return classes;
}

function composeWrapperClasses(dirty: boolean, error?: string) {
  let classes = 'relative group';
  if (dirty && !error) {
    classes += ' after:absolute after:left-0 after:top-1 after:bottom-1 after:w-[2px] after:bg-amber-400/80 after:rounded-r';
  }
  return classes;
}

function ErrorPill({ message, id }: { message: string; id: string }) {
  return (
    <div
      id={id}
      className="absolute top-[110%] left-0 z-50 whitespace-nowrap rounded bg-red-950 border border-red-800 px-2 py-1 text-[10px] font-medium text-red-200 shadow-xl"
      role="alert"
    >
      {message}
    </div>
  );
}

function Spinner() {
  return (
    <div className="absolute right-1.5 top-1/2 h-3 w-3 -translate-y-1/2">
      <div className="h-3 w-3 animate-spin rounded-full border-[2px] border-zinc-700 border-t-zinc-400" />
    </div>
  );
}

// Removed standard DirtyBeacon in favor of the subtle border indicator in composeWrapperClasses

export const ParamCellNumeric = memo(function ParamCellNumeric({
  value,
  paramDef,
  dirty,
  error,
  saving,
  onChange,
  onBlur,
  onFocus,
  onSave,
  onNavigate,
  dataAttrs,
  min,
  max,
  step,
  inputType,
  density = 'relaxed'
}: NumericProps) {
  const [isFocused, setIsFocused] = useState(false);
  const [localValue, setLocalValue] = useSyncedValue(value, !isFocused);
  const inputRef = useRef<HTMLInputElement>(null);
  const isNumeric = paramDef.type === 'int' || paramDef.type === 'float';

  const type = inputType ?? (paramDef.type === 'int' || paramDef.type === 'float' ? 'number' : 'text');

  const handleChange = (val: string) => {
    const normalized = isNumeric ? normalizeNumericInput(val) : val;
    setLocalValue(normalized);
    onChange(normalized);
  };

  const handleFocus = () => {
    setIsFocused(true);
    onFocus?.();
  };

  const handleBlur = () => {
    setIsFocused(false);
    if (isNumeric) {
      const normalized = normalizeNumericInput(localValue);
      if (normalized !== localValue) {
        setLocalValue(normalized);
        onChange(normalized);
      }
    }
    onBlur?.();
  };

  const handleKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if ((event.metaKey || event.ctrlKey) && event.key === 'Enter') {
      return;
    }
    if (event.key === 'Enter') {
      event.preventDefault();
      if (!error && onSave) onSave();
      return;
    }
    if (event.key === 'Escape') {
      event.preventDefault();
      handleChange(value);
      inputRef.current?.blur();
      return;
    }
    applyDirectionalNavigation(event, inputRef, onNavigate);
  };

  const stateClasses = composeStateClasses(dirty, error, saving);
  const wrapperClasses = composeWrapperClasses(dirty, error);
  const dataProps = mergeDataAttrs(dataAttrs);
  const stepValue = step ?? (paramDef.type === 'int' ? 1 : paramDef.type === 'float' ? 0.01 : undefined);
  const displayValue = isFocused ? localValue : (isNumeric ? formatNumericDisplay(localValue) : localValue);

  return (
    <div className={wrapperClasses}>
      <input
        ref={inputRef}
        type={isNumeric ? 'text' : type}
        inputMode={isNumeric ? 'decimal' : undefined}
        value={displayValue}
        onChange={(event) => handleChange(event.target.value)}
        onFocus={handleFocus}
        onBlur={handleBlur}
        onKeyDown={handleKeyDown}
        min={min}
        max={max}
        step={stepValue}
        className={`${inputBase} ${densityInputClasses[density]} ${stateClasses} text-right pr-2`}
        aria-label={paramDef.label}
        aria-invalid={Boolean(error)}
        aria-describedby={error ? `${paramDef.key}-error` : undefined}
        disabled={saving}
        {...(dataProps as Record<string, any>)}
      />
      {saving && <Spinner />}
      {error && <ErrorPill id={`${paramDef.key}-error`} message={error} />}
    </div>
  );
});

export const ParamCellSelect = memo(function ParamCellSelect({
  value,
  paramDef,
  dirty,
  error,
  saving,
  onChange,
  onBlur,
  onFocus,
  onNavigate,
  onSave,
  dataAttrs,
  options,
  density = 'relaxed'
}: SelectProps) {
  const [localValue, setLocalValue] = useSyncedValue(value);
  const selectRef = useRef<HTMLSelectElement>(null);

  const handleChange = (val: string) => {
    setLocalValue(val);
    onChange(val);
  };

  const handleKeyDown = (event: KeyboardEvent<HTMLSelectElement>) => {
    if ((event.metaKey || event.ctrlKey) && event.key === 'Enter') {
      return;
    }
    if (event.key === 'Enter' && onSave && !error) {
      event.preventDefault();
      onSave();
      return;
    }
    if (event.key === 'Escape') {
      event.preventDefault();
      handleChange(value);
      selectRef.current?.blur();
      return;
    }
    if (event.key === 'Tab') return;
    applyDirectionalNavigation(event, selectRef, onNavigate);
  };

  const stateClasses = composeStateClasses(dirty, error, saving);
  const wrapperClasses = composeWrapperClasses(dirty, error);
  const dataProps = mergeDataAttrs(dataAttrs);

  return (
    <div className={wrapperClasses}>
      <select
        ref={selectRef}
        value={localValue}
        onChange={(event) => handleChange(event.target.value)}
        onFocus={onFocus}
        onBlur={onBlur}
        onKeyDown={handleKeyDown}
        className={`${selectBase} ${densitySelectClasses[density]} ${stateClasses} px-2`}
        aria-label={paramDef.label}
        aria-invalid={Boolean(error)}
        aria-describedby={error ? `${paramDef.key}-error` : undefined}
        disabled={saving}
        {...(dataProps as Record<string, any>)}
      >
        {options.map(([optValue, optLabel]) => (
          <option key={optValue} value={optValue}>
            {optLabel}
          </option>
        ))}
      </select>
      {saving && <Spinner />}
      {error && <ErrorPill id={`${paramDef.key}-error`} message={error} />}
    </div>
  );
});

const DEFAULT_TOGGLE_OPTIONS: Array<[string, string]> = [
  ['1', 'On'],
  ['0', 'Off']
];

const BOT_ON_TOGGLE_OPTIONS: Array<[string, string]> = [
  ['1', 'Enabled'],
  ['0', 'Paused'],
];

export const ParamCellToggle = memo(function ParamCellToggle({
  value,
  paramDef,
  dirty,
  error,
  saving,
  onChange,
  onBlur,
  onFocus,
  onSave,
  onNavigate,
  dataAttrs,
  options,
  density = 'relaxed'
}: ToggleProps) {
  const safeOptions = useMemo(() => {
    if (options) return options;
    if (paramDef.options) return paramDef.options;
    if (paramDef.key === 'bot_on') return BOT_ON_TOGGLE_OPTIONS;
    return DEFAULT_TOGGLE_OPTIONS;
  }, [options, paramDef.key, paramDef.options]);
  const [localValue, setLocalValue] = useSyncedValue(value);
  const containerRef = useRef<HTMLDivElement>(null);

  const handleSelect = (nextValue: string) => {
    setLocalValue(nextValue);
    onChange(nextValue);
    if (onSave && !error) {
      onSave();
    }
  };

  const handleKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if ((event.metaKey || event.ctrlKey) && event.key === 'Enter') {
      return;
    }
    const direction = navKeyMap[event.key];
    if (direction === 'left' || direction === 'right') {
      event.preventDefault();
      const currentIdx = safeOptions.findIndex(([opt]) => opt === localValue);
      const delta = direction === 'left' ? -1 : 1;
      let nextIdx = (currentIdx + delta + safeOptions.length) % safeOptions.length;
      handleSelect(safeOptions[nextIdx]?.[0] ?? localValue);
      return;
    }
    if (direction && onNavigate) {
      event.preventDefault();
      onNavigate(direction);
      return;
    }
    if (event.key === 'Escape') {
      event.preventDefault();
      setLocalValue(value);
      onBlur?.();
    }
    if (event.key === 'Enter' && onSave && !error) {
      event.preventDefault();
      onSave();
    }
  };

  const invalidOption = value !== '' && !safeOptions.some(([opt]) => opt === value);
  const fallbackError = invalidOption ? `Invalid option "${value}"` : undefined;
  const effectiveError = error ?? fallbackError;

  const stateClasses = composeStateClasses(dirty, effectiveError, saving);
  const dataProps = mergeDataAttrs(dataAttrs);

  return (
    <div
      ref={containerRef}
      tabIndex={0}
      className={`${toggleBase} ${densityToggleClasses[density]} ${stateClasses} relative`}
      onKeyDown={handleKeyDown}
      onFocus={onFocus}
      onBlur={onBlur}
      role="group"
      aria-label={paramDef.label}
      aria-invalid={Boolean(effectiveError)}
      {...(dataProps as Record<string, any>)}
    >
      {safeOptions.map(([optValue, optLabel]) => {
        const active = optValue === localValue;
        return (
          <button
            key={optValue}
            type="button"
            onClick={() => handleSelect(optValue)}
            className={`flex-1 h-full px-1.5 text-[11px] font-medium transition-all duration-150 ${
              active
                ? 'bg-emerald-500/10 text-emerald-400'
                : 'text-zinc-500 hover:text-zinc-300 hover:bg-zinc-800/50'
            }`}
          >
            {optLabel}
          </button>
        );
      })}
      {/* Vertical divider */}
      <div className="absolute top-1 bottom-1 left-1/2 w-px bg-zinc-800 pointer-events-none" />

      {dirty && !effectiveError && !saving && (
         <div className="absolute top-1 right-1 w-1 h-1 rounded-full bg-amber-400" />
      )}
      {effectiveError && <ErrorPill id={`${paramDef.key}-error`} message={effectiveError} />}
    </div>
  );
});

export type ParamCellProps = ParamCellBaseProps & {
  min?: number;
  max?: number;
  step?: number;
  options?: Array<[string, string]>;
};

const ParamCell = memo(function ParamCell(props: ParamCellProps) {
  const { paramDef } = props;
  if (paramDef.type === 'bool') {
    return <ParamCellToggle {...props} />;
  }
  if (paramDef.type === 'select') {
    const options = props.options ?? paramDef.options ?? [];
    return <ParamCellSelect {...props} options={options} />;
  }
  return <ParamCellNumeric {...props} />;
});

export default ParamCell;
