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
  BalanceParentRow,
  BalanceChildRow,
  CanonicalNamingFields,
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
  ParamDef,
  ParamSchema,
  ParamsResponse,
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

function buildProfileScopedPath(path: string): string {
  const qs = new URLSearchParams();
  appendProfileQuery(qs);
  const query = qs.toString();
  return query ? `${path}?${query}` : path;
}

function routePrefersKeyLabel(profile: PathProfile): boolean {
  return profile === 'tokenmm' || profile === 'equities';
}

// Create enhanced API client instance with timeout, retry, and deduplication
const apiClient = new APIClient(base);

type FluxEnvelope<T> = { ok: boolean; data: T; error?: unknown };
type TradesDeltaCursor = number | {
  sinceSeq?: number;
  afterMs?: number;
  afterRowId?: string;
  afterVersion?: number;
};

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

function extractFluxErrorMessage(payload: unknown): string | null {
  if (typeof payload === 'string') {
    const message = payload.trim();
    return message || null;
  }
  if (!payload || typeof payload !== 'object') {
    return null;
  }

  const data = payload as Record<string, unknown>;
  const directMessage = typeof data.message === 'string' ? data.message.trim() : '';
  if (directMessage) return directMessage;

  const directError = typeof data.error === 'string' ? data.error.trim() : '';
  if (directError) return directError;

  const nestedError = extractFluxErrorMessage(data.error);
  if (nestedError) return nestedError;

  const errors = extractBulkUpdateFailures(data);
  if (errors.length > 0) {
    return errors
      .map((entry) => (entry.strategy_id ? `${entry.strategy_id}: ${entry.message}` : entry.message))
      .join('; ');
  }

  const nestedData = extractFluxErrorMessage(data.data);
  if (nestedData) return nestedData;

  const code = typeof data.code === 'string' ? data.code.trim() : '';
  return code || null;
}

function toFiniteNumber(value: unknown, fallback = 0): number {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : fallback;
}

function coerceTimestampMs(value: unknown): number | undefined {
  const ts = Number(value);
  if (!Number.isFinite(ts) || ts <= 0) return undefined;
  if (ts < 1e12) return Math.trunc(ts * 1000);
  if (ts >= 1e18) return Math.trunc(ts / 1e6);
  if (ts >= 1e15) return Math.trunc(ts / 1e3);
  return Math.trunc(ts);
}

function toUpperToken(value: unknown, fallback = 'UNKNOWN'): string {
  const text = String(value ?? '').trim().toUpperCase();
  return text || fallback;
}

function formatMoneyDisplay(value: number): string {
  const safe = Number.isFinite(value) ? value : 0;
  return `${safe < 0 ? '-$' : '$'}${Math.abs(safe).toFixed(2)}`;
}

function normalizeTradesSortParam(sort: string | undefined): string {
  if (sort === 'ts_asc') {
    return 'asc';
  }
  return sort || 'ts_desc';
}

function normalizeCoinHint(raw: Record<string, unknown>): string {
  const direct = toUpperToken(raw.coin ?? raw.base_currency ?? raw.asset ?? raw.base, '');
  if (direct) return direct;

  const instrumentId = toUpperToken(raw.instrument_id, '');
  if (!instrumentId) return 'UNKNOWN';
  const symbol = instrumentId.split('.')[0] || instrumentId;
  for (const quote of ['USDT', 'USDC', 'USD', 'PERP']) {
    if (symbol.endsWith(quote) && symbol.length > quote.length) {
      return symbol.slice(0, -quote.length) || symbol;
    }
  }
  return symbol;
}

function normalizeFlatBalancesRows(rows: unknown[]): BalanceParentRow[] {
  const stableTokens = new Set(['USD', 'USDT', 'USDC', 'DAI', 'FDUSD', 'USDE']);
  const byParent = new Map<string, BalanceParentRow>();

  rows.forEach((candidate, index) => {
    if (!candidate || typeof candidate !== 'object') return;
    const row = candidate as Record<string, unknown>;

    const canonical = normalizeCoinHint(row);
    const parentId = `${canonical}_LOGICAL`;
    const venue = String(row.exchange ?? row.venue ?? 'unknown').trim().toLowerCase() || 'unknown';
    const isPosition = String(row.kind ?? '').trim().toLowerCase() === 'position';
    const naming = deriveCanonicalNaming(row, {
      exchange: venue,
      symbol: String(row.symbol ?? '').trim(),
      asset: canonical,
      isPosition,
    });
    const childCoin = (() => {
      const preferred = toUpperToken(
        row.inventory_asset ?? row.base_asset ?? row.coin ?? row.base_currency ?? row.base ?? row.asset ?? canonical,
        canonical,
      );
      if (preferred === 'UNKNOWN' && canonical !== 'UNKNOWN') {
        return canonical;
      }
      return preferred;
    })();
    const qtyValue = isPosition
      ? (row.signed_qty ?? row.total ?? row.quantity ?? row.qty ?? row.free)
      : (row.total ?? row.quantity ?? row.signed_qty ?? row.qty ?? row.free);
    const qtyRaw = toFiniteNumber(qtyValue, 0);
    let mvRaw = toFiniteNumber(
      row.mv_raw ?? row.mv ?? row.notional ?? row.notional_quote ?? row.notional_usd,
      Number.NaN,
    );
    const markRawValue = row.mark_raw ?? row.mark ?? row.avg_px_open ?? row.price;
    const markRaw = markRawValue == null ? null : toFiniteNumber(markRawValue, 0);
    if (!Number.isFinite(mvRaw)) {
      if (markRaw != null && Number.isFinite(markRaw)) {
        mvRaw = qtyRaw * markRaw;
      } else if (stableTokens.has(canonical)) {
        mvRaw = qtyRaw;
      } else {
        mvRaw = 0;
      }
    }
    const tsMs = coerceTimestampMs(row.ts_ms ?? row.ts ?? row.timestamp) ?? 0;

    const child: BalanceChildRow = {
      id: String(row.row_id ?? `${parentId}:${venue}:${childCoin}:${index}`),
      parent_id: parentId,
      coin: childCoin,
      ...naming,
      venue,
      wallet: String(row.account ?? row.account_id ?? '').trim() || null,
      address: String(row.address ?? '').trim() || null,
      label: String(row.label ?? row.kind ?? '').trim() || null,
      qty_display: String(qtyValue ?? qtyRaw),
      qty_raw: qtyRaw,
      mv_display: formatMoneyDisplay(mvRaw),
      mv_raw: mvRaw,
      mark_display: markRaw == null ? null : String(markRaw),
      mark_raw: markRaw,
      time_display: tsMs > 0 ? new Date(tsMs).toISOString() : '',
      time_iso: tsMs > 0 ? new Date(tsMs).toISOString() : null,
      last_ts: tsMs > 0 ? tsMs : null,
      chain: String(row.chain ?? '').trim() || null,
      contract: String(row.contract ?? row.instrument_id ?? '').trim() || null,
      risk_key: String(row.risk_key ?? '').trim() || null,
      risk_label: String(row.risk_label ?? '').trim() || null,
    };

    const existing = byParent.get(parentId);
    if (existing) {
      existing.children.push(child);
      existing.qty_raw += qtyRaw;
      existing.mv_raw += mvRaw;
      existing.qty_display = String(existing.qty_raw);
      existing.mv_display = formatMoneyDisplay(existing.mv_raw);
      if (markRaw != null) {
        existing.mark_raw = markRaw;
        existing.mark_display = String(markRaw);
      }
      if ((existing.last_ts ?? 0) < tsMs) {
        existing.last_ts = tsMs;
        existing.time_iso = child.time_iso;
        existing.time_display = child.time_display;
      }
      return;
    }

    byParent.set(parentId, {
      id: parentId,
      coin: `${canonical}_LOGICAL`,
      canonical,
      is_parent: true,
      stable: stableTokens.has(canonical),
      qty_display: String(qtyRaw),
      qty_raw: qtyRaw,
      mv_display: formatMoneyDisplay(mvRaw),
      mv_raw: mvRaw,
      mark_display: markRaw == null ? null : String(markRaw),
      mark_raw: markRaw,
      time_display: child.time_display,
      time_iso: child.time_iso,
      last_ts: child.last_ts,
      children: [child],
      raw: {
        qty: qtyRaw,
        mv_usd: mvRaw,
        mark: markRaw,
      },
    });
  });

  return Array.from(byParent.values());
}

