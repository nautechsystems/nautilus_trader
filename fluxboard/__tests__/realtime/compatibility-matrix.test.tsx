import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { STANDARD_CONTRACT_VERSION, RealtimeSurfaceState } from '@/lib/realtime/types';

// Task 5 validates the deterministic compatibility matrix contract for mixed rollout states.
// This suite models the expected decision table against current flag semantics and contract
// identifiers; it does not replace later runtime/surface-specific withdrawal handling tests.

type SurfaceMode = 'legacy' | 'standard' | RealtimeSurfaceState.MANUAL_REFRESH_REQUIRED;
type RecoveryReason =
  | 'backend_kill_switch'
  | 'canary_denied'
  | 'capability_withdrawn'
  | 'trade_gap'
  | 'unsupported_contract_version';
type SurfaceName = 'signal' | 'trades' | 'alerts';

type SurfaceCapabilities = {
  recoveryMode: 'invalidate_only';
  transportMode: 'polling_only';
  replaySupported: false;
};

type SurfaceCompatibility = {
  mode: SurfaceMode;
  contractVersion?: number;
  reason?: RecoveryReason;
  capabilities?: SurfaceCapabilities;
};

type CompatibilityScenario = {
  signal: SurfaceCompatibility;
  trades: SurfaceCompatibility;
  alerts: SurfaceCompatibility;
  legacySignal?: SurfaceCompatibility;
  legacyAlerts?: SurfaceCompatibility;
};

type CompatibilityMatrix = Record<string, CompatibilityScenario>;
type FrontendScenario = {
  client: 'old-fe' | 'new-fe';
  flags?: Partial<Record<RealtimeFlagName, boolean>>;
};
type BackendScenario = {
  contractVersion: number;
  subscribeDenied?: Partial<Record<SurfaceName, Exclude<RecoveryReason, 'capability_withdrawn' | 'trade_gap'>>>;
  activeWithdrawal?: Partial<Record<SurfaceName, Extract<RecoveryReason, 'backend_kill_switch' | 'capability_withdrawn'>>>;
  recoveryRequired?: Partial<Record<SurfaceName, Extract<RecoveryReason, 'trade_gap'>>>;
};
type ScenarioInput = {
  frontend: FrontendScenario;
  backend: BackendScenario;
  legacySignal?: boolean;
  legacyAlerts?: boolean;
};

const REALTIME_FLAG_KEYS = {
  global: 'fluxboard:feature:realtime-standard',
  signal: 'fluxboard:feature:realtime-standard-signal',
  trades: 'fluxboard:feature:realtime-standard-trades',
  alerts: 'fluxboard:feature:realtime-standard-alerts',
  killSwitch: 'fluxboard:feature:realtime-standard-kill-switch',
} as const;

type RealtimeFlagName = keyof typeof REALTIME_FLAG_KEYS;

function setRealtimeFlags(flags: Partial<Record<RealtimeFlagName, boolean>>): void {
  localStorage.clear();
  for (const [name, enabled] of Object.entries(flags) as Array<[RealtimeFlagName, boolean | undefined]>) {
    if (!enabled) {
      continue;
    }
    localStorage.setItem(REALTIME_FLAG_KEYS[name], '1');
  }
}

type FeatureFlagsModule = Awaited<ReturnType<typeof loadFeatureFlagsModule>>;

function standardSurface(): SurfaceCompatibility {
  return {
    mode: 'standard',
    contractVersion: STANDARD_CONTRACT_VERSION,
    capabilities: {
      // Live updates still arrive through the standard Socket.IO contract; these
      // capability fields describe the recovery path, which remains polling-only
      // and replay-free today.
      recoveryMode: 'invalidate_only',
      transportMode: 'polling_only',
      replaySupported: false,
    },
  };
}

function manualRefresh(reason: RecoveryReason): SurfaceCompatibility {
  return {
    mode: RealtimeSurfaceState.MANUAL_REFRESH_REQUIRED,
    reason,
  };
}

async function loadFeatureFlagsModule(
  flags: Partial<Record<RealtimeFlagName, boolean>>,
): Promise<typeof import('@/config/featureFlags')> {
  setRealtimeFlags(flags);
  vi.resetModules();
  return import('@/config/featureFlags');
}

