/**
 * Feature flag utilities for Fluxboard UI experiments.
 *
 * Flags read from (in order of precedence):
 *  1. Vite env variables (VITE_<FLAG_NAME>)
 *  2. LocalStorage overrides (persisted per browser)
 *  3. Default values defined below
 */

import {
  REALTIME_STANDARD_ENV_FLAGS,
  REALTIME_STANDARD_STORAGE_FLAGS,
  type RealtimeSurface,
} from '../lib/realtime/constants';
import { resolvePathnameProfile } from './uiProfiles';

type BooleanLike = string | number | boolean | null | undefined;

function toBoolean(value: BooleanLike): boolean | null {
  if (value === null || value === undefined) return null;
  if (typeof value === 'boolean') return value;
  if (typeof value === 'number') {
    if (Number.isNaN(value)) return null;
    return value !== 0;
  }
  if (typeof value === 'string') {
    const normalized = value.trim().toLowerCase();
    if (normalized === '') return null;
    if (['1', 'true', 'on', 'yes', 'enabled'].includes(normalized)) return true;
    if (['0', 'false', 'off', 'no', 'disabled'].includes(normalized)) return false;
  }
  return null;
}

function readEnvFlag(key: string): boolean | null {
  try {
    const envValue = (import.meta as any)?.env?.[key];
    return toBoolean(envValue);
  } catch {
    return null;
  }
}

function readStorageFlag(key: string): boolean | null {
  try {
    if (typeof window === 'undefined') return null;
    if (!window.localStorage) return null;
    const stored = window.localStorage.getItem(key);
    return toBoolean(stored);
  } catch {
    return null;
  }
}

function resolveBooleanFlag({
  envKey,
  storageKey,
  defaultValue
}: {
  envKey: string;
  storageKey: string;
  defaultValue: boolean;
}): boolean {
  const envOverride = readEnvFlag(envKey);
  if (envOverride !== null) {
    return envOverride;
  }

  const storageOverride = readStorageFlag(storageKey);
  if (storageOverride !== null) {
    return storageOverride;
  }

  return defaultValue;
}

const TRADING_STATUS_FLAG_ENV = 'VITE_TRADING_STATUS_PILLS';
const TRADING_STATUS_FLAG_STORAGE = 'fluxboard:feature:trading-status-pills';

const SCANNERS_VIRTUALIZED_FLAG_ENV = 'VITE_SCANNERS_VIRTUALIZED_V1';
const SCANNERS_VIRTUALIZED_FLAG_STORAGE = 'fluxboard:feature:scanners-virtualized-v1';

const SCANNERS_PERF_V2_FLAG_ENV = 'VITE_SCANNERS_PERF_V2';
const SCANNERS_PERF_V2_FLAG_STORAGE = 'fluxboard:feature:scanners-perf-v2';

const SCANNERS_OPTIMIZE_TIMERS_FLAG_ENV = 'VITE_SCANNERS_OPTIMIZE_TIMERS';
const SCANNERS_OPTIMIZE_TIMERS_FLAG_STORAGE = 'fluxboard:feature:scanners-optimize-timers';

const SCANNERS_OPTIMIZE_RAF_FLAG_ENV = 'VITE_SCANNERS_OPTIMIZE_RAF';
const SCANNERS_OPTIMIZE_RAF_FLAG_STORAGE = 'fluxboard:feature:scanners-optimize-raf';

const SCANNERS_OPTIMIZE_SUBSCRIPTIONS_FLAG_ENV = 'VITE_SCANNERS_OPTIMIZE_SUBSCRIPTIONS';
const SCANNERS_OPTIMIZE_SUBSCRIPTIONS_FLAG_STORAGE = 'fluxboard:feature:scanners-optimize-subscriptions';

const SCANNERS_MEMORY_CLEANUP_FLAG_ENV = 'VITE_SCANNERS_MEMORY_CLEANUP';
const SCANNERS_MEMORY_CLEANUP_FLAG_STORAGE = 'fluxboard:feature:scanners-memory-cleanup';