function normalizeBalancesRows(rows: unknown): BalanceParentRow[] {
  if (!Array.isArray(rows)) return [];
  if (rows.length === 0) return [];

  const looksLikeParentRows = rows.every((row) => {
    if (!row || typeof row !== 'object') return false;
    return Array.isArray((row as { children?: unknown }).children);
  });

  if (looksLikeParentRows) {
    return rows.map((row) => {
      const parent = row as BalanceParentRow;
      return {
        ...parent,
        children: Array.isArray(parent.children) ? parent.children : [],
      };
    });
  }

  return normalizeFlatBalancesRows(rows);
}

function normalizeRiskGroups(groups: unknown): BalancesPayload['risk_groups'] {
  if (!Array.isArray(groups)) return [];
  return groups
    .filter((group): group is Record<string, unknown> => Boolean(group) && typeof group === 'object')
    .map((group) => ({
      ...group,
      risk_key: String(group.risk_key ?? ''),
      label: String(group.label ?? group.risk_key ?? ''),
      rows: Array.isArray(group.rows)
        ? group.rows
            .filter((row): row is Record<string, unknown> => Boolean(row) && typeof row === 'object')
            .map((row) => ({
              row_id: typeof row.row_id === 'string' && row.row_id ? row.row_id : null,
              venue: String(row.venue ?? ''),
              coin: String(row.coin ?? ''),
              qty_raw: toFiniteNumber(row.qty_raw, 0),
              mv_raw: toFiniteNumber(row.mv_raw, 0),
              mark_raw: row.mark_raw == null ? null : toFiniteNumber(row.mark_raw, 0),
              time_display: typeof row.time_display === 'string' ? row.time_display : null,
              label: typeof row.label === 'string' && row.label ? row.label : null,
              wallet: typeof row.wallet === 'string' && row.wallet ? row.wallet : null,
              address: typeof row.address === 'string' && row.address ? row.address : null,
            }))
        : [],
    }));
}

function toFiniteOptionalNumber(value: unknown): number | undefined {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : undefined;
}

function normalizeTradingFlag(value: unknown): string | undefined {
  if (value == null) return undefined;
  if (typeof value === 'boolean') return value ? '1' : '0';
  if (typeof value === 'number') return Number.isFinite(value) ? (value !== 0 ? '1' : '0') : undefined;
  if (typeof value === 'string') {
    const trimmed = value.trim();
    if (!trimmed) return undefined;
    const lower = trimmed.toLowerCase();
    if (lower === '1' || lower === 'true' || lower === 't' || lower === 'yes' || lower === 'y' || lower === 'on' || lower === 'enabled') {
      return '1';
    }
    if (lower === '0' || lower === 'false' || lower === 'f' || lower === 'no' || lower === 'n' || lower === 'off' || lower === 'disabled') {
      return '0';
    }
    return undefined;
  }
  return undefined;
}

function normalizeParamType(key: string, rawType: unknown): ParamDef['type'] {
  const normalized = String(rawType ?? '').trim().toLowerCase();
  if (normalized === 'boolean' || normalized === 'bool') {
    return key === 'bot_on' ? 'select' : 'bool';
  }
  if (normalized === 'integer' || normalized === 'int') return 'int';
  if (normalized === 'number' || normalized === 'float' || normalized === 'double' || normalized === 'decimal') return 'float';
  if (normalized === 'select' || normalized === 'enum') return 'select';
  return key === 'bot_on' ? 'select' : 'float';
}

function normalizeParamOptions(
  key: string,
  rawType: unknown,
  rawOptions: unknown,
  opts?: { preferKeyLabel?: boolean },
): [string, string][] | null {
  if (Array.isArray(rawOptions) && rawOptions.length > 0) {
    const normalized = rawOptions
      .map((option) => {
        if (Array.isArray(option) && option.length >= 2) {
          return [String(option[0]), String(option[1])] as [string, string];
        }
        const text = String(option ?? '').trim();
        if (!text) return null;
        return [text, text] as [string, string];
      })
      .filter((option): option is [string, string] => Array.isArray(option));
    return normalized.length > 0 ? normalized : null;
  }

  const typeText = String(rawType ?? '').trim().toLowerCase();
  if (key === 'bot_on' || typeText === 'boolean' || typeText === 'bool') {
    return opts?.preferKeyLabel === true
      ? [['0', 'Off (0)'], ['1', 'On (1)']]
      : [['0', 'Paused (0)'], ['1', 'Enabled (1)']];
  }
  return null;
}

function botOnLabel(opts?: { preferKeyLabel?: boolean }): string {
  return opts?.preferKeyLabel === true ? 'bot_on' : 'Trading';
}

function botOnDescription(): string {
  return 'Trading gate. Controls whether the strategy may place new orders. Independent of runner state. 1 = Enabled, 0 = Paused.';
}

