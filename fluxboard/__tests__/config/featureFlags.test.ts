import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

async function loadFeatureFlagsModule() {
  return import('@/config/featureFlags');
}

describe('featureFlags', () => {
  beforeEach(() => {
    localStorage.clear();
    vi.unstubAllEnvs();
    vi.resetModules();
  });

  afterEach(() => {
    localStorage.clear();
    vi.unstubAllEnvs();
    vi.restoreAllMocks();
  });

  describe('scannersPerfV2', () => {
    it('defaults to false', async () => {
      const { featureFlags, isScannersPerfV2Enabled } = await loadFeatureFlagsModule();

      expect(featureFlags.scannersPerfV2).toBe(false);
      expect(isScannersPerfV2Enabled()).toBe(false);
    });

    it('can be enabled via localStorage', async () => {
      localStorage.setItem('fluxboard:feature:scanners-perf-v2', '1');

      const { featureFlags, isScannersPerfV2Enabled } = await loadFeatureFlagsModule();

      expect(featureFlags.scannersPerfV2).toBe(true);
      expect(isScannersPerfV2Enabled()).toBe(true);
    });

    it('isScannersPerfV2Enabled returns boolean', async () => {
      const { isScannersPerfV2Enabled } = await loadFeatureFlagsModule();

      expect(typeof isScannersPerfV2Enabled()).toBe('boolean');
    });
  });

  describe('realtime standard flags', () => {
    it('exposes per-surface rollout and kill-switch flags', async () => {
      const {
        REALTIME_SURFACE_FLAGS,
        isRealtimeStandardEnabled,
        isRealtimeSurfaceKillSwitched,
      } = await loadFeatureFlagsModule();

      expect(REALTIME_SURFACE_FLAGS.signal).toBeDefined();
      expect(REALTIME_SURFACE_FLAGS.trades).toBeDefined();
      expect(REALTIME_SURFACE_FLAGS.killSwitch).toBeDefined();
      expect(typeof isRealtimeStandardEnabled('signal')).toBe('boolean');
      expect(typeof isRealtimeSurfaceKillSwitched('signal')).toBe('boolean');
    });

    it('defaults the realtime standard rollout to disabled with no kill switch', async () => {
      const {
        featureFlags,
        isRealtimeStandardEnabled,
        isRealtimeSurfaceKillSwitched,
      } = await loadFeatureFlagsModule();

      expect(featureFlags.realtimeStandard.global).toBe(false);
      expect(featureFlags.realtimeStandard.signal).toBe(false);
      expect(featureFlags.realtimeStandard.killSwitch).toBe(false);
      expect(isRealtimeStandardEnabled('signal')).toBe(false);
      expect(isRealtimeSurfaceKillSwitched('signal')).toBe(false);
    });

    it('requires both global and per-surface rollout flags', async () => {
      localStorage.setItem('fluxboard:feature:realtime-standard', '1');

      const globalOnly = await loadFeatureFlagsModule();
      expect(globalOnly.isRealtimeStandardEnabled('signal')).toBe(false);

      vi.resetModules();
      localStorage.setItem('fluxboard:feature:realtime-standard-signal', '1');

      const enabled = await loadFeatureFlagsModule();
      expect(enabled.isRealtimeStandardEnabled('signal')).toBe(true);
    });

    it('lets the global kill switch disable all realtime surfaces', async () => {
      localStorage.setItem('fluxboard:feature:realtime-standard', '1');
      localStorage.setItem('fluxboard:feature:realtime-standard-signal', '1');
      localStorage.setItem('fluxboard:feature:realtime-standard-kill-switch', '1');

      const {
        featureFlags,
        isRealtimeStandardEnabled,
        isRealtimeSurfaceKillSwitched,
      } = await loadFeatureFlagsModule();

      expect(featureFlags.realtimeStandard.killSwitch).toBe(true);
      expect(isRealtimeSurfaceKillSwitched('signal')).toBe(true);
      expect(isRealtimeStandardEnabled('signal')).toBe(false);
    });

    it('defaults equities signal, balances, and trades realtime standard on for the /equities surface', async () => {
      Object.defineProperty(window, 'location', {
        value: new URL('http://localhost/equities'),
        configurable: true,
      });

      const {
        featureFlags,
        isRealtimeStandardEnabled,
      } = await loadFeatureFlagsModule();

      expect(featureFlags.realtimeStandard.global).toBe(true);
      expect(featureFlags.realtimeStandard.signal).toBe(true);
      expect(featureFlags.realtimeStandard.balances).toBe(true);
      expect(featureFlags.realtimeStandard.trades).toBe(true);
      expect(isRealtimeStandardEnabled('signal')).toBe(true);
      expect(isRealtimeStandardEnabled('balances')).toBe(true);
      expect(isRealtimeStandardEnabled('trades')).toBe(true);
    });

    it('keeps non-equities surfaces disabled by default without an explicit rollout override', async () => {
      Object.defineProperty(window, 'location', {
        value: new URL('http://localhost/tokenmm'),
        configurable: true,
      });

      const {
        featureFlags,
        isRealtimeStandardEnabled,
      } = await loadFeatureFlagsModule();

      expect(featureFlags.realtimeStandard.global).toBe(false);
      expect(featureFlags.realtimeStandard.signal).toBe(false);
      expect(featureFlags.realtimeStandard.balances).toBe(false);
      expect(featureFlags.realtimeStandard.trades).toBe(false);
      expect(isRealtimeStandardEnabled('signal')).toBe(false);
      expect(isRealtimeStandardEnabled('balances')).toBe(false);
      expect(isRealtimeStandardEnabled('trades')).toBe(false);
    });
  });

  describe('featureFlags object structure', () => {
    it('contains all expected flags', async () => {
      const { featureFlags } = await loadFeatureFlagsModule();

      expect(featureFlags).toHaveProperty('tradingStatusPills');
      expect(featureFlags).toHaveProperty('scannersVirtualizedV1');
      expect(featureFlags).toHaveProperty('scannersPerfV2');
      expect(featureFlags).toHaveProperty('realtimeStandard');
    });

    it('all flags are boolean-valued or nested boolean flag groups', async () => {
      const { featureFlags } = await loadFeatureFlagsModule();

      expect(typeof featureFlags.tradingStatusPills).toBe('boolean');
      expect(typeof featureFlags.scannersVirtualizedV1).toBe('boolean');
      expect(typeof featureFlags.scannersPerfV2).toBe('boolean');
      expect(typeof featureFlags.realtimeStandard.global).toBe('boolean');
      expect(typeof featureFlags.realtimeStandard.signal).toBe('boolean');
      expect(typeof featureFlags.realtimeStandard.killSwitch).toBe('boolean');
    });
  });
});
