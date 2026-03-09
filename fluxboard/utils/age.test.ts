import { describe, it, expect } from 'vitest';
import { computeStrategyAge } from './age';
import type { SignalStrategy } from '../types';

describe('computeStrategyAge', () => {
  const baseStrategy = {
    id: 's1',
    params: {},
    legs: { A: null, B: null },
    balances_ok: true,
  } as unknown as SignalStrategy;

  it('prefers md_ts_ms over md_age_ms and returns max leg staleness as displayAgeMs', () => {
    const strategy: SignalStrategy = {
      ...baseStrategy,
      legs: {
        A: { md_ts_ms: 9_900 },
        B: { md_ts_ms: 9_950 },
      },
    } as SignalStrategy;

    const result = computeStrategyAge(strategy, 10_000);

    expect(result.displayAgeMs).toBe(100);
    expect(result.perLeg.A?.ageMs).toBe(100);
    expect(result.perLeg.B?.ageMs).toBe(50);
    expect(result.recentAgeMs).toBe(50);
    expect(result.mostRecentTsMs).toBe(9_950);
    expect(result.mostRecentSide).toBe('B');
  });

  it('falls back to md_age_ms when md_ts_ms is absent', () => {
    const strategy: SignalStrategy = {
      ...baseStrategy,
      legs: {
        A: { md_age_ms: 2_000 },
        B: { md_ts_ms: 9_000 },
      },
    } as SignalStrategy;

    const result = computeStrategyAge(strategy, 10_000);

    expect(result.displayAgeMs).toBe(2_000);
    expect(result.perLeg.A?.ageMs).toBe(2_000);
    expect(result.perLeg.A?.tsMs).toBe(8_000);
    expect(result.perLeg.B?.ageMs).toBe(1_000);
    expect(result.recentAgeMs).toBe(1_000);
    expect(result.mostRecentSide).toBe('B');
  });

  it('uses expected precedence across mixed timestamp inputs', () => {
    const nowMs = 10_000;

    const withMdTs: SignalStrategy = {
      ...baseStrategy,
      legs: {
        A: {
          md_ts_ms: 9_000,
          md_age_ms: 2_000,
          update_ts_ms: 8_500,
          update_time: '1970-01-01 00:00:08',
        },
        B: null,
      },
    } as SignalStrategy;
    const mdTsResult = computeStrategyAge(withMdTs, nowMs);
    expect(mdTsResult.perLeg.A?.source).toBe('md_ts_ms');
    expect(mdTsResult.perLeg.A?.ageMs).toBe(1_000);
    expect(mdTsResult.perLeg.A?.tsMs).toBe(9_000);

    const withMdAge: SignalStrategy = {
      ...baseStrategy,
      legs: {
        A: {
          md_age_ms: 2_000,
          update_ts_ms: 8_500,
          update_time: '1970-01-01 00:00:08',
        },
        B: null,
      },
    } as SignalStrategy;
    const mdAgeResult = computeStrategyAge(withMdAge, nowMs);
    expect(mdAgeResult.perLeg.A?.source).toBe('md_age_ms');
    expect(mdAgeResult.perLeg.A?.ageMs).toBe(2_000);
    expect(mdAgeResult.perLeg.A?.tsMs).toBe(8_000);

    const withUpdateTs: SignalStrategy = {
      ...baseStrategy,
      legs: {
        A: {
          update_ts_ms: 8_500,
          update_time: '1970-01-01 00:00:08',
        },
        B: null,
      },
    } as SignalStrategy;
    const updateTsResult = computeStrategyAge(withUpdateTs, nowMs);
    expect(updateTsResult.perLeg.A?.source).toBe('update_ts_ms');
    expect(updateTsResult.perLeg.A?.ageMs).toBe(1_500);
    expect(updateTsResult.perLeg.A?.tsMs).toBe(8_500);

    const withUpdateTime: SignalStrategy = {
      ...baseStrategy,
      legs: {
        A: {
          update_time: '1970-01-01 00:00:08',
        },
        B: null,
      },
    } as SignalStrategy;
    const updateTimeResult = computeStrategyAge(withUpdateTime, nowMs);
    expect(updateTimeResult.perLeg.A?.source).toBe('update_time');
    expect(updateTimeResult.perLeg.A?.ageMs).toBe(2_000);
    expect(updateTimeResult.perLeg.A?.tsMs).toBe(8_000);
  });

  it('keeps md_age_ms-derived timestamp stable so age advances with now', () => {
    const strategy: SignalStrategy = {
      ...baseStrategy,
      legs: {
        A: { md_age_ms: 2_000 },
        B: { update_ts_ms: 9_500 },
      },
    } as SignalStrategy;

    const first = computeStrategyAge(strategy, 10_000);
    expect(first.perLeg.A?.source).toBe('md_age_ms');
    expect(first.perLeg.A?.tsMs).toBe(8_000);
    expect(first.perLeg.A?.ageMs).toBe(2_000);
    expect(first.perLeg.B?.ageMs).toBe(500);

    const second = computeStrategyAge(strategy, 11_000);
    expect(second.perLeg.A?.source).toBe('md_age_ms');
    expect(second.perLeg.A?.tsMs).toBe(8_000);
    expect(second.perLeg.A?.ageMs).toBe(3_000);
    expect(second.perLeg.B?.ageMs).toBe(1_500);
  });

  it('uses legs_order before lexical key ordering', () => {
    const strategy: SignalStrategy = {
      ...baseStrategy,
      legs_order: ['contract_z', 'contract_a'],
      legs: {
        contract_a: { md_ts_ms: 9_900 },
        contract_z: { md_ts_ms: 9_000 },
      },
    } as SignalStrategy;

    const result = computeStrategyAge(strategy, 10_000);

    // Slot A follows legs_order (contract_z), slot B is contract_a.
    expect(result.perLeg.A?.ageMs).toBe(1_000);
    expect(result.perLeg.B?.ageMs).toBe(100);
    expect(result.mostRecentSide).toBe('B');
    expect(result.mostRecentTsMs).toBe(9_900);
  });

  it('falls back to lexical key ordering when legs_order is absent', () => {
    const strategy: SignalStrategy = {
      ...baseStrategy,
      legs: {
        contract_z: { md_ts_ms: 9_000 },
        contract_a: { md_ts_ms: 9_900 },
      },
    } as SignalStrategy;

    const result = computeStrategyAge(strategy, 10_000);

    // Lexical order: contract_a first (slot A), contract_z second (slot B).
    expect(result.perLeg.A?.ageMs).toBe(100);
    expect(result.perLeg.B?.ageMs).toBe(1_000);
    expect(result.mostRecentSide).toBe('A');
    expect(result.mostRecentTsMs).toBe(9_900);
  });
});