function normalizeParamDef(
  key: string,
  rawDef: unknown,
  deprecated = false,
  opts?: { preferKeyLabel?: boolean },
): ParamDef {
  const source = rawDef && typeof rawDef === 'object' ? (rawDef as Record<string, unknown>) : {};
  const type = normalizeParamType(key, source.type);
  const description =
    key === 'bot_on'
      ? botOnDescription()
      : (String(source.description ?? source.label ?? key).trim() || key);
  const preferKeyLabel = opts?.preferKeyLabel === true;
  const label =
    key === 'bot_on'
      ? botOnLabel(opts)
      : preferKeyLabel
        ? key
        : (String(source.label ?? source.description ?? key).trim() || key);
  const options = normalizeParamOptions(key, source.type, source.options, opts);
  const step = toFiniteOptionalNumber(source.step);
  const minValue = toFiniteOptionalNumber(source.min_value ?? source.minimum);
  const maxValue = toFiniteOptionalNumber(source.max_value ?? source.maximum);
  const appliesTo = Array.isArray(source.applies_to)
    ? source.applies_to.map((value) => String(value ?? '').trim()).filter(Boolean)
    : undefined;
  const advanced = Boolean(source.advanced);
  let defaultValue = source.default;
  if (defaultValue === undefined || defaultValue === null) {
    if (key === 'bot_on') {
      defaultValue = '0';
    } else if (type === 'int' || type === 'float') {
      defaultValue = '';
    }
  } else if (key === 'bot_on') {
    defaultValue = normalizeTradingFlag(defaultValue) ?? defaultValue;
  }

  return {
    key,
    label,
    description,
    type,
    default: defaultValue,
    min_value: minValue,
    max_value: maxValue,
    step: step ?? null,
    options,
    unit: source.unit == null ? null : String(source.unit),
    deprecated: Boolean(source.deprecated ?? deprecated),
    replacement: source.replacement == null ? null : String(source.replacement),
    applies_to: appliesTo,
    advanced,
  };
}

function normalizeParamSchemaPayload(payload: unknown): ParamSchema {
  return normalizeParamSchemaPayloadWithOptions(payload, {});
}

function normalizeParamSchemaPayloadWithOptions(
  payload: unknown,
  options: { preferKeyLabel?: boolean },
): ParamSchema {
  const data = payload && typeof payload === 'object' ? (payload as Record<string, unknown>) : {};
  const paramsRaw =
    data.params && typeof data.params === 'object'
      ? (data.params as Record<string, unknown>)
      : {};
  const deprecatedRaw =
    data.deprecated && typeof data.deprecated === 'object'
      ? (data.deprecated as Record<string, unknown>)
      : {};

  const params: Record<string, ParamDef> = {};
  for (const [key, rawDef] of Object.entries(paramsRaw)) {
    params[key] = normalizeParamDef(key, rawDef, false, options);
  }
  const deprecated: Record<string, ParamDef> = {};
  for (const [key, rawDef] of Object.entries(deprecatedRaw)) {
    deprecated[key] = normalizeParamDef(key, rawDef, true, options);
  }
  return { params, deprecated };
}

function normalizeParamsMap(
  raw: unknown,
  schema?: Record<string, unknown> | null,
): Record<string, string> {
  if (!raw || typeof raw !== 'object') return {};
  const out: Record<string, string> = {};
  for (const [key, value] of Object.entries(raw as Record<string, unknown>)) {
    if (value == null) continue;
    const schemaEntry =
      schema && typeof schema === 'object' ? (schema as Record<string, unknown>)[key] : undefined;
    const schemaType =
      schemaEntry && typeof schemaEntry === 'object'
        ? String((schemaEntry as Record<string, unknown>).type ?? '').trim().toLowerCase()
        : '';
    if (key === 'bot_on' || typeof value === 'boolean' || schemaType === 'boolean' || schemaType === 'bool') {
      out[key] = normalizeTradingFlag(value) ?? String(value);
      continue;
    }
    out[key] = String(value);
  }
  return out;
}

function deriveCoinFromSymbol(rawSymbol: unknown): string | undefined {
  const symbol = String(rawSymbol ?? '').trim().toUpperCase();
  if (!symbol) return undefined;
  const baseSymbolFromVenue = symbol.split('.')[0] || symbol;
  const baseSymbolFromSlash = baseSymbolFromVenue.split('/')[0] || baseSymbolFromVenue;
  const baseSymbol = baseSymbolFromSlash.split('-')[0] || baseSymbolFromSlash;
  for (const quote of ['USDT', 'USDC', 'USD', 'PERP']) {
    if (baseSymbol.endsWith(quote) && baseSymbol.length > quote.length) {
      return baseSymbol.slice(0, -quote.length);
    }
  }
  return baseSymbol || undefined;
}

function deriveExchangeFromInstrument(instrumentId: unknown): string | undefined {
  const text = String(instrumentId ?? '').trim();
  if (!text) return undefined;
  const suffix = text.split('.').pop()?.trim().toLowerCase();
  return suffix || undefined;
}

function deriveRawSymbolFromInstrument(instrumentId: unknown): string {
  const text = String(instrumentId ?? '').trim().split('.')[0]?.trim().toUpperCase() || '';
  return stripContractSuffix(text).symbol;
}

function deriveContractTypeFromInstrument(instrumentId: unknown): string | undefined {
  const text = String(instrumentId ?? '').trim().split('.')[0]?.trim().toUpperCase() || '';
  return stripContractSuffix(text).contractType;
}

function deriveContractTypeFromVenue(venue: unknown): string | undefined {
  const text = String(venue ?? '').trim().toUpperCase();
  if (!text) return undefined;
  for (const [suffix, contractType] of [
    ['_LINEAR', 'linear'],
    ['_SWAP', 'swap'],
    ['_INVERSE', 'inverse'],
    ['_PERP', 'perp'],
    ['_SPOT', 'spot'],
  ] as const) {
    if (text.endsWith(suffix) && text.length > suffix.length) {
      return contractType;
    }
  }
  return undefined;
}

function stripContractSuffix(rawSymbol: string): { symbol: string; contractType?: string } {
  const text = rawSymbol.trim().toUpperCase();
  if (!text) return { symbol: '' };
  for (const [suffix, contractType] of [
    ['-LINEAR', 'linear'],
    ['-SWAP', 'swap'],
    ['-INVERSE', 'inverse'],
    ['-PERP', 'perp'],
    ['-SPOT', 'spot'],
  ] as const) {
    if (text.endsWith(suffix) && text.length > suffix.length) {
      return { symbol: text.slice(0, -suffix.length), contractType };
    }
  }
  return { symbol: text };
}

function deriveQuoteAssetFromSymbol(rawSymbol: string): string | undefined {
  const text = rawSymbol.trim().toUpperCase();
  if (!text) return undefined;
  const { symbol } = stripContractSuffix(text);
  const pairText = symbol.replace(/-/g, '_');
  if (pairText.includes('/')) {
    return pairText.split('/', 2)[1]?.trim().toUpperCase() || undefined;
  }
  if (pairText.includes('_')) {
    return pairText.split('_', 2)[1]?.trim().toUpperCase() || undefined;
  }
  for (const quote of ['USDT', 'USDC', 'PUSD', 'USDE', 'USD', 'BTC', 'ETH', 'EUR', 'GBP', 'JPY', 'BNB']) {
    if (pairText.endsWith(quote) && pairText.length > quote.length) {
      return quote;
    }
  }
  return undefined;
}

