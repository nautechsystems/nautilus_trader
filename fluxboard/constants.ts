import { INTERVALS as TOKEN_INTERVALS } from './lib/tokens';

// Application-wide constants

/**
 * Store row limits to prevent memory bloat
 */
export const STORE_LIMITS = {
  TRADES: 5000,         // Max trades to keep in memory
  BALANCES: 2000,       // Max balance entries
  SIGNAL: 1000,         // Max signal strategies
  ALERTS: 1000,         // Max alerts to keep in memory
} as const;

/**
 * Polling and refresh intervals (milliseconds)
 */
export const INTERVALS = {
  ...TOKEN_INTERVALS,
  BALANCES_POLL: TOKEN_INTERVALS.SLOW,       // Balance refresh + PanelHeader source (5s)
  FX_DEFAULT: TOKEN_INTERVALS.FAST,          // FX dashboard base polling interval (2s)
  FX_MIN: TOKEN_INTERVALS.CRITICAL,          // FX minimum interval/backoff floor (1s)
  FX_BACKOFF_MAX: TOKEN_INTERVALS.MANUAL,    // FX exponential backoff upper bound (10s)
  PARAMS_POLL: TOKEN_INTERVALS.MANUAL,       // Params auto-refresh + dashboard summary (10s)
  ALERTS_POLL: TOKEN_INTERVALS.NORMAL,       // Alerts poll + fallback refresh cadence (3s)
  HEDGER_POLL: TOKEN_INTERVALS.CRITICAL,     // Hedger heartbeat/status sampling (1s)
} as const;

/**
 * API and network constants
 */
export const API = {
  REQUEST_TIMEOUT: 30000,  // 30s timeout for API requests
  RETRY_ATTEMPTS: 3,       // Number of retry attempts
  RETRY_DELAY: 1000,       // Base retry delay (milliseconds)
} as const;

/**
 * UI constants
 */
export const UI = {
  NAV_HEIGHT: 56,          // Navigation bar height in pixels
  TOAST_DURATION: 4000,    // Default toast notification duration
  DEBOUNCE_DELAY: 300,     // Default debounce delay for inputs
} as const;

/**
 * Alert auto-dismiss timeouts (milliseconds)
 */
export const ALERT_AUTO_DISMISS = {
  INFO: 10000,             // 10s for informational alerts
  WARNING: 30000,          // 30s for warning alerts
  ERROR: 0,                // Never auto-dismiss actionable errors
  CRITICAL: 0,             // Never auto-dismiss critical alerts
} as const;

/**
 * Sound notification settings
 */
export const SOUND = {
  TRADE_CLICK_THROTTLE_MS: 100,  // Min interval between trade sounds (prevent spam)
  TRADE_CLICK_VOLUME: 0.15,      // Click volume (15% of max)
  STORAGE_KEY: 'fluxboard:sound:muted',  // LocalStorage key for mute preference
} as const;
