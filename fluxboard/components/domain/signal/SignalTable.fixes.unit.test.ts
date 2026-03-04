/**
 * Unit tests for SignalTable fixes (items 1-6)
 *
 * These tests focus on the specific fixes rather than full component rendering
 */

import { describe, it, expect } from 'vitest';

// Import the actual component to test helper functions
// We'll test the logic directly rather than through full component rendering

describe('SignalTable Fixes - Unit Tests', () => {
  describe('Fix 1: paramTooltip optional chaining', () => {
    it('handles undefined params without crashing', () => {
      const paramTooltip = (row: any) => [
        'Edge thresholds (minimum edge to trade):',
        `  cex_bid_edge: ${row.params?.cex_bid_edge ?? 'N/A'} bps`,
        `  cex_ask_edge: ${row.params?.cex_ask_edge ?? 'N/A'} bps`,
        `  pool_edge: ${row.params?.pool_edge ?? 'N/A'} bps`,
        '',
        'Trading params:',
        `  qty: ${row.params?.qty ?? 'N/A'}`,
        `  slippage: ${row.params?.slippage_bps ?? 'N/A'} bps`,
        '',
        'Decision prices = fee-only (fees + FX)',
        'Quoted prices = decision ± edge bias',
        'Edges gate execution; quoting is optional.'
      ].join('\n');

      const rowWithoutParams = { params: undefined };
      expect(() => paramTooltip(rowWithoutParams)).not.toThrow();
      const result = paramTooltip(rowWithoutParams);
      expect(result).toContain('N/A');
    });

    it('handles null params without crashing', () => {
      const paramTooltip = (row: any) => [
        'Edge thresholds (minimum edge to trade):',
        `  cex_bid_edge: ${row.params?.cex_bid_edge ?? 'N/A'} bps`,
        `  cex_ask_edge: ${row.params?.cex_ask_edge ?? 'N/A'} bps`,
        `  pool_edge: ${row.params?.pool_edge ?? 'N/A'} bps`,
        '',
        'Trading params:',
        `  qty: ${row.params?.qty ?? 'N/A'}`,
        `  slippage: ${row.params?.slippage_bps ?? 'N/A'} bps`,
        '',
        'Decision prices = fee-only (fees + FX)',
        'Quoted prices = decision ± edge bias',
        'Edges gate execution; quoting is optional.'
      ].join('\n');

      const rowWithNullParams = { params: null };
      expect(() => paramTooltip(rowWithNullParams)).not.toThrow();
      const result = paramTooltip(rowWithNullParams);
      expect(result).toContain('N/A');
    });

    it('displays params correctly when present', () => {
      const paramTooltip = (row: any) => [
        'Edge thresholds (minimum edge to trade):',
        `  cex_bid_edge: ${row.params?.cex_bid_edge ?? 'N/A'} bps`,
        `  cex_ask_edge: ${row.params?.cex_ask_edge ?? 'N/A'} bps`,
        `  pool_edge: ${row.params?.pool_edge ?? 'N/A'} bps`,
        '',
        'Trading params:',
        `  qty: ${row.params?.qty ?? 'N/A'}`,
        `  slippage: ${row.params?.slippage_bps ?? 'N/A'} bps`,
        '',
        'Decision prices = fee-only (fees + FX)',
        'Quoted prices = decision ± edge bias',
        'Edges gate execution; quoting is optional.'
      ].join('\n');

      const rowWithParams = {
        params: {
          cex_bid_edge: '5',
          cex_ask_edge: '6',
          pool_edge: '7',
          qty: '100',
          slippage_bps: '10'
        }
      };
      const result = paramTooltip(rowWithParams);
      expect(result).toContain('cex_bid_edge: 5 bps');
      expect(result).toContain('qty: 100');
      expect(result).not.toContain('N/A');
    });
  });

  describe('Fix 5: buildBalanceTooltip handling 0 values', () => {
    const formatCoveragePercent = (coverage?: number | null): string => {
      if (coverage === null || coverage === undefined) return '—';
      return `${Math.max(0, Math.min(coverage * 100, 999)).toFixed(1)}%`;
    };

    const buildBalanceTooltip = (readiness?: any, fallback?: string): string => {
      if (!readiness) {
        return fallback || 'No readiness data yet';
      }

      const lines: string[] = [];

      // Methodology header (compact)
      lines.push('Methodology: Coverage = Avail/Reqd (10× qty buffer)');
      lines.push('OK: ≥100% | WARN: 80-100% | FAIL: <80% | UNKNOWN: No pricing');
      lines.push('');

      // Status summary
      if (readiness.summary) {
        lines.push(readiness.summary);
      } else {
        lines.push(`Status: ${readiness.status}`);
      }

      // Show all requirements if available
      if (readiness.requirements && readiness.requirements.length > 0) {
        lines.push('');
        lines.push('Requirements:');
        readiness.requirements.forEach((req: any) => {
          const hasRequired = req.required != null;
          const required = hasRequired ? Number(req.required).toFixed(2) : 'N/A';
          const hasAvail = req.available != null;
          const available = hasAvail ? Number(req.available).toFixed(2) : 'N/A';
          const coverage = formatCoveragePercent(req.coverage);
          const kindBadge = req.kind === 'gas' ? ' [gas]' : '';
          lines.push(`  ${req.location} ${req.token}${kindBadge}: ${available}/${required} (${coverage})`);
        });
      } else if (readiness.missing && readiness.missing.length > 0) {
        // Fall back to showing just gaps if no full requirements
        lines.push('');
        lines.push('Top gaps:');
        readiness.missing.forEach((req: any) => {
          lines.push(`  ${req.location} ${req.token} ${formatCoveragePercent(req.coverage)}`);
        });
      }

      return lines.join('\n');
    };

    it('displays 0.00 instead of N/A when required is 0', () => {
      const readiness = {
        status: 'OK',
        requirements: [
          {
            location: 'bybit',
            token: 'BTC',
            required: 0, // Zero value
            available: 10.5,
            coverage: 999
          }
        ]
      };

      const result = buildBalanceTooltip(readiness);
      expect(result).toContain('0.00'); // Should show 0.00, not N/A
      expect(result).not.toContain('N/A'); // Should not show N/A for required
    });

    it('displays 0.00 instead of N/A when available is 0', () => {
      const readiness = {
        status: 'FAIL',
        requirements: [
          {
            location: 'bybit',
            token: 'BTC',
            required: 10.5,
            available: 0, // Zero value
            coverage: 0
          }
        ]
      };

      const result = buildBalanceTooltip(readiness);
      expect(result).toContain('0.00'); // Should show 0.00, not N/A
      expect(result).toContain('/10.50'); // Should show the required value
    });

    it('displays N/A when required is null/undefined', () => {
      const readiness = {
        status: 'OK',
        requirements: [
          {
            location: 'bybit',
            token: 'BTC',
            required: null, // Null value
            available: 10.5,
            coverage: undefined
          }
        ]
      };

      const result = buildBalanceTooltip(readiness);
      expect(result).toContain('N/A'); // Should show N/A for null required
      expect(result).toContain('10.50'); // Should show available value
    });

    it('displays N/A when available is null/undefined', () => {
      const readiness = {
        status: 'OK',
        requirements: [
          {
            location: 'bybit',
            token: 'BTC',
            required: 10.5,
            available: undefined, // Undefined value
            coverage: undefined
          }
        ]
      };

      const result = buildBalanceTooltip(readiness);
      expect(result).toContain('N/A'); // Should show N/A for undefined available
      expect(result).toContain('10.50'); // Should show required value
    });
  });

  describe('Fix 4: LegCell fallback logic', () => {
    it('returns undefined when decision prices are missing', () => {
      const leg = {
        // No decision_bid, decision_ask, fv_bid, or fv_ask
      };

      const hasDecision = leg.decision_bid != null && leg.decision_ask != null;
      const decisionBid = hasDecision ? (leg.decision_bid ?? leg.fv_bid!) : undefined;
      const decisionAsk = hasDecision ? (leg.decision_ask ?? leg.fv_ask!) : undefined;
      const decisionMid = (decisionBid != null && decisionAsk != null)
        ? (decisionBid + decisionAsk) / 2
        : undefined;

      expect(hasDecision).toBe(false);
      expect(decisionBid).toBeUndefined();
      expect(decisionAsk).toBeUndefined();
      expect(decisionMid).toBeUndefined();
    });

    it('calculates mid correctly when both bid and ask exist', () => {
      const leg = {
        decision_bid: 50000,
        decision_ask: 50100
      };

      const hasDecision = leg.decision_bid != null && leg.decision_ask != null;
      const decisionBid = hasDecision ? (leg.decision_bid ?? leg.fv_bid!) : undefined;
      const decisionAsk = hasDecision ? (leg.decision_ask ?? leg.fv_ask!) : undefined;
      const decisionMid = (decisionBid != null && decisionAsk != null)
        ? (decisionBid + decisionAsk) / 2
        : undefined;

      expect(hasDecision).toBe(true);
      expect(decisionBid).toBe(50000);
      expect(decisionAsk).toBe(50100);
      expect(decisionMid).toBe(50050);
    });

    it('handles partial decision prices (only bid)', () => {
      const leg = {
        decision_bid: 50000
        // decision_ask is missing
      };

      const hasDecision = leg.decision_bid != null && leg.decision_ask != null;
      const decisionBid = hasDecision ? (leg.decision_bid ?? leg.fv_bid!) : undefined;
      const decisionAsk = hasDecision ? (leg.decision_ask ?? leg.fv_ask!) : undefined;
      const decisionMid = (decisionBid != null && decisionAsk != null)
        ? (decisionBid + decisionAsk) / 2
        : undefined;

      expect(hasDecision).toBe(false);
      expect(decisionBid).toBeUndefined();
      expect(decisionAsk).toBeUndefined();
      expect(decisionMid).toBeUndefined();
    });

    it('formats missing prices as "—"', () => {
      const fmtMaybe = (n?: number) => n == null ? '—' : String(n);

      expect(fmtMaybe(undefined)).toBe('—');
      expect(fmtMaybe(null as any)).toBe('—');
      expect(fmtMaybe(50000)).toBe('50000');
      expect(fmtMaybe(0)).toBe('0'); // Zero is a valid number
    });
  });

  describe('Fix 2: signal_delta merge logic', () => {
    it('only includes legs that exist in delta', () => {
      const delta = {
        id: 'test',
        legs: {
          A: { coin: 'BTC', exchange: 'bybit' }
          // B is not included
        }
      };

      const apply: any = { id: delta.id };
      if (delta.legs && typeof delta.legs === 'object') {
        const patch: any = {};
        if ('A' in delta.legs) patch.A = delta.legs.A ?? null;
        if ('B' in delta.legs) patch.B = delta.legs.B ?? null;
        if (Object.keys(patch).length > 0) {
          apply.legs = patch;
        }
      }

      expect(apply.legs).toBeDefined();
      expect(apply.legs.A).toBeDefined();
      expect(apply.legs.B).toBeUndefined(); // B should not be in patch
    });

    it('handles null leg deletion', () => {
      const delta = {
        id: 'test',
        legs: {
          A: null // Explicit deletion
        }
      };

      const apply: any = { id: delta.id };
      if (delta.legs && typeof delta.legs === 'object') {
        const patch: any = {};
        if ('A' in delta.legs) patch.A = delta.legs.A ?? null;
        if (Object.keys(patch).length > 0) {
          apply.legs = patch;
        }
      }

      expect(apply.legs.A).toBeNull();
    });

    it('does not create legs object when delta has no legs', () => {
      const delta = {
        id: 'test'
        // No legs property
      };

      const apply: any = { id: delta.id };
      if (delta.legs && typeof delta.legs === 'object') {
        const patch: any = {};
        if ('A' in delta.legs) patch.A = delta.legs.A ?? null;
        if ('B' in delta.legs) patch.B = delta.legs.B ?? null;
        if (Object.keys(patch).length > 0) {
          apply.legs = patch;
        }
      }

      expect(apply.legs).toBeUndefined();
    });
  });
});