function deriveVenueRoot(
  rawVenueRoot: unknown,
  venue: unknown,
  exchange: unknown,
  instrumentId: unknown,
): string {
  const explicit = String(rawVenueRoot ?? '').trim().toLowerCase();
  if (explicit) return explicit;
  const venueText = String(venue ?? '').trim().toLowerCase();
  if (venueText) return venueText.split('_')[0] || venueText;
  const exchangeText = String(exchange ?? '').trim().toLowerCase();
  if (exchangeText) return exchangeText.split('_')[0] || exchangeText;
  const derived = deriveExchangeFromInstrument(instrumentId);
  return (derived ?? '').split('_')[0] || (derived ?? '');
}

export function deriveCanonicalNaming(
  raw: Record<string, unknown>,
  {
    exchange,
    symbol,
    asset,
    isPosition = false,
  }: {
    exchange?: string;
    symbol?: string;
    asset?: string;
    isPosition?: boolean;
  } = {},
): CanonicalNamingFields {
  const instrumentId = String(raw.instrument_id ?? '').trim().toUpperCase();
  const venue = String(raw.venue ?? '').trim().toUpperCase()
    || String(exchange ?? raw.exchange ?? deriveExchangeFromInstrument(instrumentId) ?? '').trim().toUpperCase();
  const venueRoot = deriveVenueRoot(raw.venue_root, venue, exchange ?? raw.exchange, instrumentId);
  const derivedRawSymbol = deriveRawSymbolFromInstrument(instrumentId);
  const rawSymbolSource = raw.raw_symbol ?? (derivedRawSymbol || symbol || raw.symbol || '');
  const stripped = stripContractSuffix(String(rawSymbolSource).trim().toUpperCase());
  const rawSymbol = stripped.symbol;
  const explicitContractType = String(raw.contract_type ?? '').trim().toLowerCase();
  let contractType =
    explicitContractType
    || deriveContractTypeFromInstrument(instrumentId)
    || stripped.contractType
    || deriveContractTypeFromVenue(venue)
    || deriveContractTypeFromVenue(exchange ?? raw.exchange)
    || '';
  if (!contractType) {
    if (venue.endsWith('_SPOT')) contractType = 'spot';
    else if (venue.endsWith('_PERP')) contractType = 'perp';
    else if (isPosition && venueRoot === 'ibkr') contractType = 'equity';
    else if (isPosition && rawSymbol) contractType = 'spot';
    else if (isPosition) contractType = 'perp';
    else if (rawSymbol) contractType = 'spot';
    else contractType = 'cash';
  }
  const explicitProductType = String(raw.product_type ?? raw.market_type ?? '').trim().toLowerCase();
  const productType = explicitProductType || (['linear', 'swap', 'inverse', 'perp'].includes(contractType) ? 'perp' : 'spot');
  const pairText = String(raw.pair ?? symbol ?? raw.symbol ?? '').trim().toUpperCase();
  const baseAsset = String(
    raw.base_asset ??
      raw.inventory_asset ??
      deriveCoinFromSymbol(pairText || stripped.symbol || rawSymbol) ??
      asset ??
      raw.coin ??
      raw.asset ??
      '',
  ).trim().toUpperCase();
  const quoteAsset = String(raw.quote_asset ?? deriveQuoteAssetFromSymbol(pairText || rawSymbol) ?? '').trim().toUpperCase();
  const pair = String(raw.pair ?? ((baseAsset && quoteAsset) ? `${baseAsset}/${quoteAsset}` : (pairText || stripped.symbol || rawSymbol))).trim();
  const inventoryAsset = String(raw.inventory_asset ?? asset ?? raw.coin ?? raw.asset ?? baseAsset).trim().toUpperCase();
  const displayAsset = inventoryAsset || baseAsset || rawSymbol;
  const displayNameShort = String(
    raw.display_name_short ??
      (
        displayAsset
          ? `${displayAsset} ${
              contractType === 'equity'
                ? 'Stock'
                : (productType === 'perp' ? 'Perp' : 'Spot')
            }`
          : ''
      ),
  ).trim();
  const displayNameLong = String(
    raw.display_name_long ??
      ([venueRoot ? `${venueRoot[0]?.toUpperCase() ?? ''}${venueRoot.slice(1)}` : '', displayNameShort].filter(Boolean).join(' ')),
  ).trim();
  const instrumentUid = String(
    raw.instrument_uid ??
      ([venueRoot, contractType, instrumentId || rawSymbol || inventoryAsset].filter(Boolean).join(':')),
  ).trim();

  return {
    instrument_uid: instrumentUid || undefined,
    instrument_id: instrumentId || undefined,
    venue: venue || undefined,
    venue_root: venueRoot || undefined,
    product_type: productType || undefined,
    market_type: productType || undefined,
    contract_type: contractType || undefined,
    raw_symbol: rawSymbol || undefined,
    base_asset: baseAsset || undefined,
    quote_asset: quoteAsset || undefined,
    pair: pair || undefined,
    inventory_asset: inventoryAsset || undefined,
    display_name_short: displayNameShort || undefined,
    display_name_long: displayNameLong || undefined,
  };
}

function normalizeLegacySignalLeg(contractId: string, candidate: unknown): Record<string, unknown> | null {
  if (candidate == null) return null;
  if (typeof candidate !== 'object') {
    return { contract_id: contractId };
  }
  const raw = candidate as Record<string, unknown>;
  const normalized: Record<string, unknown> = { ...raw };
  const decisionBid = toFiniteOptionalNumber(raw.decision_bid ?? raw.fv_bid ?? raw.bid);
  const decisionAsk = toFiniteOptionalNumber(raw.decision_ask ?? raw.fv_ask ?? raw.ask);
  const tsMs = toFiniteOptionalNumber(raw.update_ts_ms ?? raw.ts_ms ?? raw.timestamp);
  const ageMs = toFiniteOptionalNumber(raw.md_age_ms ?? raw.age_ms);
  const symbol = String(raw.symbol ?? '').trim();
  const exchangeFromContract = String(contractId || '').split(':')[0]?.trim().toLowerCase() || '';
  const exchange = String(raw.exchange ?? exchangeFromContract).trim().toLowerCase();
  const naming = deriveCanonicalNaming(raw, {
    exchange,
    symbol,
    asset: String(raw.coin ?? '').trim(),
    isPosition: false,
  });
  const coin = String(raw.coin ?? deriveCoinFromSymbol(symbol) ?? '').trim().toUpperCase();

  if (!normalized.contract_id) normalized.contract_id = contractId;
  if (decisionBid !== undefined) {
    if (normalized.fv_bid == null) normalized.fv_bid = decisionBid;
    if (normalized.decision_bid == null) normalized.decision_bid = decisionBid;
  }
  if (decisionAsk !== undefined) {
    if (normalized.fv_ask == null) normalized.fv_ask = decisionAsk;
    if (normalized.decision_ask == null) normalized.decision_ask = decisionAsk;
  }
  if (normalized.mid == null && decisionBid !== undefined && decisionAsk !== undefined) {
    normalized.mid = (decisionBid + decisionAsk) / 2;
  }
  if (!normalized.update_time && tsMs !== undefined) {
    normalized.update_time = new Date(tsMs).toISOString().replace('T', ' ').slice(0, 19);
  }
  if (normalized.update_ts_ms == null && tsMs !== undefined) normalized.update_ts_ms = tsMs;
  if (normalized.md_age_ms == null && ageMs !== undefined) normalized.md_age_ms = ageMs;
  if (!normalized.exchange && exchange) normalized.exchange = exchange;
  if (!normalized.coin && coin) normalized.coin = coin;
  Object.assign(normalized, naming);
  return normalized;
}

