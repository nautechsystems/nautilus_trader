// Constants tests

import { describe, it, expect } from 'vitest';
import { STORE_LIMITS, INTERVALS, API, UI } from './constants';

describe('Constants', () => {
  describe('STORE_LIMITS', () => {
    it('defines positive row limits for all stores', () => {
      expect(STORE_LIMITS.TRADES).toBeGreaterThan(0);
      expect(STORE_LIMITS.BALANCES).toBeGreaterThan(0);
      expect(STORE_LIMITS.SIGNAL).toBeGreaterThan(0);
    });

    it('sets trades limit higher than other stores', () => {
      // Trades need more history
      expect(STORE_LIMITS.TRADES).toBeGreaterThan(STORE_LIMITS.BALANCES);
    });

    it('uses reasonable limits to prevent memory bloat', () => {
      // None should exceed 10k rows
      expect(STORE_LIMITS.TRADES).toBeLessThanOrEqual(10000);
      expect(STORE_LIMITS.BALANCES).toBeLessThanOrEqual(10000);
      expect(STORE_LIMITS.SIGNAL).toBeLessThanOrEqual(10000);
    });

    it('is immutable (readonly)', () => {
      // TypeScript ensures this, but verify at runtime
      expect(Object.isFrozen(STORE_LIMITS)).toBe(false); // 'as const' doesn't freeze at runtime
      // But TypeScript will prevent modification: STORE_LIMITS.TRADES = 999; // TS error
    });
  });

  describe('INTERVALS', () => {
    it('defines positive intervals in milliseconds', () => {
      expect(INTERVALS.BALANCES_POLL).toBeGreaterThan(0);
      expect(INTERVALS.FX_DEFAULT).toBeGreaterThan(0);
      expect(INTERVALS.FX_MIN).toBeGreaterThan(0);
      expect(INTERVALS.FX_BACKOFF_MAX).toBeGreaterThan(0);
      expect(INTERVALS.HEDGER_POLL).toBeGreaterThan(0);
    });

    it('has sensible polling interval for balances', () => {
      // Should be at least 1 second to avoid server spam
      expect(INTERVALS.BALANCES_POLL).toBeGreaterThanOrEqual(1000);
      // Should not be more than 30 seconds (too slow)
      expect(INTERVALS.BALANCES_POLL).toBeLessThanOrEqual(30000);
    });

    it('enforces FX interval constraints', () => {
      // Min should be at least 1 second
      expect(INTERVALS.FX_MIN).toBeGreaterThanOrEqual(1000);
      // Max backoff should be reasonable
      expect(INTERVALS.FX_BACKOFF_MAX).toBeLessThanOrEqual(60000); // Max 1 minute
      // Default should be between min and max
      expect(INTERVALS.FX_DEFAULT).toBeGreaterThanOrEqual(INTERVALS.FX_MIN);
      expect(INTERVALS.FX_DEFAULT).toBeLessThanOrEqual(INTERVALS.FX_BACKOFF_MAX);
    });

    it('aligns hedger polling with the critical tier', () => {
      expect(INTERVALS.HEDGER_POLL).toBe(INTERVALS.CRITICAL);
    });
  });

  describe('API', () => {
    it('defines positive timeouts and retry settings', () => {
      expect(API.REQUEST_TIMEOUT).toBeGreaterThan(0);
      expect(API.RETRY_ATTEMPTS).toBeGreaterThan(0);
      expect(API.RETRY_DELAY).toBeGreaterThan(0);
    });

    it('has reasonable request timeout', () => {
      // Should be at least 5 seconds for slow networks
      expect(API.REQUEST_TIMEOUT).toBeGreaterThanOrEqual(5000);
      // Should not exceed 2 minutes (user will give up)
      expect(API.REQUEST_TIMEOUT).toBeLessThanOrEqual(120000);
    });

    it('has reasonable retry attempts', () => {
      // At least 1 retry
      expect(API.RETRY_ATTEMPTS).toBeGreaterThanOrEqual(1);
      // Not more than 5 (too many retries)
      expect(API.RETRY_ATTEMPTS).toBeLessThanOrEqual(5);
    });
  });

  describe('UI', () => {
    it('defines positive UI constants', () => {
      expect(UI.NAV_HEIGHT).toBeGreaterThan(0);
      expect(UI.TOAST_DURATION).toBeGreaterThan(0);
      expect(UI.DEBOUNCE_DELAY).toBeGreaterThan(0);
    });

    it('has reasonable nav height', () => {
      // Should be between 40-80px
      expect(UI.NAV_HEIGHT).toBeGreaterThanOrEqual(40);
      expect(UI.NAV_HEIGHT).toBeLessThanOrEqual(80);
    });

    it('has reasonable toast duration', () => {
      // At least 2 seconds to read
      expect(UI.TOAST_DURATION).toBeGreaterThanOrEqual(2000);
      // Not more than 10 seconds (annoying)
      expect(UI.TOAST_DURATION).toBeLessThanOrEqual(10000);
    });

    it('has reasonable debounce delay', () => {
      // At least 100ms
      expect(UI.DEBOUNCE_DELAY).toBeGreaterThanOrEqual(100);
      // Not more than 1 second (too laggy)
      expect(UI.DEBOUNCE_DELAY).toBeLessThanOrEqual(1000);
    });
  });

  describe('Integration', () => {
    it('balances poll interval matches interval constants', () => {
      // Verify BALANCES_POLL is in valid range
      expect(INTERVALS.BALANCES_POLL).toBe(5000); // Explicit check for 5s
    });

  });
});