const SCANNERS_DELTA_BUFFER_LIMITS_FLAG_ENV = 'VITE_SCANNERS_DELTA_BUFFER_LIMITS';
const SCANNERS_DELTA_BUFFER_LIMITS_FLAG_STORAGE = 'fluxboard:feature:scanners-delta-buffer-limits';
const TRADES_DECISION_DETAILS_FLAG_ENV = 'VITE_TRADES_DECISION_DETAILS';
const TRADES_DECISION_DETAILS_FLAG_STORAGE = 'fluxboard:feature:trades-decision-details';
const PNL_DECISION_DETAILS_FLAG_ENV = 'VITE_PNL_DECISION_DETAILS';
const PNL_DECISION_DETAILS_FLAG_STORAGE = 'fluxboard:feature:pnl-decision-details';

export const REALTIME_SURFACE_FLAGS = REALTIME_STANDARD_STORAGE_FLAGS;

function resolveRealtimeStandardFlag(flag: keyof typeof REALTIME_STANDARD_STORAGE_FLAGS): boolean {
  return resolveBooleanFlag({
    envKey: REALTIME_STANDARD_ENV_FLAGS[flag],
    storageKey: REALTIME_STANDARD_STORAGE_FLAGS[flag],
    defaultValue: false,
  });
}

const tradingStatusPillsEnabled = resolveBooleanFlag({
  envKey: TRADING_STATUS_FLAG_ENV,
  storageKey: TRADING_STATUS_FLAG_STORAGE,
  defaultValue: true
});

const scannersVirtualizedEnabled = resolveBooleanFlag({
  envKey: SCANNERS_VIRTUALIZED_FLAG_ENV,
  storageKey: SCANNERS_VIRTUALIZED_FLAG_STORAGE,
  defaultValue: false,
});

const scannersPerfV2Enabled = resolveBooleanFlag({
  envKey: SCANNERS_PERF_V2_FLAG_ENV,
  storageKey: SCANNERS_PERF_V2_FLAG_STORAGE,
  defaultValue: false,
});

const scannersOptimizeTimersEnabled = resolveBooleanFlag({
  envKey: SCANNERS_OPTIMIZE_TIMERS_FLAG_ENV,
  storageKey: SCANNERS_OPTIMIZE_TIMERS_FLAG_STORAGE,
  defaultValue: true, // Enabled by default for immediate benefit
});

const scannersOptimizeRafEnabled = resolveBooleanFlag({
  envKey: SCANNERS_OPTIMIZE_RAF_FLAG_ENV,
  storageKey: SCANNERS_OPTIMIZE_RAF_FLAG_STORAGE,
  defaultValue: true, // Enabled by default for immediate benefit
});

const scannersOptimizeSubscriptionsEnabled = resolveBooleanFlag({
  envKey: SCANNERS_OPTIMIZE_SUBSCRIPTIONS_FLAG_ENV,
  storageKey: SCANNERS_OPTIMIZE_SUBSCRIPTIONS_FLAG_STORAGE,
  defaultValue: true, // Enabled by default for immediate benefit
});

const scannersMemoryCleanupEnabled = resolveBooleanFlag({
  envKey: SCANNERS_MEMORY_CLEANUP_FLAG_ENV,
  storageKey: SCANNERS_MEMORY_CLEANUP_FLAG_STORAGE,
  defaultValue: true, // Enabled by default for immediate benefit
});