function normalizeSignalStrategyCandidate(candidate: unknown): SignalStrategy | null {
  if (!candidate || typeof candidate !== 'object') return null;
  const raw = candidate as Record<string, unknown>;
  const meta = raw.meta && typeof raw.meta === 'object' ? (raw.meta as Record<string, unknown>) : {};
  const id = String(raw.strategy_id ?? raw.id ?? meta.strategy_id ?? '').trim();
  if (!id) return null;

  const state = raw.state && typeof raw.state === 'object' ? (raw.state as Record<string, unknown>) : {};
  const paramsRaw = raw.params && typeof raw.params === 'object' ? (raw.params as Record<string, unknown>) : {};
  const params: Record<string, string | undefined> = {};
  for (const [key, value] of Object.entries(paramsRaw)) {
    const normalizedValue =
      key === 'bot_on'
        ? (normalizeTradingFlag(value) ?? (value == null ? undefined : String(value)))
        : (value == null ? undefined : String(value));
    params[key] = normalizedValue;
  }
  if (params.bot_on === undefined) {
    params.bot_on = normalizeTradingFlag(state.bot_on);
  }

  const rawLegs = raw.legs && typeof raw.legs === 'object' ? (raw.legs as Record<string, unknown>) : {};
  const legs: Record<string, Record<string, unknown> | null> = {};
  for (const [contractId, legCandidate] of Object.entries(rawLegs)) {
    legs[contractId] = normalizeLegacySignalLeg(contractId, legCandidate);
  }

  const balancesCount = toFiniteOptionalNumber(raw.balances_count) ?? 0;
  const explicitBalancesOk = raw.balances_ok;
  const balancesOk = typeof explicitBalancesOk === 'boolean' ? explicitBalancesOk : balancesCount > 0;
  const tradeable =
    typeof raw.tradeable === 'boolean'
      ? raw.tradeable
      : normalizeTradingFlag(state.bot_on) === '1';

  return {
    ...(raw as SignalStrategy),
    id,
    params,
    legs: legs as SignalStrategy['legs'],
    balances_ok: balancesOk,
    tradeable,
    blocked: typeof raw.blocked === 'boolean' ? raw.blocked : !tradeable,
  };
}

function normalizeSignalStrategiesPayload(payload: unknown): SignalStrategiesPayload {
  const data = payload && typeof payload === 'object' ? (payload as Record<string, unknown>) : {};
  const rawStrategies = Array.isArray(data.strategies) ? data.strategies : [];
  const strategies = rawStrategies
    .map((candidate) => normalizeSignalStrategyCandidate(candidate))
    .filter((row): row is SignalStrategy => Boolean(row));
  return {
    strategies,
    server_time: typeof data.server_time === 'string' ? data.server_time : undefined,
    server_ts_ms: toFiniteOptionalNumber(data.server_ts_ms),
    balance_summary: data.balance_summary as BalanceSummary | undefined,
  };
}

function normalizeTradeSide(value: unknown): string {
  const text = String(value ?? '').trim().toLowerCase();
  if (text === '1' || text === 'buy' || text === 'bid') return 'buy';
  if (text === '2' || text === 'sell' || text === 'ask') return 'sell';
  return text;
}

function normalizeTradeEventCandidate(candidate: unknown, index: number, seqSeed: number): TradeEvent | null {
  if (!candidate || typeof candidate !== 'object') return null;
  const row = candidate as Record<string, unknown>;

  const op = String(row.op ?? 'upsert').trim().toLowerCase() || 'upsert';
  const tsMs = toFiniteOptionalNumber(row.ts_ms ?? row.ts_event ?? row.ts ?? row.timestamp);
  const seq =
    toFiniteOptionalNumber(row.seq) ??
    toFiniteOptionalNumber(String(row.entry_id ?? '').split('-')[0]) ??
    tsMs ??
    (seqSeed + index);
  const version = Math.max(1, Math.trunc(toFiniteOptionalNumber(row.version) ?? 1));
  const instrumentId = String(row.instrument_id ?? '').trim();
  const symbol = String(row.symbol ?? instrumentId.split('.')[0] ?? '').trim();
  const price = toFiniteOptionalNumber(row.price);
  const qty = toFiniteOptionalNumber(row.qty);
  const derivedNotional = (price !== undefined && qty !== undefined) ? price * qty : undefined;
  const reportedNotional = toFiniteOptionalNumber(
    row.mv ??
    row.notional ??
    row.notional_quote ??
    row.notional_usd,
  );
  let notional = reportedNotional;
  if ((notional === undefined || notional === 0) && derivedNotional !== undefined && derivedNotional !== 0) {
    notional = derivedNotional;
  }
  const coin = String(
    row.coin ??
      row.asset ??
      row.base_currency ??
      deriveCoinFromSymbol(symbol),
  ).trim();
  const exchange = String(
    row.exchange ??
      row.venue ??
      deriveExchangeFromInstrument(instrumentId),
  ).trim().toLowerCase();
  const naming = deriveCanonicalNaming(row, {
    exchange,
    symbol,
    asset: coin,
    isPosition: false,
  });
  const rowId = String(
    row.row_id ??
      row.trade_id ??
      row.client_order_id ??
      row.entry_id ??
      `${exchange}:${coin}:${seq}`,
  ).trim();
  if (!rowId) return null;

  return {
    ...(row as TradeEvent),
    op: op === 'delete' ? 'delete' : 'upsert',
    row_id: rowId,
    version,
    seq,
    ts_ms: tsMs ?? undefined,
    ts: tsMs ?? seq,
    side: normalizeTradeSide(row.side),
    coin,
    exchange,
    ...naming,
    signal_id: String(row.signal_id ?? row.strategy_id ?? '').trim(),
    order_id: String(row.order_id ?? row.client_order_id ?? '').trim(),
    time:
      typeof row.time === 'string' && row.time
        ? row.time
        : (tsMs ? new Date(tsMs).toISOString() : ''),
    mv: notional,
  } as TradeEvent;
}

function extractBulkUpdateFailures(payload: unknown): Array<{ strategy_id: string; message: string }> {
  if (!payload || typeof payload !== 'object') return [];
  const data = payload as Record<string, unknown>;
  const errorsRaw = Array.isArray(data.errors) ? data.errors : [];
  const fromErrors = errorsRaw
    .map((entry) => {
      if (!entry || typeof entry !== 'object') return null;
      const row = entry as Record<string, unknown>;
      const strategyId = String(row.strategy_id ?? '').trim();
      const message = extractFluxErrorMessage(row.error ?? row.message ?? row.code) ?? 'update_failed';
      return { strategy_id: strategyId, message };
    })
    .filter((entry): entry is { strategy_id: string; message: string } => Boolean(entry && entry.message));
  if (fromErrors.length > 0) return fromErrors;

  const failedRaw = Array.isArray(data.failed) ? data.failed : [];
  return failedRaw
    .map((entry) => String(entry ?? '').trim())
    .filter(Boolean)
    .map((strategy_id) => ({ strategy_id, message: 'update_failed' }));
}