function resolveSurfaceCompatibility(
  surface: SurfaceName,
  featureFlagsModule: FeatureFlagsModule | null,
  frontend: FrontendScenario,
  backend: BackendScenario,
): SurfaceCompatibility {
  if (frontend.client === 'old-fe' || featureFlagsModule === null) {
    return { mode: 'legacy' };
  }

  if (!featureFlagsModule.isRealtimeStandardEnabled(surface)) {
    return { mode: 'legacy' };
  }

  if (backend.contractVersion !== STANDARD_CONTRACT_VERSION) {
    return manualRefresh('unsupported_contract_version');
  }

  const subscribeDenied = backend.subscribeDenied?.[surface];
  if (subscribeDenied) {
    return manualRefresh(subscribeDenied);
  }

  const activeWithdrawal = backend.activeWithdrawal?.[surface];
  if (activeWithdrawal) {
    return manualRefresh(activeWithdrawal);
  }

  const recoveryRequired = backend.recoveryRequired?.[surface];
  if (recoveryRequired) {
    return manualRefresh(recoveryRequired);
  }

  return standardSurface();
}

async function resolveScenario(input: ScenarioInput): Promise<CompatibilityScenario> {
  const featureFlagsModule = input.frontend.client === 'new-fe'
    ? await loadFeatureFlagsModule(input.frontend.flags ?? {})
    : null;

  return {
    signal: resolveSurfaceCompatibility('signal', featureFlagsModule, input.frontend, input.backend),
    trades: resolveSurfaceCompatibility('trades', featureFlagsModule, input.frontend, input.backend),
    alerts: resolveSurfaceCompatibility('alerts', featureFlagsModule, input.frontend, input.backend),
    legacySignal: input.legacySignal ? { mode: 'legacy' } : undefined,
    legacyAlerts: input.legacyAlerts ? { mode: 'legacy' } : undefined,
  };
}

const SCENARIOS: Array<[string, ScenarioInput]> = [
  [
    'old-fe:old-be',
    {
      frontend: { client: 'old-fe' },
      backend: { contractVersion: 1 },
    },
  ],
  [
    'old-fe:new-be',
    {
      frontend: { client: 'old-fe' },
      backend: { contractVersion: STANDARD_CONTRACT_VERSION },
    },
  ],
  [
    'new-fe-flag-off:new-be',
    {
      frontend: { client: 'new-fe', flags: {} },
      backend: { contractVersion: STANDARD_CONTRACT_VERSION },
    },
  ],
  [
    'new-fe-flag-on:new-be',
    {
      frontend: { client: 'new-fe', flags: { global: true, signal: true } },
      backend: { contractVersion: STANDARD_CONTRACT_VERSION },
    },
  ],
  [
    'new-fe-flag-on:old-be',
    {
      frontend: { client: 'new-fe', flags: { global: true, signal: true } },
      backend: { contractVersion: 1 },
    },
  ],
  [
    'backend-kill-switch:new-be',
    {
      frontend: { client: 'new-fe', flags: { global: true, signal: true } },
      backend: {
        contractVersion: STANDARD_CONTRACT_VERSION,
        subscribeDenied: { signal: 'backend_kill_switch' },
      },
      legacySignal: true,
    },
  ],
  [
    'backend-kill-switch-after-subscribe:new-be',
    {
      frontend: { client: 'new-fe', flags: { global: true, signal: true } },
      backend: {
        contractVersion: STANDARD_CONTRACT_VERSION,
        activeWithdrawal: { signal: 'backend_kill_switch' },
      },
    },
  ],
  [
    'backend-canary-deny:new-be',
    {
      frontend: { client: 'new-fe', flags: { global: true, signal: true } },
      backend: {
        contractVersion: STANDARD_CONTRACT_VERSION,
        subscribeDenied: { signal: 'canary_denied' },
      },
      legacyAlerts: true,
    },
  ],
  [
    'capability-withdrawn:new-be',
    {
      frontend: { client: 'new-fe', flags: { global: true, signal: true } },
      backend: {
        contractVersion: STANDARD_CONTRACT_VERSION,
        activeWithdrawal: { signal: 'capability_withdrawn' },
      },
    },
  ],
  [
    'trade-gap-recovery-required:new-be',
    {
      frontend: { client: 'new-fe', flags: { global: true, trades: true } },
      backend: {
        contractVersion: STANDARD_CONTRACT_VERSION,
        recoveryRequired: { trades: 'trade_gap' },
      },
    },
  ],
  [
    'rollback-new-be-to-legacy',
    {
      frontend: { client: 'new-fe', flags: {} },
      backend: { contractVersion: STANDARD_CONTRACT_VERSION },
    },
  ],
];

