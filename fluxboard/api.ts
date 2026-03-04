// REST API client for Flask backend

import { toast } from 'sonner';
import { APIClient } from './apiClient';
import { resolvePathnameProfile, type PathProfile } from './config/uiProfiles';
import { normalizeMarketRow } from './utils/marketData';
import type {
  MarketSnapshot,
  TradesResponse,
  TradeEvent,
  StrategyParams,
  FvSnapshot,
  FxDashboard,
  FxPair,
  SignalStrategy,
  BalancesPayload,
  BalancesResponse,
  Alert,
  RawStrategy,
  BalanceSummary,
  PnLReport,
  PnLDeltaResponse,
  PnLParams,
  ScannerAggregateOppsResponse,
  ScannerRegistryItem,
  ScannerOpportunity,
  ScannerPricingSnapshot,
  ScannerPricingPageInfo,
  HedgerStatus,
  HedgerGeometry,
  HedgerGeometryOverrides,
  HedgerThresholds,
  HedgerThresholdOverrides,
  HedgerInstanceMeta,
  HedgerConfig,
  SignalStrategiesPayload,
  OrderViewSnapshot,
  OrderViewLeg,
} from './types';

export type RunPnLReportOptions = {
  etag?: string | null;
};

export type RunPnLReportResult = {
  status: number;
  etag?: string | null;
  report: PnLReport | null;
};

export type PnLDeltaRequest = PnLParams & {
  known_groups?: Record<string, string>;
  known_symbols?: Record<string, string>;
  known_unhedged?: Record<string, string>;
};
export type RunPnLDeltaOptions = { etag?: string | null };

export type MarketDataSnapshotResponse = {
  rows: MarketSnapshot[];
  count: number;
  freshnessKey?: string;
  etag?: string;
  lastUpdateMs?: number;
};

export type GetOrderViewSnapshotParams = {
  strategyId: string;
  leg?: OrderViewLeg;
  includeEvents?: boolean;
  eventsLimit?: number;
  includeBook?: boolean;
  bookDepth?: number;
  candleIntervalMs?: number;
  candleRange?: '5m' | '15m' | '1h' | string;
  orderViewV02?: boolean;
};

// Base URL resolution
// Priority:
// 1) VITE_FLUXAPI_BASE_URL (absolute or relative)
// 2) When running flux standalone (dev server on non-5000 port), connect to Flask on :5000
// 3) When served by Flask on :5000 or same-origin, use relative URLs (empty base)
// 4) In test environment, default to http://localhost:5000 to prevent "Invalid URL" errors
const envBase = (typeof import.meta !== 'undefined' && (import.meta as any)?.env?.VITE_FLUXAPI_BASE_URL) as string | undefined;
const isTestEnv = typeof process !== 'undefined' && process.env?.NODE_ENV === 'test';
// Default to same-origin relative base; only override if explicitly configured.
// In tests, use localhost:5000 to prevent fetch URL errors
const base = envBase && envBase.length ? envBase : (isTestEnv ? 'http://localhost:5000' : '');

function getActivePathProfile(): PathProfile {
  if (typeof window === 'undefined') {
    return 'default';
  }
  return resolvePathnameProfile(window.location?.pathname);
}

function appendProfileQuery(qs: URLSearchParams): void {
  const profile = getActivePathProfile();
  if (profile !== 'default') qs.set('profile', profile);
}

// Create enhanced API client instance with timeout, retry, and deduplication
const apiClient = new APIClient(base);

type FluxEnvelope<T> = { ok: boolean; data: T; error?: unknown };

function isFluxEnvelope<T>(payload: unknown): payload is FluxEnvelope<T> {
  return Boolean(payload && typeof payload === 'object' && 'ok' in (payload as Record<string, unknown>));
}

function unwrapFluxEnvelope<T>(payload: T | FluxEnvelope<T>): T {
  if (isFluxEnvelope<T>(payload)) {
    if (!payload.ok) {
      const message = typeof payload.error === 'string' ? payload.error : 'fluxapi_error';
      throw new Error(message);
    }
    return payload.data;
  }
  return payload as T;
}

async function hmacSha256Hex(secret: string, message: string): Promise<string> {
  const enc = new TextEncoder();
  const data = enc.encode(message);
  // Browser WebCrypto if available
  if (typeof crypto !== 'undefined' && crypto.subtle) {
    const key = await crypto.subtle.importKey(
      'raw',
      enc.encode(secret),
      { name: 'HMAC', hash: 'SHA-256' },
      false,
      ['sign']
    );
    const sig = await crypto.subtle.sign('HMAC', key, data);
    const bytes = new Uint8Array(sig);
    return Array.from(bytes).map(b => b.toString(16).padStart(2, '0')).join('');
  }
  // Node fallback (tests)
  try {
    // eslint-disable-next-line @typescript-eslint/no-var-requires
    const nodeCrypto = require('crypto');
    return nodeCrypto.createHmac('sha256', secret).update(Buffer.from(message, 'utf8')).digest('hex');
  } catch {
    throw new Error('HMAC not available');
  }
}

async function sha256Hex(message: string): Promise<string> {
  const enc = new TextEncoder();
  const data = enc.encode(message);
  if (typeof crypto !== 'undefined' && crypto.subtle) {
    const digest = await crypto.subtle.digest('SHA-256', data);
    const bytes = new Uint8Array(digest);
    return Array.from(bytes).map(b => b.toString(16).padStart(2, '0')).join('');
  }
  try {
    // eslint-disable-next-line @typescript-eslint/no-var-requires
    const nodeCrypto = require('crypto');
    return nodeCrypto.createHash('sha256').update(Buffer.from(message, 'utf8')).digest('hex');
  } catch {
    throw new Error('SHA256 not available');
  }
}

function resolveHmacSecret(): string | null {
  try {
    if (typeof window !== 'undefined') {
      const w: any = window as any;
      if (typeof w.FLUXAPI_HMAC_SECRET === 'string' && w.FLUXAPI_HMAC_SECRET) return w.FLUXAPI_HMAC_SECRET;
      try {
        const ls = window.localStorage?.getItem('fluxapi:hmac');
        if (ls) return ls;
      } catch {}
      try {
        const cookies = document.cookie ? document.cookie.split(';').map(s => s.trim()) : [];
        for (const c of cookies) {
          if (c.startsWith('fluxapi_hmac=')) {
            return decodeURIComponent(c.split('=')[1] || '');
          }
        }
      } catch {}
    }
  } catch {}
  // Build-time fallback (use only in controlled environments)
  // @ts-ignore
  const envSecret = (import.meta as any)?.env?.VITE_FLUXAPI_HMAC_SECRET;
  return envSecret ? String(envSecret) : null;
}

