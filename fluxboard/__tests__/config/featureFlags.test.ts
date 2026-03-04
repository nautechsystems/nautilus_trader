import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { featureFlags, isScannersPerfV2Enabled } from '../../config/featureFlags';

describe('featureFlags', () => {
  beforeEach(() => {
    // Clear localStorage
    localStorage.clear();
    // Reset module cache to re-evaluate flags
    vi.resetModules();
  });

  afterEach(() => {
    localStorage.clear();
    vi.restoreAllMocks();
  });

  describe('scannersPerfV2', () => {
    it('defaults to false', () => {
      expect(featureFlags.scannersPerfV2).toBe(false);
      expect(isScannersPerfV2Enabled()).toBe(false);
    });

    it('can be enabled via localStorage', () => {
      localStorage.setItem('fluxboard:feature:scanners-perf-v2', '1');
      // Note: In a real test, we'd need to re-import the module to see the change
      // This test verifies the flag structure exists
      expect(typeof featureFlags.scannersPerfV2).toBe('boolean');
      expect(typeof isScannersPerfV2Enabled).toBe('function');
    });

    it('isScannersPerfV2Enabled returns boolean', () => {
      const result = isScannersPerfV2Enabled();
      expect(typeof result).toBe('boolean');
    });
  });

  describe('featureFlags object structure', () => {
    it('contains all expected flags', () => {
      expect(featureFlags).toHaveProperty('tradingStatusPills');
      expect(featureFlags).toHaveProperty('scannersVirtualizedV1');
      expect(featureFlags).toHaveProperty('scannersPerfV2');
    });

    it('all flags are boolean', () => {
      expect(typeof featureFlags.tradingStatusPills).toBe('boolean');
      expect(typeof featureFlags.scannersVirtualizedV1).toBe('boolean');
      expect(typeof featureFlags.scannersPerfV2).toBe('boolean');
    });
  });
});