async function exerciseCompatibilityMatrix(): Promise<CompatibilityMatrix> {
  const entries: Array<readonly [string, CompatibilityScenario]> = [];
  for (const [scenarioName, input] of SCENARIOS) {
    entries.push([scenarioName, await resolveScenario(input)] as const);
  }
  return Object.fromEntries(entries);
}

describe('realtime compatibility matrix contract', () => {
  beforeEach(() => {
    localStorage.clear();
    vi.resetModules();
  });

  afterEach(() => {
    localStorage.clear();
    vi.resetModules();
  });

  it('keeps old clients and flag-off rollout states on legacy paths in the matrix', async () => {
    const matrix = await exerciseCompatibilityMatrix();

    expect(matrix['old-fe:old-be']).toMatchObject({
      signal: { mode: 'legacy' },
      trades: { mode: 'legacy' },
      alerts: { mode: 'legacy' },
    });
    expect(matrix['old-fe:new-be']).toMatchObject({
      signal: { mode: 'legacy' },
      trades: { mode: 'legacy' },
      alerts: { mode: 'legacy' },
    });
    expect(matrix['new-fe-flag-off:new-be']).toMatchObject({
      signal: { mode: 'legacy' },
      trades: { mode: 'legacy' },
      alerts: { mode: 'legacy' },
    });
  });

  it('routes only flagged surfaces to the standard contract with invalidate-only recovery capabilities in the matrix', async () => {
    const matrix = await exerciseCompatibilityMatrix();

    expect(matrix['new-fe-flag-on:new-be']).toMatchObject({
      signal: {
        mode: 'standard',
        contractVersion: STANDARD_CONTRACT_VERSION,
        capabilities: {
          recoveryMode: 'invalidate_only',
          transportMode: 'polling_only',
          replaySupported: false,
        },
      },
      trades: { mode: 'legacy' },
      alerts: { mode: 'legacy' },
    });
  });

  it('resolves unsupported contracts and backend gating explicitly in the matrix while leaving legacy clients healthy', async () => {
    const matrix = await exerciseCompatibilityMatrix();

    expect(matrix['new-fe-flag-on:old-be']).toMatchObject({
      signal: {
        mode: 'manual_refresh_required',
        reason: 'unsupported_contract_version',
      },
    });
    expect(matrix['backend-kill-switch:new-be']).toMatchObject({
      signal: {
        mode: 'manual_refresh_required',
        reason: 'backend_kill_switch',
      },
      legacySignal: { mode: 'legacy' },
    });
    expect(matrix['backend-kill-switch-after-subscribe:new-be']).toMatchObject({
      signal: {
        mode: 'manual_refresh_required',
        reason: 'backend_kill_switch',
      },
    });
    expect(matrix['backend-canary-deny:new-be']).toMatchObject({
      signal: {
        mode: 'manual_refresh_required',
        reason: 'canary_denied',
      },
      legacyAlerts: { mode: 'legacy' },
    });
  });

  it('models active withdrawal and trade-gap recovery with deterministic reasons in the matrix', async () => {
    const matrix = await exerciseCompatibilityMatrix();

    expect(matrix['capability-withdrawn:new-be']).toMatchObject({
      signal: {
        mode: 'manual_refresh_required',
        reason: 'capability_withdrawn',
      },
    });
    expect(matrix['trade-gap-recovery-required:new-be']).toMatchObject({
      trades: {
        mode: 'manual_refresh_required',
        reason: 'trade_gap',
      },
    });
  });

  it('resolves rollback-to-legacy as legacy mode in the matrix', async () => {
    const matrix = await exerciseCompatibilityMatrix();

    expect(matrix['rollback-new-be-to-legacy']).toMatchObject({
      signal: { mode: 'legacy' },
      trades: { mode: 'legacy' },
      alerts: { mode: 'legacy' },
    });
  });
});