type SignedHeaderContext = {
  method: string;
  path: string;
};

function normalizeRequestPath(path: string): string {
  const basePath = String(path || '/').split('?')[0] || '/';
  return basePath.startsWith('/') ? basePath : `/${basePath}`;
}

async function signedJsonHeaders(payload: any, context?: SignedHeaderContext): Promise<Record<string, string>> {
  const secret = resolveHmacSecret();
  if (!secret) return {};
  const body = payload == null ? '' : (typeof payload === 'string' ? payload : JSON.stringify(payload));
  try {
    if (!context) {
      const sig = await hmacSha256Hex(secret, body);
      return { 'X-Signature': sig };
    }
    const timestamp = String(Math.floor(Date.now() / 1000));
    const method = context.method.toUpperCase();
    const path = normalizeRequestPath(context.path);
    const bodyDigest = await sha256Hex(body);
    const message = `${timestamp}.${method}.${path}.${bodyDigest}`;
    const sig = await hmacSha256Hex(secret, message);
    return {
      'X-Timestamp': timestamp,
      'X-Signature': sig,
    };
  } catch (e) {
    if ((import.meta as any)?.env?.DEV) {
      console.error('[API] Failed to generate signed headers', e);
    }
    return {};
  }
}

// Helper wrapper for backward compatibility
async function fetchJSON<T>(path: string, init?: RequestInit): Promise<T> {
  try {
    return await apiClient.fetchJSON<T>(path, init);
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    throw new Error(msg);
  }
}

const BULK_PARAMS_PATH = '/api/v1/params';

