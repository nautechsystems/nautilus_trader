import type { SignalLeg, SignalStrategy } from '../types';
import { getLegForSlot } from './signalLegs';

export type StrategyAgeInfo = {
  displayAgeMs: number;
  recentAgeMs: number;
  perLeg: {
    A?: LegAgeInfo;
    B?: LegAgeInfo;
  };
  mostRecentTsMs?: number;
  mostRecentSide?: 'A' | 'B';
};

export type LegAgeInfo = {
  ageMs: number;
  tsMs?: number;
  source: 'md_ts_ms' | 'md_age_ms' | 'update_ts_ms' | 'update_time' | 'missing';
};

const MISSING_AGE_MS = 999_999;
const mdAgeTsCache = new WeakMap<SignalLeg, { mdAgeMs: number; tsMs: number }>();

function parseUpdateTime(value?: string): number | undefined {
  if (!value) return undefined;
  const iso = value.includes('T') ? value : value.replace(' ', 'T');
  const ts = Date.parse(iso.endsWith('Z') ? iso : `${iso}Z`);
  return Number.isNaN(ts) ? undefined : ts;
}

function resolveMdAgeTs(leg: SignalLeg, mdAgeMs: number, nowMs: number): number {
  const cached = mdAgeTsCache.get(leg);
  if (cached && cached.mdAgeMs === mdAgeMs) {
    return cached.tsMs;
  }

  const tsMs = nowMs - mdAgeMs;
  mdAgeTsCache.set(leg, { mdAgeMs, tsMs });
  return tsMs;
}

function resolveLegAge(leg: SignalLeg | null | undefined, nowMs: number): LegAgeInfo {
  if (!leg) {
    return { ageMs: MISSING_AGE_MS, tsMs: undefined, source: 'missing' };
  }

  if (typeof leg.md_ts_ms === 'number' && Number.isFinite(leg.md_ts_ms)) {
    return { ageMs: Math.max(0, nowMs - leg.md_ts_ms), tsMs: leg.md_ts_ms, source: 'md_ts_ms' };
  }

  if (typeof leg.md_age_ms === 'number' && Number.isFinite(leg.md_age_ms)) {
    const mdAgeMs = Math.max(0, leg.md_age_ms);
    const tsMs = resolveMdAgeTs(leg, mdAgeMs, nowMs);
    return { ageMs: Math.max(0, nowMs - tsMs), tsMs, source: 'md_age_ms' };
  }

  if (typeof leg.update_ts_ms === 'number' && Number.isFinite(leg.update_ts_ms)) {
    return { ageMs: Math.max(0, nowMs - leg.update_ts_ms), tsMs: leg.update_ts_ms, source: 'update_ts_ms' };
  }

  const parsed = parseUpdateTime(leg.update_time);
  if (parsed !== undefined) {
    return { ageMs: Math.max(0, nowMs - parsed), tsMs: parsed, source: 'update_time' };
  }

  return { ageMs: MISSING_AGE_MS, tsMs: undefined, source: 'missing' };
}

export function computeStrategyAge(strategy: SignalStrategy, serverNowMs?: number): StrategyAgeInfo {
  const nowMs = typeof serverNowMs === 'number' && Number.isFinite(serverNowMs) ? serverNowMs : Date.now();

  const legA = resolveLegAge(getLegForSlot(strategy, 'A'), nowMs);
  const legB = resolveLegAge(getLegForSlot(strategy, 'B'), nowMs);

  const ages = [legA.ageMs, legB.ageMs];
  const displayAgeMs = Math.max(...ages);
  const recentAgeMs = Math.min(...ages);
  const mostRecentSide = legA.ageMs <= legB.ageMs ? 'A' : 'B';
  const mostRecentTsMs = mostRecentSide === 'A' ? legA.tsMs : legB.tsMs;

  return {
    displayAgeMs,
    recentAgeMs,
    perLeg: { A: legA, B: legB },
    mostRecentSide,
    mostRecentTsMs,
  };
}