const scannersDeltaBufferLimitsEnabled = resolveBooleanFlag({
  envKey: SCANNERS_DELTA_BUFFER_LIMITS_FLAG_ENV,
  storageKey: SCANNERS_DELTA_BUFFER_LIMITS_FLAG_STORAGE,
  defaultValue: true, // Enabled by default for immediate benefit
});
const tradesDecisionDetailsEnabled = resolveBooleanFlag({
  envKey: TRADES_DECISION_DETAILS_FLAG_ENV,
  storageKey: TRADES_DECISION_DETAILS_FLAG_STORAGE,
  defaultValue: true, // Default ON for Trades table visibility
});
const pnlDecisionDetailsEnabled = resolveBooleanFlag({
  envKey: PNL_DECISION_DETAILS_FLAG_ENV,
  storageKey: PNL_DECISION_DETAILS_FLAG_STORAGE,
  defaultValue: false, // Keep PnL decision visuals opt-in
});

const realtimeStandardFlags = {
  global: resolveRealtimeStandardFlag('global'),
  signal: resolveRealtimeStandardFlag('signal'),
  trades: resolveRealtimeStandardFlag('trades'),
  alerts: resolveRealtimeStandardFlag('alerts'),
  marketData: resolveRealtimeStandardFlag('marketData'),
  balances: resolveRealtimeStandardFlag('balances'),
  scanners: resolveRealtimeStandardFlag('scanners'),
  killSwitch: resolveRealtimeStandardFlag('killSwitch'),
} as const;

export const featureFlags = {
  tradingStatusPills: tradingStatusPillsEnabled,
  scannersVirtualizedV1: scannersVirtualizedEnabled,
  scannersPerfV2: scannersPerfV2Enabled,
  scannersOptimizeTimers: scannersOptimizeTimersEnabled,
  scannersOptimizeRaf: scannersOptimizeRafEnabled,
  scannersOptimizeSubscriptions: scannersOptimizeSubscriptionsEnabled,
  scannersMemoryCleanup: scannersMemoryCleanupEnabled,
  scannersDeltaBufferLimits: scannersDeltaBufferLimitsEnabled,
  tradesDecisionDetails: tradesDecisionDetailsEnabled,
  pnlDecisionDetails: pnlDecisionDetailsEnabled,
  realtimeStandard: realtimeStandardFlags,
} as const;

function isProfileDefaultRealtimeStandardEnabled(surface: RealtimeSurface): boolean {
  if (typeof window === 'undefined') {
    return false;
  }
  const profile = resolvePathnameProfile(window.location?.pathname);
  return surface === 'trades' && profile === 'tokenmm';
}

export function isTradingStatusPillEnabled(): boolean {
  return featureFlags.tradingStatusPills;
}

export function isScannersVirtualizedEnabled(): boolean {
  return featureFlags.scannersVirtualizedV1;
}

export function isScannersPerfV2Enabled(): boolean {
  return featureFlags.scannersPerfV2;
}

export function isScannersOptimizeTimersEnabled(): boolean {
  return featureFlags.scannersOptimizeTimers;
}

export function isScannersOptimizeRafEnabled(): boolean {
  return featureFlags.scannersOptimizeRaf;
}

export function isScannersOptimizeSubscriptionsEnabled(): boolean {
  return featureFlags.scannersOptimizeSubscriptions;
}

export function isScannersMemoryCleanupEnabled(): boolean {
  return featureFlags.scannersMemoryCleanup;
}

export function isScannersDeltaBufferLimitsEnabled(): boolean {
  return featureFlags.scannersDeltaBufferLimits;
}

export function isTradesDecisionDetailsEnabled(): boolean {
  return featureFlags.tradesDecisionDetails;
}

export function isPnlDecisionDetailsEnabled(): boolean {
  return featureFlags.pnlDecisionDetails;
}

export function isRealtimeStandardEnabled(surface: RealtimeSurface): boolean {
  return (
    (
      (
        featureFlags.realtimeStandard.global
        && featureFlags.realtimeStandard[surface]
      )
      || isProfileDefaultRealtimeStandardEnabled(surface)
    )
    && !isRealtimeSurfaceKillSwitched(surface)
  );
}

export function isRealtimeSurfaceKillSwitched(surface: RealtimeSurface): boolean {
  return Boolean(REALTIME_SURFACE_FLAGS[surface]) && featureFlags.realtimeStandard.killSwitch;
}