export const api = {
  // Scanners · registry and aggregate opportunities
  getScannersRegistry: async (): Promise<{ scanners: ScannerRegistryItem[]; total: number }> => {
    const response = await fetchJSON<FluxEnvelope<{ scanners: ScannerRegistryItem[]; total: number }>>('/api/v1/scanners/registry');
    const payload = unwrapFluxEnvelope(response);
    return { scanners: payload.scanners || [], total: payload.total ?? (payload.scanners?.length || 0) };
  },

  getScannerAggregateOpps: async (params: { min_edge?: number; limit?: number; bybit_marginable?: boolean; dex_name?: string; chain?: string }): Promise<ScannerAggregateOppsResponse> => {
    const qs = new URLSearchParams();
    if (params.min_edge != null) qs.set('min_edge', String(params.min_edge));
    if (params.limit != null) qs.set('limit', String(params.limit));
    if (params.bybit_marginable) qs.set('bybit_marginable', 'true');
    if (params.dex_name) qs.set('dex_name', params.dex_name);
    if (params.chain) qs.set('chain', params.chain);
    const response = await fetchJSON<FluxEnvelope<ScannerAggregateOppsResponse>>(`/api/v1/scanners/aggregate/opportunities?${qs.toString()}`);
    return unwrapFluxEnvelope(response);
  },

  getScannerOpportunities: async (scannerId: string, params: { min_edge?: number; limit?: number; bybit_marginable?: boolean }): Promise<{ opportunities: ScannerOpportunity[] }> => {
    const qs = new URLSearchParams();
    if (params.min_edge != null) qs.set('min_edge', String(params.min_edge));
    if (params.limit != null) qs.set('limit', String(params.limit));
    if (params.bybit_marginable) qs.set('bybit_marginable', 'true');
    const response = await fetchJSON<FluxEnvelope<{ opportunities: ScannerOpportunity[] }>>(`/api/v1/scanners/${encodeURIComponent(scannerId)}/opportunities?${qs.toString()}`);
    return unwrapFluxEnvelope(response) as { opportunities: ScannerOpportunity[] };
  },

  // Aggregate pricing fallback across all scanners (useful if a specific
  // scanner instance is not running yet in an environment)
  getScannerAggregatePricingSnapshots: async (params: { min_edge?: number; dex_name?: string; chain?: string; bybit_marginable?: boolean } = {}): Promise<{ snapshots: ScannerPricingSnapshot[]; total: number }> => {
    const qs = new URLSearchParams();
    if (params.min_edge != null) qs.set('min_edge', String(params.min_edge));
    if (params.dex_name) qs.set('dex_name', params.dex_name);
    if (params.chain) qs.set('chain', params.chain);
    if (params.bybit_marginable) qs.set('bybit_marginable', 'true');
    const response = await fetchJSON<FluxEnvelope<{ snapshots: ScannerPricingSnapshot[]; total: number }>>(
      `/api/v1/scanners/aggregate/pricing${qs.toString() ? `?${qs.toString()}` : ''}`
    );
    const payload = unwrapFluxEnvelope(response);
    return {
      snapshots: payload.snapshots || [],
      total: payload.total ?? (payload.snapshots?.length || 0),
    };
  },

  // Market data snapshot (debug/monitoring)
  getMarketDataSnapshot: async (): Promise<MarketDataSnapshotResponse> => {
    const response = await fetchJSON<
      FluxEnvelope<{
        rows: MarketSnapshot[];
        count?: number;
        freshness_key?: string | null;
        freshnessKey?: string | null;
        etag?: string | null;
        last_update_ms?: number | string | null;
        lastUpdateMs?: number | string | null;
      }>
    >('/api/v1/market-data/snapshot');
    const payload = unwrapFluxEnvelope(response);
    const normalizedRows = (payload.rows || []).map(normalizeMarketRow);
    const rawLastUpdateMs = payload.last_update_ms ?? payload.lastUpdateMs;
    const numericLastUpdateMs = rawLastUpdateMs == null ? undefined : Number(rawLastUpdateMs);
    return {
      rows: normalizedRows,
      count: payload.count ?? normalizedRows.length,
      freshnessKey: payload.freshness_key ?? payload.freshnessKey ?? undefined,
      etag: payload.etag ?? undefined,
      lastUpdateMs: Number.isFinite(numericLastUpdateMs) ? numericLastUpdateMs : undefined,
    };
  },

  getOrderViewSnapshot: async (params: GetOrderViewSnapshotParams): Promise<OrderViewSnapshot> => {
    const strategyId = String(params.strategyId ?? '').trim();
    if (!strategyId) {
      throw new Error('invalid_request');
    }

    const qs = new URLSearchParams({
      strategy_id: strategyId,
      leg: params.leg ?? 'maker',
      include_events: params.includeEvents === false ? '0' : '1',
      events_limit: String(params.eventsLimit ?? 200),
      include_book: params.includeBook ? '1' : '0',
      book_depth: String(params.bookDepth ?? 20),
      order_view_v02: params.orderViewV02 ? '1' : '0',
    });
    if (params.candleIntervalMs != null) qs.set('candle_interval_ms', String(params.candleIntervalMs));
    if (params.candleRange) qs.set('candle_range', String(params.candleRange));
    appendProfileQuery(qs);

    const response = await fetchJSON<FluxEnvelope<OrderViewSnapshot>>(
      `/api/v1/order-view/snapshot?${qs.toString()}`
    );
    return unwrapFluxEnvelope(response);
  },

  // FV server endpoints
  getFvSymbols: async (fvProfile = 'fv1'): Promise<string[]> => {
    const qs = new URLSearchParams();
    if (fvProfile) qs.set('fv_profile', fvProfile);
    const response = await fetchJSON<FluxEnvelope<{ symbols: string[] }>>(
      `/api/v1/fv/symbols${qs.toString() ? `?${qs.toString()}` : ''}`
    );
    const payload = unwrapFluxEnvelope(response);
    return payload?.symbols || [];
  },

  getFvLatest: async (
    symbol: string,
    options?: { fvProfile?: string; fvVersion?: number }
  ): Promise<FvSnapshot | null> => {
    const qs = new URLSearchParams();
    if (options?.fvProfile) qs.set('fv_profile', options.fvProfile);
    if (options?.fvVersion != null) qs.set('fv_version', String(options.fvVersion));
    const path = `/api/v1/fv/${encodeURIComponent(symbol)}/latest${qs.toString() ? `?${qs.toString()}` : ''}`;
    try {
      const response = await fetchJSON<FluxEnvelope<FvSnapshot>>(path);
      return unwrapFluxEnvelope(response);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      if (message.includes('404') || message.includes('not_found')) {
        return null;
      }
      throw error;
    }
  },

  getFvConfig: async (
    symbol: string,
    options?: { fvProfile?: string }
  ): Promise<Record<string, unknown>> => {
    const qs = new URLSearchParams();
    if (options?.fvProfile) qs.set('fv_profile', options.fvProfile);
    const path = `/api/v1/fv/${encodeURIComponent(symbol)}/config${qs.toString() ? `?${qs.toString()}` : ''}`;
    const response = await fetchJSON<FluxEnvelope<Record<string, unknown>>>(path);
    return unwrapFluxEnvelope(response);
  },

  updateFvConfig: async (
    symbol: string,
    patch: Record<string, unknown>,
    source = 'fluxboard',
    options?: { fvProfile?: string }
  ): Promise<Record<string, unknown>> => {
    const qs = new URLSearchParams();
    if (options?.fvProfile) qs.set('fv_profile', options.fvProfile);
    const path = `/api/v1/fv/${encodeURIComponent(symbol)}/config${qs.toString() ? `?${qs.toString()}` : ''}`;
    const payload = { ...patch, source };
    const extra = await signedJsonHeaders(payload, { method: 'POST', path });
    const response = await fetchJSON<FluxEnvelope<Record<string, unknown>>>(path, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', ...extra },
      body: JSON.stringify(payload),
    });
    return unwrapFluxEnvelope(response);
  },

  // Perf V2: Publish scanner performance stats to backend
  publishScannerPerfStats: async (stats: {
    updates_per_sec: number;
    apply_duration_p50_ms: number;
    apply_duration_p95_ms: number;
    index_update_p50_ms: number;
    index_update_p95_ms: number;
    render_duration_p50_ms: number;
    render_duration_p95_ms: number;
    visible_rows: number;
    total_rows: number;
    dropped_delta_rate_pct: number;
    delta_buffer_size: number;
    delta_buffer_high_water: number;
    last_update_ts: number;
    last_apply_duration_ms?: number;
    last_applied_at_ts?: number;
  }): Promise<void> => {
    try {
      await apiClient.fetchJSON<FluxEnvelope<{ stored: boolean }>>('/api/v1/scanners/perf-stats', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(stats),
      });
    } catch (error) {
      // Silently fail - stats publishing is non-critical
      if (import.meta.env?.DEV) {
        console.debug('[API] Failed to publish scanner perf stats', error);
      }
    }
  },

  // Hedger registry (all instances)
  listHedgerInstances: async (): Promise<HedgerInstanceMeta[]> => {
    const response = await fetchJSON<FluxEnvelope<HedgerInstanceMeta[]>>('/api/v1/hedgers/instances');
    return unwrapFluxEnvelope(response) || [];
  },

  getHedgerStatusById: async (hedgerId: string): Promise<HedgerStatus> => {
    const response = await fetchJSON<FluxEnvelope<HedgerStatus>>(`/api/v1/hedgers/${encodeURIComponent(hedgerId)}`);
    return unwrapFluxEnvelope(response);
  },

  setHedgerJobStateById: async (hedgerId: string, action: 'start' | 'stop' | 'restart'): Promise<HedgerStatus> => {
    const payload = { action };
    const headers = await signedJsonHeaders(payload);
    const response = await apiClient.fetchJSON<FluxEnvelope<HedgerStatus>>(
      `/api/v1/hedgers/${encodeURIComponent(hedgerId)}/job`,
      {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', ...headers },
        body: JSON.stringify(payload),
      }
    );
    return unwrapFluxEnvelope(response);
  },

  getHedgerConfig: async (hedgerId: string): Promise<HedgerConfig> => {
    const response = await fetchJSON<FluxEnvelope<HedgerConfig>>(
      `/api/v1/hedgers/${encodeURIComponent(hedgerId)}/config`
    );
    return unwrapFluxEnvelope(response);
  },

  patchHedgerConfig: async (hedgerId: string, patch: Partial<HedgerConfig>): Promise<HedgerConfig> => {
    const payload = patch;
    const headers = await signedJsonHeaders(payload);
    const response = await apiClient.fetchJSON<FluxEnvelope<HedgerConfig>>(
      `/api/v1/hedgers/${encodeURIComponent(hedgerId)}/config`,
      {
        method: 'PATCH',
        headers: { 'Content-Type': 'application/json', ...headers },
        body: JSON.stringify(payload),
      }
    );
    return unwrapFluxEnvelope(response);
  },

  // Hedger – ETH/PLUME LP hedger status (primary band)
  getEthPlumeHedgerStatus: async (): Promise<HedgerStatus> => {
    return api.getHedgerStatusById('eth_plume_lp');
  },

  setEthPlumeHedgerJobState: async (action: 'start' | 'stop' | 'restart'): Promise<HedgerStatus> => {
    return api.setHedgerJobStateById('eth_plume_lp', action);
  },

  // Hedger – Band2 instance
  getEthPlumeHedgerBand2Status: async (): Promise<HedgerStatus> => {
    return api.getHedgerStatusById('eth_plume_lp_band2');
  },

  setEthPlumeHedgerBand2JobState: async (action: 'start' | 'stop' | 'restart'): Promise<HedgerStatus> => {
    return api.setHedgerJobStateById('eth_plume_lp_band2', action);
  },

  setHedgerGeometryOverrides: async (
    overrides: HedgerGeometryOverrides
  ): Promise<HedgerGeometryResponse> => {
    const headers = await signedJsonHeaders(overrides);
    const response = await apiClient.fetchJSON<FluxEnvelope<HedgerGeometryResponse>>(
      '/api/v1/hedgers/eth_plume_lp/geometry-overrides',
      {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', ...headers },
        body: JSON.stringify(overrides),
      }
    );
    return unwrapFluxEnvelope(response);
  },

  clearHedgerGeometryOverrides: async (): Promise<HedgerGeometryResponse> => {
    const headers = await signedJsonHeaders({});
    const response = await apiClient.fetchJSON<FluxEnvelope<HedgerGeometryResponse>>(
      '/api/v1/hedgers/eth_plume_lp/geometry-overrides',
      {
        method: 'DELETE',
        headers: { 'Content-Type': 'application/json', ...headers },
      }
    );
    return unwrapFluxEnvelope(response);
  },

  setHedgerThresholdOverrides: async (
    overrides: HedgerThresholdOverrides
  ): Promise<HedgerThresholdResponse> => {
    const headers = await signedJsonHeaders(overrides);
    const response = await apiClient.fetchJSON<FluxEnvelope<HedgerThresholdResponse>>(
      '/api/v1/hedgers/eth_plume_lp/threshold-overrides',
      {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', ...headers },
        body: JSON.stringify(overrides),
      }
    );
    return unwrapFluxEnvelope(response);
  },

  clearHedgerThresholdOverrides: async (): Promise<HedgerThresholdResponse> => {
    const headers = await signedJsonHeaders({});
    const response = await apiClient.fetchJSON<FluxEnvelope<HedgerThresholdResponse>>(
      '/api/v1/hedgers/eth_plume_lp/threshold-overrides',
      {
        method: 'DELETE',
        headers: { 'Content-Type': 'application/json', ...headers },
      }
    );
    return unwrapFluxEnvelope(response);
  },

  setHedgerEnabled: async (enabled: boolean): Promise<{ hedger_enabled: boolean }> => {
    const payload = { enabled };
    const headers = await signedJsonHeaders(payload);
    const response = await apiClient.fetchJSON<FluxEnvelope<{ hedger_enabled: boolean }>>(
      '/api/v1/hedgers/eth_plume_lp/enabled',
      {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', ...headers },
        body: JSON.stringify(payload),
      }
    );
    return unwrapFluxEnvelope(response);
  },

  setHedgerBand2GeometryOverrides: async (
    overrides: HedgerGeometryOverrides
  ): Promise<HedgerGeometryResponse> => {
    const headers = await signedJsonHeaders(overrides);
    const response = await apiClient.fetchJSON<FluxEnvelope<HedgerGeometryResponse>>(
      '/api/v1/hedgers/eth_plume_lp_band2/geometry-overrides',
      {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', ...headers },
        body: JSON.stringify(overrides),
      }
    );
    return unwrapFluxEnvelope(response);
  },

  clearHedgerBand2GeometryOverrides: async (): Promise<HedgerGeometryResponse> => {
    const headers = await signedJsonHeaders({});
    const response = await apiClient.fetchJSON<FluxEnvelope<HedgerGeometryResponse>>(
      '/api/v1/hedgers/eth_plume_lp_band2/geometry-overrides',
      {
        method: 'DELETE',
        headers: { 'Content-Type': 'application/json', ...headers },
      }
    );
    return unwrapFluxEnvelope(response);
  },

  setHedgerBand2ThresholdOverrides: async (
    overrides: HedgerThresholdOverrides
  ): Promise<HedgerThresholdResponse> => {
    const headers = await signedJsonHeaders(overrides);
    const response = await apiClient.fetchJSON<FluxEnvelope<HedgerThresholdResponse>>(
      '/api/v1/hedgers/eth_plume_lp_band2/threshold-overrides',
      {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', ...headers },
        body: JSON.stringify(overrides),
      }
    );
    return unwrapFluxEnvelope(response);
  },

  clearHedgerBand2ThresholdOverrides: async (): Promise<HedgerThresholdResponse> => {
    const headers = await signedJsonHeaders({});
    const response = await apiClient.fetchJSON<FluxEnvelope<HedgerThresholdResponse>>(
      '/api/v1/hedgers/eth_plume_lp_band2/threshold-overrides',
      {
        method: 'DELETE',
        headers: { 'Content-Type': 'application/json', ...headers },
      }
    );
    return unwrapFluxEnvelope(response);
  },

  setHedgerBand2Enabled: async (enabled: boolean): Promise<{ hedger_enabled: boolean }> => {
    const payload = { enabled };
    const headers = await signedJsonHeaders(payload);
    const response = await apiClient.fetchJSON<FluxEnvelope<{ hedger_enabled: boolean }>>(
      '/api/v1/hedgers/eth_plume_lp_band2/enabled',
      {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', ...headers },
        body: JSON.stringify(payload),
      }
    );
    return unwrapFluxEnvelope(response);
  },

  clearHedgerEvents: async (): Promise<{ cleared: number }> => {
    const headers = await signedJsonHeaders({});
    const response = await apiClient.fetchJSON<FluxEnvelope<{ cleared: number }>>(
      '/api/v1/hedgers/eth_plume_lp/events/clear',
      {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', ...headers },
      }
    );
    return unwrapFluxEnvelope(response);
  },

  clearHedgerBand2Events: async (): Promise<{ cleared: number }> => {
    const headers = await signedJsonHeaders({});
    const response = await apiClient.fetchJSON<FluxEnvelope<{ cleared: number }>>(
      '/api/v1/hedgers/eth_plume_lp_band2/events/clear',
      {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', ...headers },
      }
    );
    return unwrapFluxEnvelope(response);
  },

  getScannerPricingSnapshots: async (
    scannerId: string,
    params: {
      cursor?: string | null;
      limit?: number;
      min_edge_bps?: number | null;
      min_tvl_usd?: number | null;
      search?: string;
      bybit_marginable?: boolean;
      dex_name?: string;
      chain?: string;
      sort_by?: 'last_update_ts' | 'best_edge_bps';
      sort_dir?: 'asc' | 'desc';
    } = {}
  ): Promise<{ snapshots: ScannerPricingSnapshot[]; total: number; pageInfo: ScannerPricingPageInfo }> => {
    const qs = new URLSearchParams();
    const limit = params.limit ?? 200;
    qs.set('limit', String(limit));
    if (params.cursor) qs.set('cursor', params.cursor);
    if (params.sort_by) qs.set('sort_by', params.sort_by);
    if (params.sort_dir) qs.set('sort_dir', params.sort_dir);
    if (params.min_edge_bps != null) qs.set('min_edge_bps', String(params.min_edge_bps));
    if (params.min_tvl_usd != null) qs.set('min_tvl_usd', String(params.min_tvl_usd));
    if (params.search) qs.set('search', params.search);
    if (params.bybit_marginable) qs.set('bybit_marginable', 'true');
    if (params.dex_name) qs.set('dex_name', params.dex_name);
    if (params.chain) qs.set('chain', params.chain);
    const response = await fetchJSON<FluxEnvelope<{ snapshots: ScannerPricingSnapshot[]; total: number }>>(
      `/api/v1/scanners/${encodeURIComponent(scannerId)}/pricing?${qs.toString()}`
    );
    const pageInfoRaw = (response as unknown as { page_info?: Partial<ScannerPricingPageInfo> }).page_info ?? {};
    const payload = unwrapFluxEnvelope(response);
    return {
      snapshots: payload.snapshots || [],
      total: payload.total ?? (payload.snapshots?.length || 0),
      pageInfo: {
        next_cursor: (pageInfoRaw.next_cursor ?? null) as string | null,
        has_more: Boolean(pageInfoRaw.has_more),
        limit: pageInfoRaw.limit ?? limit,
        sort_by: (pageInfoRaw.sort_by ?? params.sort_by ?? 'last_update_ts') as string,
        sort_dir: (pageInfoRaw.sort_dir ?? params.sort_dir ?? 'desc') as string,
      },
    };
  },

  // Trades snapshot
  getTrades: async (
    page: number,
    pageSize: number,
    params: Record<string, string | number | undefined> = {},
    init?: RequestInit
  ): Promise<{
    rows: TradeEvent[];
    total: number;
    last_seq?: number;
    page?: number;
    page_size?: number;
    total_records?: number;
    capped?: boolean;
    has_more?: boolean;
    next_offset?: number | null;
    next_cursor?: string | null;
    sort?: string;
  }> => {
    const normalizedPage = Number.isFinite(page) ? Math.max(page, 1) : 1;
    const normalizedPageSize = Number.isFinite(pageSize) ? Math.max(pageSize, 1) : 1;
    const limit = normalizedPageSize;
    const offset = (normalizedPage - 1) * normalizedPageSize;
    const cursorParam = typeof params.cursor === 'string' && params.cursor ? params.cursor : null;
    const qs = new URLSearchParams({
      limit: String(limit),
      offset: String(offset),
      sort: (params.sort as string) || 'ts_desc',
      coin: (params.coin as string) || '',
      exchange: (params.exchange as string) || '',
      side: (params.side as string) || '',
      signal_id: (params.signal_id as string) || '',
    });
    if (cursorParam) {
      qs.set('cursor', cursorParam);
      qs.set('offset', '0');
    }
    appendProfileQuery(qs);
    const response = await fetchJSON<FluxEnvelope<{
      rows: TradeEvent[];
      total: number;
      limit: number;
      offset: number;
      last_seq?: number;
      page?: number;
      page_size?: number;
      total_records?: number;
      capped?: boolean;
      next_cursor?: string | null;
      has_more?: boolean;
      next_offset?: number | null;
      sort?: string;
    }>>(`/api/v1/trades?${qs.toString()}`, init);
    const data = unwrapFluxEnvelope(response);
    const resolvedLimit =
      typeof data.limit === 'number' && !Number.isNaN(data.limit) && data.limit > 0
        ? data.limit
        : limit;
    const resolvedOffset =
      typeof data.offset === 'number' && !Number.isNaN(data.offset) && data.offset >= 0
        ? data.offset
        : offset;
    const resolvedPageSize =
      typeof data.page_size === 'number' && data.page_size > 0
        ? data.page_size
        : resolvedLimit;
    const resolvedPage =
      typeof data.page === 'number' && data.page > 0
        ? data.page
        : (resolvedPageSize > 0 ? Math.floor(resolvedOffset / resolvedPageSize) + 1 : normalizedPage);
    const rows = data.rows || [];
    const returned = rows.length;
    const totalCount = data.total ?? 0;
    const nextCursorValue = typeof data.next_cursor === 'string' ? data.next_cursor : null;
    const hasMore =
      typeof data.has_more === 'boolean'
        ? data.has_more
        : Boolean(nextCursorValue) || (resolvedOffset + returned) < totalCount;
    const nextOffset =
      typeof data.next_offset === 'number'
        ? data.next_offset
        : (!cursorParam && hasMore ? resolvedOffset + returned : null);

    return {
      rows,
      total: totalCount,
      last_seq: data.last_seq,
      page: resolvedPage,
      page_size: resolvedPageSize,
      total_records: data.total_records,
      capped: data.capped,
      has_more: hasMore,
      next_offset: nextOffset,
      next_cursor: nextCursorValue,
      sort: typeof data.sort === 'string' ? data.sort : (params.sort as string | undefined),
    };
  },

  getTradesDelta: async (
    sinceSeq: number,
    limit = 2000,
    init?: RequestInit
  ): Promise<{ rows: TradeEvent[]; last_seq?: number; reset_required?: boolean }> => {
    const qs = new URLSearchParams({
      since_seq: String(sinceSeq),
      limit: String(limit),
    });
    appendProfileQuery(qs);
    const r = await fetchJSON<FluxEnvelope<{ rows: TradeEvent[]; last_seq?: number; reset_required?: boolean }>>(`/api/v1/trades/delta?${qs.toString()}`, init);
    const data = unwrapFluxEnvelope(r);
    return {
      rows: (data.rows || []) as TradeEvent[],
      last_seq: data.last_seq,
      reset_required: data.reset_required,
    };
  },

  // Balances - FluxAPI v1 returns {"ok": true, "data": {...}}
  getBalances: async (): Promise<BalancesPayload> => {
    const qs = new URLSearchParams();
    appendProfileQuery(qs);
    const response = await fetchJSON<BalancesResponse>(
      `/api/v1/balances${qs.toString() ? `?${qs.toString()}` : ''}`
    );
    const payload = response?.data;
    if (!payload) {
      return {
        rows: [],
        total: 0,
        totals: { mv_raw: 0, mv_display: '$0.00' },
        generated_at: new Date().toISOString(),
        view: 'parents_only',
        risk_groups: [],
      };
    }
    return {
      ...payload,
      rows: Array.isArray(payload.rows) ? payload.rows : [],
      risk_groups: Array.isArray(payload.risk_groups) ? payload.risk_groups : [],
    };
  },

  // Signal strategies - FluxAPI v1 returns {"ok": true, "data": {"strategies": [...], "server_time": "..."}}
  getSignalStrategies: async (): Promise<SignalStrategiesPayload> => {
    const qs = new URLSearchParams();
    appendProfileQuery(qs);
    const response = await fetchJSON<{ ok: boolean; data: SignalStrategiesPayload }>(
      `/api/v1/signals${qs.toString() ? `?${qs.toString()}` : ''}`
    );
    return {
      strategies: response.data?.strategies || [],
      server_time: response.data?.server_time,
      server_ts_ms: response.data?.server_ts_ms,
      balance_summary: response.data?.balance_summary,
    };
  },

  // Signal strategies (FluxAPI v1) - Returns {"ok": true, "data": {"strategies": [...], "server_time": "..."}}
  getSignals: async (): Promise<SignalStrategiesPayload> => {
    const qs = new URLSearchParams();
    appendProfileQuery(qs);
    const response = await fetchJSON<{ ok: boolean; data: SignalStrategiesPayload }>(
      `/api/v1/signals${qs.toString() ? `?${qs.toString()}` : ''}`
    );
    return {
      strategies: response.data?.strategies || [],
      server_time: response.data?.server_time,
      server_ts_ms: response.data?.server_ts_ms,
      balance_summary: response.data?.balance_summary,
    };
  },

  // Strategies - Flask returns {"strategies": [...]}
  getStrategies: async (): Promise<string[]> => {
    const response = await fetchJSON<FluxEnvelope<{ strategies?: RawStrategy[]; rows?: RawStrategy[] }>>('/api/v1/strategies');
    const payload = unwrapFluxEnvelope(response);
    const strategies = payload?.strategies || payload?.rows || [];
    return strategies.map(s => s.id || '');
  },

  // Get full strategy objects with status (for Params page)
  getStrategiesWithStatus: async (): Promise<RawStrategy[]> => {
    const response = await fetchJSON<FluxEnvelope<{ strategies?: RawStrategy[]; rows?: RawStrategy[]; count: number }>>('/api/v1/strategies');
    const payload = unwrapFluxEnvelope(response);
    return payload?.strategies || payload?.rows || [];
  },

  getStrategyParams: async (id: string) => {
    const response = await fetchJSON<FluxEnvelope<{ params?: Record<string, any>; parameters?: Record<string, any> }>>(`/api/v1/strategies/${id}/parameters`);
    const payload = unwrapFluxEnvelope(response);
    const params = payload?.params || payload?.parameters || {};
    const normalized: Record<string, string> = {};
    for (const [k, v] of Object.entries(params)) {
      normalized[k] = String(v);
    }
    return normalized;
  },

  // Save strategy parameters with error toast
  saveStrategyParams: async (id: string, params: StrategyParams) => {
    try {
      const payload = { updates: [{ strategy_id: id, params }], source: 'fluxboard' };
      const extra = await signedJsonHeaders(payload, {
        method: 'PATCH',
        path: '/api/v1/params',
      });
      const result = await fetchJSON<FluxEnvelope<import('./types').BulkUpdateResult>>('/api/v1/params', {
        method: 'PATCH',
        headers: { 'Content-Type': 'application/json', ...extra },
        body: JSON.stringify(payload)
      });
      unwrapFluxEnvelope(result);
      toast.success('Parameters saved');
      return { ok: true } as const;
    } catch (e) {
      const msg = e instanceof Error ? e.message : 'Save failed';
      toast.error(`Save failed: ${msg}`);
      throw e;
    }
  },

  // Update strategy parameters without toast (for custom error handling)
  updateStrategyParams: async (id: string, params: StrategyParams) => {
    const payload = { updates: [{ strategy_id: id, params }], source: 'fluxboard' };
    const extra = await signedJsonHeaders(payload, {
      method: 'PATCH',
      path: BULK_PARAMS_PATH,
    });
    const response = await fetchJSON<FluxEnvelope<import('./types').BulkUpdateResult>>(BULK_PARAMS_PATH, {
      method: 'PATCH',
      headers: { 'Content-Type': 'application/json', ...extra },
      body: JSON.stringify(payload)
    });
    unwrapFluxEnvelope(response);
    return { ok: true } as const;
  },

  // PATCH strategy parameters (partial update with source field)
  patchStrategyParams: async (id: string, params: StrategyParams, source = 'fluxboard') => {
    const payload = { updates: [{ strategy_id: id, params }], source };
    const extra = await signedJsonHeaders(payload, {
      method: 'PATCH',
      path: BULK_PARAMS_PATH,
    });
    const response = await fetchJSON<FluxEnvelope<import('./types').BulkUpdateResult>>(BULK_PARAMS_PATH, {
      method: 'PATCH',
      headers: { 'Content-Type': 'application/json', ...extra },
      body: JSON.stringify(payload)
    });
    unwrapFluxEnvelope(response);
    return { ok: true } as const;
  },

  // Get parameter schema with validation rules
  getParamSchema: async () => {
    const qs = new URLSearchParams();
    appendProfileQuery(qs);
    const response = await fetchJSON<FluxEnvelope<import('./types').ParamSchema>>(
      `/api/v1/param-schema${qs.toString() ? `?${qs.toString()}` : ''}`
    );
    return unwrapFluxEnvelope(response);
  },

  // Get all strategy parameters in bulk
  getParams: async () => {
    const qs = new URLSearchParams();
    appendProfileQuery(qs);
    const response = await fetchJSON<FluxEnvelope<import('./types').ParamsResponse[]>>(
      `/api/v1/params${qs.toString() ? `?${qs.toString()}` : ''}`
    );
    return unwrapFluxEnvelope(response) || [];
  },

  // Bulk update multiple strategies
  updateParams: async (
    updates: import('./types').ParamUpdate[],
    source = 'fluxboard'
  ) => {
    const payload = { updates, source };
    const extra = await signedJsonHeaders(payload, { method: 'PATCH', path: BULK_PARAMS_PATH });
    const response = await fetchJSON<FluxEnvelope<import('./types').BulkUpdateResult>>(BULK_PARAMS_PATH, {
      method: 'PATCH',
      headers: { 'Content-Type': 'application/json', ...extra },
      body: JSON.stringify(payload)
    });
    return unwrapFluxEnvelope(response);
  },

  // Get config files for a strategy
  getStrategyConfig: async (id: string) => {
    const response = await fetchJSON<import('./types').ConfigResponse | FluxEnvelope<import('./types').ConfigResponse>>(`/api/v1/strategies/${id}/config-files`);
    return unwrapFluxEnvelope(response);
  },

  // Alerts - Flask returns {"alerts": [...]}
  getAlerts: async (): Promise<Alert[]> => {
    const qs = new URLSearchParams();
    appendProfileQuery(qs);
    const response = await fetchJSON<FluxEnvelope<{ rows: Alert[]; total: number }>>(
      `/api/v1/alerts${qs.toString() ? `?${qs.toString()}` : ''}`
    );
    const payload = unwrapFluxEnvelope(response);
    return payload?.rows || [];
  },

  // Clear all alerts with success toast (FluxAPI v1)
  clearAlerts: async () => {
    try {
      const extra = await signedJsonHeaders('', { method: 'DELETE', path: '/api/v1/alerts' });
      const qs = new URLSearchParams();
      appendProfileQuery(qs);
      const result = await fetchJSON<FluxEnvelope<{ success?: boolean; deleted?: number; remaining?: number }>>(
        `/api/v1/alerts${qs.toString() ? `?${qs.toString()}` : ''}`,
        {
          method: 'DELETE',
          headers: { 'Content-Type': 'application/json', ...extra }
        }
      );
      const payload = unwrapFluxEnvelope(result);
      const deleted = typeof payload.deleted === 'number' ? payload.deleted : 0;
      const remaining = typeof payload.remaining === 'number' ? payload.remaining : 0;
      const success = payload.success ?? deleted >= 0;
      if (success) {
        toast.success('All alerts cleared');
        return { success: true, deleted, remaining } as const;
      } else {
        throw new Error('Failed to clear alerts');
      }
    } catch (e) {
      const msg = e instanceof Error ? e.message : 'Clear failed';
      toast.error(`Clear alerts failed: ${msg}`);
      throw e;
    }
  },

  // FX service - calls FX service with timeout and validation
  getFxDashboard: async (): Promise<FxDashboard> => {
    // Default to Flask proxy path so prod uses same-origin /api/v1/fx/*
    const fxBase = import.meta.env.VITE_FX_BASE_URL || '/api';
    const controller = new AbortController();
    const timeoutId = setTimeout(() => controller.abort(), 3000);

    try {
      const r = await fetch(`${fxBase}/v1/fx/dashboard`, {
        signal: controller.signal,
        cache: 'no-store'
      });

      clearTimeout(timeoutId);

      const ct = r.headers.get('content-type') || '';
      if (!ct.includes('application/json')) {
        throw new Error(`Invalid Content-Type: ${ct}`);
      }

      if (!r.ok) {
        throw new Error(`${r.status} ${r.statusText}`);
      }

      const response = await r.json();

      // Map the actual FX service response to our FxDashboard type
      const pairs: FxPair[] = [];

      if (response.pairs && typeof response.pairs === 'object') {
        // response.pairs is an object with pair names as keys
        for (const [pairName, pairData] of Object.entries(response.pairs as Record<string, any>)) {
          if (response.cache && response.cache[pairName]) {
            const cacheData = response.cache[pairName];
            pairs.push({
              pair: pairName,
              price: cacheData.price || '0',
              source: cacheData.source || 'unknown',
              src_ts_ms: cacheData.timestamp ? cacheData.timestamp * 1000 : undefined,
              recv_ts_ms: Date.now(),
              age_ms: cacheData.age_ms || 0,
              stale: cacheData.is_stale || false,
              jump_bps: pairData.jump_bps !== undefined ? pairData.jump_bps : undefined,
              deviation_bps: pairData.deviation_bps !== undefined ? pairData.deviation_bps : undefined,
              clamp_breach: pairData.clamp_breach || false
            });
          }
        }
      }

      return {
        service: {
          ok: response.health?.bybit_connected || false,
          // NOTE: Uptime approximation using client-server time diff.
          // Assumes last_reload_time is Unix timestamp in seconds.
          // Not accurate if clocks are skewed. Ideally, FX service should calculate this.
          uptime_s: Date.now() / 1000 - (response.config?.last_reload_time || 0),
          version: response.config?.version || '1.0.0'
        },
        pairs,
        bybit: {
          connected: response.health?.bybit_connected || false,
          last_msg_age_ms: response.health?.bybit_last_message_time
            ? (Date.now() - response.health.bybit_last_message_time * 1000)
            : undefined,
          reconnects: response.health?.bybit_reconnect_count || 0
        },
        curve: {
          polls: response.health?.curve_pools_loaded || 0,
          last_poll_age_ms: undefined,
          errors: response.health?.curve_clients_initialized || 0
        }
      };
    } catch (e) {
      clearTimeout(timeoutId);
      if (e instanceof Error && e.name === 'AbortError') {
        throw new Error('Request timeout (3s)');
      }
      throw e;
    }
  },

  // PnL Report - Run report with parameters
  // Overloads:
  // - runPnLReport(params) -> Promise<PnLReport>
  // - runPnLReport(params, { etag }) -> Promise<RunPnLReportResult>
  runPnLReport: async (
    params: PnLParams,
    options?: RunPnLReportOptions
  ): Promise<any> => {
    const headers: Record<string, string> = { 'Content-Type': 'application/json' };
    if (options?.etag) headers['If-None-Match'] = options.etag;

    // Add a client-side timeout to avoid hanging UI when network stalls
    // Increased timeout for PnL reports (60s) since they can take longer
    const controller = new AbortController();
    const timeoutMs = 60000; // 60s for PnL reports (was 30s)
    const timeoutId = setTimeout(() => controller.abort(), timeoutMs);

    try {
      const response = await fetch(`${base}/api/v1/pnl`, {
        method: 'POST',
        headers,
        body: JSON.stringify(params),
        cache: 'no-store',
        signal: controller.signal,
      });

      clearTimeout(timeoutId);

      // If caller passed options (ETag flow), return structured result
      const wantsStructured = typeof options !== 'undefined';

      if (response.status === 304) {
        const etag = response.headers.get('ETag') || options?.etag || null;
        return wantsStructured
          ? { status: 304, etag, report: null }
          : Promise.reject(new Error('Not Modified'));
      }

      if (!response.ok) {
        throw new Error(`PnL request failed: ${response.status} ${response.statusText}`);
      }

      const payload = await response.json();
      if (payload && typeof payload === 'object' && 'ok' in payload) {
        if (!payload.ok) {
          const errMsg = typeof payload.error === 'string' ? payload.error : 'pnl_request_failed';
          throw new Error(errMsg);
        }
        const resolvedReport = payload.data as PnLReport;
        const resolvedEtag = response.headers.get('ETag') || resolvedReport?.report_signature || null;
        return wantsStructured
          ? { status: 200, etag: resolvedEtag, report: resolvedReport }
          : resolvedReport;
      }
      const fallbackReport = payload as PnLReport;
      const fallbackEtag = response.headers.get('ETag') || fallbackReport?.report_signature || null;
      return wantsStructured
        ? { status: 200, etag: fallbackEtag, report: fallbackReport }
        : fallbackReport;
    } catch (e) {
      clearTimeout(timeoutId);
      if (e instanceof Error && e.name === 'AbortError') {
        throw new Error(`PnL request timeout after ${timeoutMs}ms`);
      }
      throw e;
    }
  },

  runPnLDelta: async (params: PnLDeltaRequest, options?: RunPnLDeltaOptions): Promise<PnLDeltaResponse | { status: 304 }> => {
    const headers: Record<string, string> = { 'Content-Type': 'application/json' };
    if (options?.etag) {
      headers['If-None-Match'] = options.etag;
    }
    const controller = new AbortController();
    const timeoutMs = 30000; // 30s default
    const timeoutId = setTimeout(() => controller.abort(), timeoutMs);
    const response = await fetch(`${base}/api/v1/pnl/delta`, {
      method: 'POST',
      headers,
      body: JSON.stringify(params),
      cache: 'no-store',
      signal: controller.signal,
    });

    clearTimeout(timeoutId);

    if (response.status === 304) {
      const etag = response.headers.get('ETag') || options?.etag || null;
      return { status: 304 } as any;
    }

    if (!response.ok) {
      throw new Error(`PnL delta failed: ${response.status} ${response.statusText}`);
    }

    return response.json() as Promise<PnLDeltaResponse>;
  },

  runPnLInventoryReport: async (params: import('./types').PnLInventoryParams): Promise<import('./types').PnLInventoryReport> => {
    const controller = new AbortController();
    const timeoutMs = 60000;
    const timeoutId = setTimeout(() => controller.abort(), timeoutMs);
    try {
      const response = await fetch(`${base}/api/v1/pnl/inventory`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(params),
        cache: 'no-store',
        signal: controller.signal,
      });

      clearTimeout(timeoutId);

      if (!response.ok) {
        throw new Error(`PnL inventory failed: ${response.status} ${response.statusText}`);
      }

      const payload = await response.json();
      if (payload && typeof payload === 'object' && 'ok' in payload) {
        if (!payload.ok) {
          const errMsg = typeof payload.error === 'string' ? payload.error : 'pnl_inventory_failed';
          throw new Error(errMsg);
        }
        return payload.data as import('./types').PnLInventoryReport;
      }
      return payload as import('./types').PnLInventoryReport;
    } catch (e) {
      clearTimeout(timeoutId);
      if (e instanceof Error && e.name === 'AbortError') {
        throw new Error(`PnL inventory request timeout after ${timeoutMs}ms`);
      }
      throw e;
    }
  },

  // PnL CSV Export - Download ZIP file with CSVs
  downloadPnLCSV: async (params: PnLParams): Promise<void> => {
    try {
      const response = await fetch(`${base}/api/v1/pnl/csv`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(params)
      });

      if (!response.ok) {
        try {
          const errJson = await response.json();
          const errMsg = (errJson && typeof errJson.error === 'string') ? errJson.error : `${response.status} ${response.statusText}`;
          throw new Error(`CSV download failed: ${errMsg}`);
        } catch {
          // Fall back to status
          throw new Error(`CSV download failed: ${response.status} ${response.statusText}`);
        }
      }

      // Get filename from Content-Disposition header or use default
      const contentDisposition = response.headers.get('Content-Disposition');
      const filenameMatch = contentDisposition?.match(/filename="?([^"]+)"?/);
      const filename = filenameMatch?.[1] || 'pnl_report.zip';

      // Download file
      const blob = await response.blob();
      const url = window.URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = filename;
      document.body.appendChild(a);
      a.click();
      document.body.removeChild(a);
      window.URL.revokeObjectURL(url);

      toast.success('CSV export downloaded');
    } catch (e) {
      const msg = e instanceof Error ? e.message : 'CSV download failed';
      toast.error(msg);
      throw e;
    }
  },

  getAvailableSymbols: async (): Promise<string[]> => {
    try {
      const resp = await fetchJSON<any>('/api/v1/pnl/symbols');
      const payload = (resp && typeof resp === 'object' && 'ok' in resp) ? resp.data : resp;
      const bases: string[] | undefined = Array.isArray(payload?.bases) ? payload.bases : undefined;
      if (bases && bases.length > 0) {
        return bases;
      }
      const symbols: string[] = Array.isArray(payload?.symbols) ? payload.symbols : [];
      const derived = Array.from(new Set(symbols.map((s) => {
        const base = (s && s.includes('/') ? s.split('/', 1)[0] : s || '').toUpperCase();
        if (base === 'WETH') return 'ETH';
        if (base === 'WSEI') return 'SEI';
        if (base === 'WPLUME') return 'PLUME';
        return base;
      })));
      return derived.length ? derived : ['PLUME', 'ETH', 'SEI', 'ASTER', 'WBNB'];
    } catch (error) {
      if (import.meta.env?.DEV) {
        console.error('[api] Failed to fetch symbols:', error);
      }
      return ['PLUME', 'ETH', 'SEI', 'ASTER', 'WBNB'];
    }
  },

  // Get strategy FX configuration
  getStrategyFxConfig: async (): Promise<import('./types').StrategyFxConfig[]> => {
    const response = await fetchJSON<import('./types').StrategyFxConfigResponse>('/api/v1/fx/strategies');
    return response.strategies || [];
  }
};
type HedgerGeometryResponse = {
  geometry_overrides: HedgerGeometryOverrides;
  geometry_effective: HedgerGeometry;
};

type HedgerThresholdResponse = {
  threshold_overrides: HedgerThresholdOverrides;
  threshold_effective: HedgerThresholds;
};
