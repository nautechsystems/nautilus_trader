import type { StrategyRunState, StrategyStatus } from '@/types';

export type TradingFlagInput = string | number | boolean | null | undefined;

export type StrategyStatusInput = {
  running?: boolean | null;
  trading?: TradingFlagInput;
  coolingDown?: boolean;
};

export type TradingFilterValue = 'Live' | 'Pending' | 'Paused';
export type TradingStatusVariant = 'live' | 'pending' | 'inactive';
export type RunStatusVariant = 'live' | 'pending' | 'inactive';

const TRUE_STRINGS = new Set(['1', 'true', 'on', 'yes', 'enabled', 'live']);

function normalize(value: TradingFlagInput): string | null {
  if (value === null || value === undefined) return null;
  if (typeof value === 'boolean') {
    return value ? '1' : '0';
  }
  if (typeof value === 'number') {
    if (!Number.isFinite(value)) return null;
    return value !== 0 ? '1' : '0';
  }
  if (typeof value === 'string') {
    const trimmed = value.trim();
    if (!trimmed) return null;
    return trimmed.toLowerCase();
  }
  return null;
}

export function parseTradingEnabled(value: TradingFlagInput): boolean {
  const normalized = normalize(value);
  if (!normalized) return false;
  if (TRUE_STRINGS.has(normalized)) return true;
  if (normalized === '0' || normalized === 'off' || normalized === 'false' || normalized === 'disabled') {
    return false;
  }
  return false;
}

function deriveRunState(running?: boolean | null): StrategyRunState {
  if (running === true) return 'running';
  if (running === false) return 'stopped';
  return 'unknown';
}

export function deriveStrategyStatus(input: StrategyStatusInput): StrategyStatus {
  const tradingEnabled = parseTradingEnabled(input.trading);
  const runState = deriveRunState(input.running);

  let coolingDown = Boolean(input.coolingDown);
  if (!coolingDown) {
    // When trading intent and runner state disagree, treat as pending/cooling.
    if (runState === 'running' && !tradingEnabled) {
      coolingDown = true;
    } else if (runState === 'stopped' && tradingEnabled) {
      coolingDown = true;
    }
  }

  return {
    runState,
    tradingEnabled,
    coolingDown: coolingDown || undefined
  };
}

export function statusToFilterValue(status: StrategyStatus): TradingFilterValue {
  if (status.coolingDown) return 'Pending';
  return status.tradingEnabled ? 'Live' : 'Paused';
}

export function tradingFilterLabel(value: TradingFilterValue): string {
  return value;
}

export const TRADING_FILTER_VALUES: TradingFilterValue[] = ['Live', 'Pending', 'Paused'];

export function describeTradingStatus(status: StrategyStatus): {
  variant: TradingStatusVariant;
  label: TradingFilterValue;
  subLabel: string;
} {
  const variant: TradingStatusVariant = status.coolingDown
    ? 'pending'
    : status.tradingEnabled
      ? 'live'
      : 'inactive';

  let label: TradingFilterValue;
  let subLabel: string;
  if (variant === 'live') {
    label = 'Live';
    subLabel = 'Enabled';
  } else if (variant === 'pending') {
    label = 'Pending';
    subLabel = status.tradingEnabled ? 'Syncing' : 'Cooling';
  } else {
    label = 'Paused';
    subLabel = 'Disabled';
  }

  return { variant, label, subLabel };
}

export function describeRunState(runState: StrategyRunState): {
  variant: RunStatusVariant;
  label: string;
  subLabel: string;
} {
  if (runState === 'running') {
    return { variant: 'live', label: 'Running', subLabel: 'Node' };
  }
  if (runState === 'stopped') {
    return { variant: 'inactive', label: 'Stopped', subLabel: 'Node' };
  }
  return { variant: 'pending', label: 'Unknown', subLabel: 'Node' };
}
