import type { StrategyRunState, StrategyStatus } from '@/types';

export type TradingFlagInput = string | number | boolean | null | undefined;

export type StrategyStatusInput = {
  running?: boolean | null;
  trading?: TradingFlagInput;
  blocked?: boolean;
  coolingDown?: boolean;
};

export type TradingFilterValue = 'Enabled' | 'Pending' | 'Paused';
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

  return {
    runState,
    tradingEnabled,
    blocked: input.blocked ? true : undefined,
    coolingDown: input.coolingDown ? true : undefined,
  };
}

export function statusToFilterValue(status: StrategyStatus): TradingFilterValue {
  if (status.coolingDown) return 'Pending';
  if (status.blocked && status.tradingEnabled) return 'Pending';
  if (status.tradingEnabled && status.runState !== 'running') return 'Pending';
  if (status.tradingEnabled) return 'Enabled';
  return 'Paused';
}

function runnerSubLabel(runState: StrategyRunState): string {
  if (runState === 'running') return 'Runner On';
  if (runState === 'stopped') return 'Runner Off';
  return 'Runner Unknown';
}

export function tradingFilterLabel(value: TradingFilterValue): string {
  return value;
}

export const TRADING_FILTER_VALUES: TradingFilterValue[] = ['Enabled', 'Pending', 'Paused'];

export function describeTradingStatus(status: StrategyStatus): {
  variant: TradingStatusVariant;
  label: TradingFilterValue;
  subLabel: string;
} {
  if (status.coolingDown) {
    return {
      variant: 'pending',
      label: 'Pending',
      subLabel: 'Cooling',
    };
  }
  if (status.blocked && status.tradingEnabled) {
    return {
      variant: 'pending',
      label: 'Pending',
      subLabel: runnerSubLabel(status.runState),
    };
  }
  if (status.tradingEnabled) {
    if (status.runState === 'running') {
      return {
        variant: 'live',
        label: 'Enabled',
        subLabel: 'Runner On',
      };
    }
    return {
      variant: 'pending',
      label: 'Pending',
      subLabel: runnerSubLabel(status.runState),
    };
  }
  return {
    variant: 'inactive',
    label: 'Paused',
    subLabel: runnerSubLabel(status.runState),
  };
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