function attachAlertsPaginationMetadata(
  rows: Alert[],
  payload: Record<string, unknown> | null | undefined,
): Alert[] {
  const out = [...rows] as Alert[] & {
    total?: number;
    limit?: number;
    offset?: number;
    has_more?: boolean;
    next_offset?: number | null;
    next_cursor?: string | null;
  };
  if (!payload || typeof payload !== 'object') {
    return out;
  }
  const total = toFiniteOptionalNumber(payload.total);
  const limit = toFiniteOptionalNumber(payload.limit);
  const offset = toFiniteOptionalNumber(payload.offset);
  const nextOffset = toFiniteOptionalNumber(payload.next_offset);
  const nextCursor =
    typeof payload.next_cursor === 'string'
      ? payload.next_cursor
      : payload.next_cursor == null
        ? null
        : undefined;
  if (total !== undefined) out.total = total;
  if (limit !== undefined) out.limit = limit;
  if (offset !== undefined) out.offset = offset;
  if (typeof payload.has_more === 'boolean') out.has_more = payload.has_more;
  if (nextOffset !== undefined) out.next_offset = nextOffset;
  if (nextCursor !== undefined) out.next_cursor = nextCursor;
  return out;
}

function normalizeAlertRow(candidate: unknown): Alert | null {
  if (!candidate || typeof candidate !== 'object') return null;
  const row = candidate as Record<string, unknown>;
  const id = String(row.id ?? row.row_id ?? row.entry_id ?? '').trim();
  if (!id) return null;

  const severityRaw = String(row.severity ?? row.level ?? 'INFO').trim().toUpperCase();
  const level = (
    severityRaw === 'CRITICAL'
      || severityRaw === 'ERROR'
      || severityRaw === 'WARNING'
  )
    ? severityRaw
    : 'INFO';
  const tsMsCandidate = toFiniteOptionalNumber(row.ts_ms ?? row.ts_event);
  const timestamp = Math.floor(
    toFiniteNumber(
      row.timestamp
        ?? row.ts
        ?? (
          tsMsCandidate == null
            ? undefined
            : (tsMsCandidate >= 1_000_000_000_000 ? tsMsCandidate / 1000 : tsMsCandidate)
        )
        ?? Date.parse(String(row.time ?? '')) / 1000,
      0,
    ),
  );
  const safeTimestamp = timestamp > 0 ? timestamp : Math.floor(Date.now() / 1000);
  const message = String(row.message ?? row.title ?? id).trim();
  const details = row.details && typeof row.details === 'object' ? (row.details as Record<string, unknown>) : {};
  const strategyId = String(row.strategy_id ?? row.strategy ?? row.signal_id ?? '').trim();

  return {
    ...(row as Alert),
    id,
    level: level as Alert['level'],
    severity: row.severity != null ? (level as Alert['severity']) : undefined,
    strategy_id: strategyId || undefined,
    timestamp: safeTimestamp,
    message: message || id,
    details,
  };
}

function parseAlertItemCandidate(candidate: unknown): unknown {
  if (typeof candidate !== 'string') {
    return candidate;
  }
  const text = candidate.trim();
  if (!text) return null;
  try {
    return JSON.parse(text);
  } catch {
    return null;
  }
}

