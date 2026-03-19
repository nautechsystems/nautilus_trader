export const REALTIME_STANDARD_SURFACES = [
  'signal',
  'trades',
  'alerts',
  'marketData',
  'balances',
  'scanners',
] as const;

export type RealtimeSurface = (typeof REALTIME_STANDARD_SURFACES)[number];

export const REALTIME_STANDARD_ENV_FLAGS = {
  global: 'VITE_REALTIME_STANDARD',
  signal: 'VITE_REALTIME_STANDARD_SIGNAL',
  trades: 'VITE_REALTIME_STANDARD_TRADES',
  alerts: 'VITE_REALTIME_STANDARD_ALERTS',
  marketData: 'VITE_REALTIME_STANDARD_MARKETDATA',
  balances: 'VITE_REALTIME_STANDARD_BALANCES',
  scanners: 'VITE_REALTIME_STANDARD_SCANNERS',
  killSwitch: 'VITE_REALTIME_STANDARD_KILL_SWITCH',
} as const;

export const REALTIME_STANDARD_STORAGE_FLAGS = {
  global: 'fluxboard:feature:realtime-standard',
  signal: 'fluxboard:feature:realtime-standard-signal',
  trades: 'fluxboard:feature:realtime-standard-trades',
  alerts: 'fluxboard:feature:realtime-standard-alerts',
  marketData: 'fluxboard:feature:realtime-standard-marketdata',
  balances: 'fluxboard:feature:realtime-standard-balances',
  scanners: 'fluxboard:feature:realtime-standard-scanners',
  killSwitch: 'fluxboard:feature:realtime-standard-kill-switch',
} as const;