export function normalizeAlertsSnapshotCandidate(payload: unknown): Alert[] {
  if (Array.isArray(payload)) {
    return payload
      .map((item) => parseAlertItemCandidate(item))
      .map((row) => normalizeAlertRow(row))
      .filter((row): row is Alert => Boolean(row));
  }
  if (!payload || typeof payload !== 'object') {
    return [];
  }

  const data = payload as Record<string, unknown>;
  const rawRows = Array.isArray(data.rows)
    ? data.rows
    : (Array.isArray(data.alerts) ? data.alerts : []);
  return rawRows
    .map((item) => parseAlertItemCandidate(item))
    .map((row) => normalizeAlertRow(row))
    .filter((row): row is Alert => Boolean(row));
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

async function fetchParamsJSON<T>(path: string, init?: RequestInit): Promise<T> {
  try {
    const response = await fetch(`${base}${path}`, init);
    let payload: unknown = null;
    if (typeof response.json === 'function') {
      payload = await response.json();
    }
    if (!response.ok) {
      const detail = extractFluxErrorMessage(payload);
      throw new Error(detail || `${response.status} ${response.statusText}`);
    }
    return payload as T;
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

  setHedgerGeometryOverridesById: async (
    hedgerId: string,
    overrides: HedgerGeometryOverrides
  ): Promise<HedgerGeometryResponse> => {
    const headers = await signedJsonHeaders(overrides);
    const response = await apiClient.fetchJSON<FluxEnvelope<HedgerGeometryResponse>>(
      `/api/v1/hedgers/${encodeURIComponent(hedgerId)}/geometry-overrides`,
      {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', ...headers },
        body: JSON.stringify(overrides),
      }
    );
    return unwrapFluxEnvelope(response);
  },

  clearHedgerGeometryOverridesById: async (hedgerId: string): Promise<HedgerGeometryResponse> => {
    const headers = await signedJsonHeaders({});
    const response = await apiClient.fetchJSON<FluxEnvelope<HedgerGeometryResponse>>(
      `/api/v1/hedgers/${encodeURIComponent(hedgerId)}/geometry-overrides`,
      {
        method: 'DELETE',
        headers: { 'Content-Type': 'application/json', ...headers },
      }
    );
    return unwrapFluxEnvelope(response);
  },

  setHedgerThresholdOverridesById: async (
    hedgerId: string,
    overrides: HedgerThresholdOverrides
  ): Promise<HedgerThresholdResponse> => {
    const headers = await signedJsonHeaders(overrides);
    const response = await apiClient.fetchJSON<FluxEnvelope<HedgerThresholdResponse>>(
      `/api/v1/hedgers/${encodeURIComponent(hedgerId)}/threshold-overrides`,
      {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', ...headers },
        body: JSON.stringify(overrides),
      }
    );
    return unwrapFluxEnvelope(response);
  },

  clearHedgerThresholdOverridesById: async (hedgerId: string): Promise<HedgerThresholdResponse> => {
    const headers = await signedJsonHeaders({});
    const response = await apiClient.fetchJSON<FluxEnvelope<HedgerThresholdResponse>>(
      `/api/v1/hedgers/${encodeURIComponent(hedgerId)}/threshold-overrides`,
      {
        method: 'DELETE',
        headers: { 'Content-Type': 'application/json', ...headers },
      }
    );
    return unwrapFluxEnvelope(response);
  },

  setHedgerEnabledById: async (
    hedgerId: string,
    enabled: boolean
  ): Promise<{ hedger_enabled: boolean }> => {
    const payload = { enabled };
    const headers = await signedJsonHeaders(payload);
    const response = await apiClient.fetchJSON<FluxEnvelope<{ hedger_enabled: boolean }>>(
      `/api/v1/hedgers/${encodeURIComponent(hedgerId)}/enabled`,
      {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', ...headers },
        body: JSON.stringify(payload),
      }
    );
    return unwrapFluxEnvelope(response);
  },

  clearHedgerEventsById: async (hedgerId: string): Promise<{ cleared: number }> => {
    const headers = await signedJsonHeaders({});
    const response = await apiClient.fetchJSON<FluxEnvelope<{ cleared: number }>>(
      `/api/v1/hedgers/${encodeURIComponent(hedgerId)}/events/clear`,
      {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', ...headers },
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
    return api.setHedgerGeometryOverridesById('eth_plume_lp', overrides);
  },

  clearHedgerGeometryOverrides: async (): Promise<HedgerGeometryResponse> => {
    return api.clearHedgerGeometryOverridesById('eth_plume_lp');
  },

  setHedgerThresholdOverrides: async (
    overrides: HedgerThresholdOverrides
  ): Promise<HedgerThresholdResponse> => {
    return api.setHedgerThresholdOverridesById('eth_plume_lp', overrides);
  },

  clearHedgerThresholdOverrides: async (): Promise<HedgerThresholdResponse> => {
    return api.clearHedgerThresholdOverridesById('eth_plume_lp');
  },

  setHedgerEnabled: async (enabled: boolean): Promise<{ hedger_enabled: boolean }> => {
    return api.setHedgerEnabledById('eth_plume_lp', enabled);
  },

  setHedgerBand2GeometryOverrides: async (
    overrides: HedgerGeometryOverrides
  ): Promise<HedgerGeometryResponse> => {
    return api.setHedgerGeometryOverridesById('eth_plume_lp_band2', overrides);
  },

  clearHedgerBand2GeometryOverrides: async (): Promise<HedgerGeometryResponse> => {
    return api.clearHedgerGeometryOverridesById('eth_plume_lp_band2');
  },

  setHedgerBand2ThresholdOverrides: async (
    overrides: HedgerThresholdOverrides
  ): Promise<HedgerThresholdResponse> => {
    return api.setHedgerThresholdOverridesById('eth_plume_lp_band2', overrides);
  },

  clearHedgerBand2ThresholdOverrides: async (): Promise<HedgerThresholdResponse> => {
    return api.clearHedgerThresholdOverridesById('eth_plume_lp_band2');
  },

  setHedgerBand2Enabled: async (enabled: boolean): Promise<{ hedger_enabled: boolean }> => {
    return api.setHedgerEnabledById('eth_plume_lp_band2', enabled);
  },

  clearHedgerEvents: async (): Promise<{ cleared: number }> => {
    return api.clearHedgerEventsById('eth_plume_lp');
  },

  clearHedgerBand2Events: async (): Promise<{ cleared: number }> => {
    return api.clearHedgerEventsById('eth_plume_lp_band2');
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
    const MAX_TRADES_PAGE_SIZE = 200;
    const normalizedPage = Number.isFinite(page) ? Math.max(page, 1) : 1;
    const normalizedPageSize = Number.isFinite(pageSize)
      ? Math.min(Math.max(pageSize, 1), MAX_TRADES_PAGE_SIZE)
      : 1;
    const limit = normalizedPageSize;
    const offset = (normalizedPage - 1) * normalizedPageSize;
    const cursorParam = typeof params.cursor === 'string' && params.cursor ? params.cursor : null;
    const qs = new URLSearchParams({
      limit: String(limit),
      offset: String(offset),
      sort: normalizeTradesSortParam(params.sort as string | undefined),
      coin: (params.coin as string) || '',
      market_type: (params.market_type as string) || '',
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
    const rows = (data.rows || [])
      .map((row, index) => normalizeTradeEventCandidate(row, index, resolvedOffset + 1))
      .filter((row): row is TradeEvent => Boolean(row));
    const returned = rows.length;
    const totalCount = data.total ?? data.total_records ?? 0;
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
    cursor: TradesDeltaCursor,
    limit = 2000,
    init?: RequestInit
  ): Promise<{ rows: TradeEvent[]; last_seq?: number; reset_required?: boolean }> => {
    const resolvedCursor =
      typeof cursor === 'number'
        ? { sinceSeq: cursor }
        : (cursor ?? {});
    const qs = new URLSearchParams({
      limit: String(limit),
    });
    const afterMs =
      typeof resolvedCursor.afterMs === 'number' && Number.isFinite(resolvedCursor.afterMs)
        ? Math.max(0, Math.trunc(resolvedCursor.afterMs))
        : undefined;
    const afterRowId =
      typeof resolvedCursor.afterRowId === 'string' ? resolvedCursor.afterRowId.trim() : '';
    const afterVersion =
      typeof resolvedCursor.afterVersion === 'number' && Number.isFinite(resolvedCursor.afterVersion)
        ? Math.max(1, Math.trunc(resolvedCursor.afterVersion))
        : undefined;
    const sinceSeq =
      typeof resolvedCursor.sinceSeq === 'number' && Number.isFinite(resolvedCursor.sinceSeq)
        ? Math.max(0, Math.trunc(resolvedCursor.sinceSeq))
        : 0;
    if (afterMs !== undefined) {
      qs.set('after', String(afterMs));
      if (afterRowId) {
        qs.set('after_row_id', afterRowId);
        qs.set('after_version', String(afterVersion ?? 1));
      }
    } else {
      qs.set('since_seq', String(sinceSeq));
    }
    appendProfileQuery(qs);
    const r = await fetchJSON<FluxEnvelope<{ rows: TradeEvent[]; last_seq?: number; reset_required?: boolean }>>(`/api/v1/trades/delta?${qs.toString()}`, init);
    const data = unwrapFluxEnvelope(r);
    const rows = (data.rows || [])
      .map((row, index) => normalizeTradeEventCandidate(row, index, sinceSeq + index + 1))
      .filter((row): row is TradeEvent => Boolean(row));
    const maxSeq = rows.reduce((acc, row) => Math.max(acc, Number(row.seq) || 0), 0);
    return {
      rows,
      last_seq: typeof data.last_seq === 'number' ? data.last_seq : (maxSeq > 0 ? maxSeq : sinceSeq),
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
    const rows = normalizeBalancesRows(payload.rows);
    const totalMv = rows.reduce((sum, row) => sum + (row.mv_raw ?? 0), 0);
    const generatedAt =
      typeof payload.generated_at === 'string' && payload.generated_at
        ? payload.generated_at
        : new Date(
            coerceTimestampMs((payload as Record<string, unknown>).server_ts_ms) ?? Date.now(),
          ).toISOString();
    return {
      ...payload,
      rows,
      total: typeof payload.total === 'number' ? payload.total : toFiniteNumber((payload as Record<string, unknown>).count, rows.length),
      totals: payload.totals ?? { mv_raw: totalMv, mv_display: formatMoneyDisplay(totalMv) },
      generated_at: generatedAt,
      view: payload.view ?? 'parents_only',
      risk_groups: normalizeRiskGroups(payload.risk_groups),
    };
  },

  // Signal strategies - FluxAPI v1 returns {"ok": true, "data": {"strategies": [...], "server_time": "..."}}
  getSignalStrategies: async (): Promise<SignalStrategiesPayload> => {
    const qs = new URLSearchParams();
    appendProfileQuery(qs);
    const response = await fetchJSON<{ ok: boolean; data: SignalStrategiesPayload }>(
      `/api/v1/signals${qs.toString() ? `?${qs.toString()}` : ''}`
    );
    return normalizeSignalStrategiesPayload(response.data);
  },

  // Signal strategies (FluxAPI v1) - Returns {"ok": true, "data": {"strategies": [...], "server_time": "..."}}
  getSignals: async (): Promise<SignalStrategiesPayload> => {
    const qs = new URLSearchParams();
    appendProfileQuery(qs);
    const response = await fetchJSON<{ ok: boolean; data: SignalStrategiesPayload }>(
      `/api/v1/signals${qs.toString() ? `?${qs.toString()}` : ''}`
    );
    return normalizeSignalStrategiesPayload(response.data);
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
    const path = buildProfileScopedPath(`/api/v1/strategies/${id}/parameters`);
    const response = await fetchJSON<FluxEnvelope<{ params?: Record<string, any>; parameters?: Record<string, any> }>>(path);
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
      const path = buildProfileScopedPath(BULK_PARAMS_PATH);
      const extra = await signedJsonHeaders(payload, {
        method: 'PATCH',
        path,
      });
      const result = await fetchParamsJSON<FluxEnvelope<import('./types').BulkUpdateResult>>(path, {
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
    const path = buildProfileScopedPath(BULK_PARAMS_PATH);
    const extra = await signedJsonHeaders(payload, {
      method: 'PATCH',
      path,
    });
    const response = await fetchParamsJSON<FluxEnvelope<import('./types').BulkUpdateResult>>(path, {
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
    const path = buildProfileScopedPath(BULK_PARAMS_PATH);
    const extra = await signedJsonHeaders(payload, {
      method: 'PATCH',
      path,
    });
    const response = await fetchParamsJSON<FluxEnvelope<import('./types').BulkUpdateResult>>(path, {
      method: 'PATCH',
      headers: { 'Content-Type': 'application/json', ...extra },
      body: JSON.stringify(payload)
    });
    const result = unwrapFluxEnvelope(response);
    const failures = extractBulkUpdateFailures(result);
    if (failures.length > 0) {
      const detail = failures
        .map((entry) => (entry.strategy_id ? `${entry.strategy_id}: ${entry.message}` : entry.message))
        .join('; ');
      throw new Error(detail || 'Parameter update failed');
    }
    return { ok: true } as const;
  },

  // Get parameter schema with validation rules
  getParamSchema: async (options?: { preferKeyLabel?: boolean }) => {
    const qs = new URLSearchParams();
    appendProfileQuery(qs);
    const response = await fetchJSON<FluxEnvelope<ParamSchema>>(
      `/api/v1/param-schema${qs.toString() ? `?${qs.toString()}` : ''}`
    );
    const payload = unwrapFluxEnvelope(response);
    const activeProfile = getActivePathProfile();
    const preferKeyLabel = options?.preferKeyLabel ?? routePrefersKeyLabel(activeProfile);
    return normalizeParamSchemaPayloadWithOptions(payload, {
      // TokenMM operators need compact param-key headers to keep the grid readable.
      preferKeyLabel,
    });
  },

  // Get all strategy parameters in bulk
  getParams: async () => {
    const qs = new URLSearchParams();
    appendProfileQuery(qs);
    const response = await fetchJSON<FluxEnvelope<ParamsResponse[]>>(
      `/api/v1/params${qs.toString() ? `?${qs.toString()}` : ''}`
    );
    const rows = unwrapFluxEnvelope(response) || [];
    if (!Array.isArray(rows)) return [];
    return rows.map((row) => {
      const candidate = row as Record<string, unknown>;
      const strategyId = String(candidate.strategy_id ?? '').trim();
      const schema = candidate.schema && typeof candidate.schema === 'object'
        ? (candidate.schema as Record<string, unknown>)
        : null;
      const params = normalizeParamsMap(candidate.params, schema);
      const runningCandidate = candidate.running;
      const runningFlag = normalizeTradingFlag(runningCandidate);
      const running =
        typeof runningCandidate === 'boolean'
          ? runningCandidate
          : runningFlag != null
            ? runningFlag === '1'
            : null;
      return {
        ...(row as ParamsResponse),
        strategy_id: strategyId,
        params,
        running,
      };
    }).filter((row) => Boolean(row.strategy_id));
  },

  // Bulk update multiple strategies
  updateParams: async (
    updates: import('./types').ParamUpdate[],
    source = 'fluxboard'
  ) => {
    const payload = { updates, source };
    const path = buildProfileScopedPath(BULK_PARAMS_PATH);
    const extra = await signedJsonHeaders(payload, { method: 'PATCH', path });
    const response = await fetchParamsJSON<FluxEnvelope<import('./types').BulkUpdateResult>>(path, {
      method: 'PATCH',
      headers: { 'Content-Type': 'application/json', ...extra },
      body: JSON.stringify(payload)
    });
    return unwrapFluxEnvelope(response);
  },

  // Get config files for a strategy
  getStrategyConfig: async (id: string) => {
    const path = buildProfileScopedPath(`/api/v1/strategies/${id}/config-files`);
    const response = await fetchJSON<import('./types').ConfigResponse | FluxEnvelope<import('./types').ConfigResponse>>(path);
    return unwrapFluxEnvelope(response);
  },

  // Alerts - Flask returns {"alerts": [...]}
  getAlerts: async (): Promise<Alert[]> => {
    const qs = new URLSearchParams();
    appendProfileQuery(qs);
    const response = await fetchJSON<FluxEnvelope<{
      rows?: Alert[];
      total?: number;
      limit?: number;
      offset?: number;
      has_more?: boolean;
      next_offset?: number | null;
      next_cursor?: string | null;
    }>>(
      `/api/v1/alerts${qs.toString() ? `?${qs.toString()}` : ''}`
    );
    const payload = unwrapFluxEnvelope(response);
    if (Array.isArray(payload)) {
      return normalizeAlertsSnapshotCandidate(payload);
    }
    const data = payload && typeof payload === 'object'
      ? (payload as Record<string, unknown>)
      : null;
    const rows = normalizeAlertsSnapshotCandidate(data);
    return attachAlertsPaginationMetadata(rows, data);
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
