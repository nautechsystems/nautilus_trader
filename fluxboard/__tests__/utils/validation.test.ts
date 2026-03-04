/**
 * Unit tests for validation utility.
 *
 * Tests all validation logic including:
 * - Type validation (bool, int, float, select)
 * - Range validation (min/max bounds)
 * - Edge cases (empty, whitespace, invalid formats)
 * - Error messages
 */

import { describe, it, expect, vi } from 'vitest';
import {
  validateParam,
  validateParams,
  getParamLabel,
  formatParamValue,
  getParamTooltip
} from '../../utils/validation';
import type { ParamDef, ParamSchema } from '../../types';

// Mock parameter definitions
const mockParamDefs = {
  bot_on: {
    key: 'bot_on',
    label: 'bot_on',
    description: 'Toggle strategy on (1) or off (0). When off, no new trades submitted.',
    type: 'select',
    default: '0',
    options: [['1', 'On (1)'], ['0', 'Off (0)']],
    unit: null
  } as ParamDef,

  qty: {
    key: 'qty',
    label: 'qty',
    description: 'Target base quantity per trade. Adjust to control notional size.',
    type: 'float',
    default: 1.0,
    min_value: 0.0001,
    max_value: 1000000.0,
    step: 0.0001,
    unit: 'base asset'
  } as ParamDef,

  cex_bid_edge: {
    key: 'cex_bid_edge',
    label: 'bid_edge',
    description: 'Required bid edge (bps) on CEX fills to compensate for fees.',
    type: 'float',
    default: 10.0,
    min_value: -200.0,
    max_value: 10000.0,
    step: 0.1,
    unit: 'bps'
  } as ParamDef,

  cooldown: {
    key: 'cooldown',
    label: 'cooldown',
    description: 'Post-trade cooldown in seconds before evaluating next opportunity.',
    type: 'float',
    default: 10.0,
    min_value: 0.1,
    max_value: 300.0,
    step: 0.1,
    unit: 'seconds'
  } as ParamDef,

  deadline_s: {
    key: 'deadline_s',
    label: 'dl',
    description: 'Maximum seconds a transaction can remain pending.',
    type: 'int',
    default: 90,
    min_value: 10,
    max_value: 600,
    step: 1,
    unit: 'seconds'
  } as ParamDef,

  max_errors: {
    key: 'max_errors',
    label: 'max_err',
    description: 'Maximum consecutive errors before circuit breaker trips.',
    type: 'int',
    default: 5,
    min_value: 1,
    max_value: 100,
    step: 1,
    unit: 'errors'
  } as ParamDef,

  cb_threshold: {
    key: 'cb_threshold',
    label: 'cb_thresh',
    description: 'Circuit breaker failure rate threshold (0.0-10.0).',
    type: 'float',
    default: 2.0,
    min_value: 0,
    max_value: 10,
    step: 0.01,
    unit: 'ratio'
  } as ParamDef,

  cb_window_trades: {
    key: 'cb_window_trades',
    label: 'cb_window',
    description: 'Number of recent trades used for failure rate calculation.',
    type: 'int',
    default: 20,
    min_value: 1,
    max_value: 500,
    step: 1,
    unit: 'trades'
  } as ParamDef,

  cb_cooldown_s: {
    key: 'cb_cooldown_s',
    label: 'cb_cooldown',
    description: 'Circuit breaker cooldown duration in seconds.',
    type: 'float',
    default: 10.0,
    min_value: 1.0,
    max_value: 1800.0,
    step: 0.1,
    unit: 'seconds'
  } as ParamDef
};

const mockSchema: ParamSchema = {
  params: mockParamDefs,
  deprecated: {}
};

describe('validateParam - bot_on (select type)', () => {
  it('accepts "0"', () => {
    const result = validateParam('bot_on', '0', mockParamDefs.bot_on);
    expect(result.valid).toBe(true);
  });

  it('accepts "1"', () => {
    const result = validateParam('bot_on', '1', mockParamDefs.bot_on);
    expect(result.valid).toBe(true);
  });

  it('rejects "2"', () => {
    const result = validateParam('bot_on', '2', mockParamDefs.bot_on);
    expect(result.valid).toBe(false);
    expect(result.error).toContain('must be "0"');
  });

  it('rejects "invalid"', () => {
    const result = validateParam('bot_on', 'invalid', mockParamDefs.bot_on);
    expect(result.valid).toBe(false);
    expect(result.error).toContain('must be');
  });

  it('rejects empty string', () => {
    const result = validateParam('bot_on', '', mockParamDefs.bot_on);
    expect(result.valid).toBe(false);
    expect(result.error).toContain('required');
  });
});

describe('validateParam - qty (float type)', () => {
  it('accepts valid positive number', () => {
    const result = validateParam('qty', '10.5', mockParamDefs.qty);
    expect(result.valid).toBe(true);
  });

  it('accepts minimum value (0.0001)', () => {
    const result = validateParam('qty', '0.0001', mockParamDefs.qty);
    expect(result.valid).toBe(true);
  });

  it('accepts large value', () => {
    const result = validateParam('qty', '999999', mockParamDefs.qty);
    expect(result.valid).toBe(true);
  });

  it('rejects zero', () => {
    const result = validateParam('qty', '0', mockParamDefs.qty);
    expect(result.valid).toBe(false);
    expect(result.error).toContain('must be >=');
  });

  it('rejects negative value', () => {
    const result = validateParam('qty', '-10', mockParamDefs.qty);
    expect(result.valid).toBe(false);
    expect(result.error).toContain('must be >=');
  });

  it('rejects value exceeding maximum', () => {
    const result = validateParam('qty', '9999999', mockParamDefs.qty);
    expect(result.valid).toBe(false);
    expect(result.error).toContain('must be <=');
  });

  it('rejects non-numeric value', () => {
    const result = validateParam('qty', 'abc', mockParamDefs.qty);
    expect(result.valid).toBe(false);
    expect(result.error).toContain('must be a valid number');
  });

  it('trims whitespace before validation', () => {
    const result = validateParam('qty', '  10.5  ', mockParamDefs.qty);
    expect(result.valid).toBe(true);
  });
});

describe('validateParam - cex_bid_edge (float with negative bounds)', () => {
  it('accepts positive edge', () => {
    const result = validateParam('cex_bid_edge', '10', mockParamDefs.cex_bid_edge);
    expect(result.valid).toBe(true);
  });

  it('accepts negative edge within bounds', () => {
    const result = validateParam('cex_bid_edge', '-100', mockParamDefs.cex_bid_edge);
    expect(result.valid).toBe(true);
  });

  it('accepts minimum negative edge (-200)', () => {
    const result = validateParam('cex_bid_edge', '-200', mockParamDefs.cex_bid_edge);
    expect(result.valid).toBe(true);
  });

  it('rejects edge below minimum (-201)', () => {
    const result = validateParam('cex_bid_edge', '-201', mockParamDefs.cex_bid_edge);
    expect(result.valid).toBe(false);
    expect(result.error).toContain('must be >=');
  });

  it('accepts large positive edge', () => {
    const result = validateParam('cex_bid_edge', '5000', mockParamDefs.cex_bid_edge);
    expect(result.valid).toBe(true);
  });

  it('rejects edge exceeding maximum (10001)', () => {
    const result = validateParam('cex_bid_edge', '10001', mockParamDefs.cex_bid_edge);
    expect(result.valid).toBe(false);
    expect(result.error).toContain('must be <=');
  });

  it('includes unit in error message', () => {
    const result = validateParam('cex_bid_edge', '20000', mockParamDefs.cex_bid_edge);
    expect(result.valid).toBe(false);
    expect(result.error).toContain('bps');
  });
});

describe('validateParam - cooldown (float with positive minimum)', () => {
  it('accepts valid cooldown', () => {
    const result = validateParam('cooldown', '10', mockParamDefs.cooldown);
    expect(result.valid).toBe(true);
  });

  it('accepts minimum cooldown (0.1)', () => {
    const result = validateParam('cooldown', '0.1', mockParamDefs.cooldown);
    expect(result.valid).toBe(true);
  });

  it('rejects cooldown below minimum (0.05)', () => {
    const result = validateParam('cooldown', '0.05', mockParamDefs.cooldown);
    expect(result.valid).toBe(false);
    expect(result.error).toContain('must be >=');
  });

  it('rejects negative cooldown', () => {
    const result = validateParam('cooldown', '-1', mockParamDefs.cooldown);
    expect(result.valid).toBe(false);
    expect(result.error).toContain('must be >=');
  });
});

describe('validateParam - deadline_s (int type)', () => {
  it('accepts valid integer', () => {
    const result = validateParam('deadline_s', '90', mockParamDefs.deadline_s);
    expect(result.valid).toBe(true);
  });

  it('accepts minimum value (10)', () => {
    const result = validateParam('deadline_s', '10', mockParamDefs.deadline_s);
    expect(result.valid).toBe(true);
  });

  it('rejects decimal value', () => {
    const result = validateParam('deadline_s', '90.5', mockParamDefs.deadline_s);
    expect(result.valid).toBe(false);
    expect(result.error).toContain('must be an integer');
  });

  it('rejects value below minimum', () => {
    const result = validateParam('deadline_s', '5', mockParamDefs.deadline_s);
    expect(result.valid).toBe(false);
    expect(result.error).toContain('must be >=');
  });

  it('rejects value above maximum', () => {
    const result = validateParam('deadline_s', '999', mockParamDefs.deadline_s);
    expect(result.valid).toBe(false);
    expect(result.error).toContain('must be <=');
  });
});

describe('validateParams - multiple params', () => {
  it('validates all valid params', () => {
    const params = {
      bot_on: '1',
      qty: '10.5',
      cex_bid_edge: '15.0',
      cooldown: '5.0'
    };

    const result = validateParams(params, mockSchema);
    expect(result.valid).toBe(true);
    expect(Object.keys(result.errors).length).toBe(0);
  });

  it('catches multiple invalid params', () => {
    const params = {
      bot_on: 'invalid',
      qty: '-10',
      cex_bid_edge: '99999'
    };

    const result = validateParams(params, mockSchema);
    expect(result.valid).toBe(false);
    expect(Object.keys(result.errors).length).toBe(3);
    expect(result.errors.bot_on).toBeDefined();
    expect(result.errors.qty).toBeDefined();
    expect(result.errors.cex_bid_edge).toBeDefined();
  });

  it('skips unknown parameters with warning', () => {
    const consoleWarnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});

    const params = {
      bot_on: '1',
      unknown_param: 'value'
    };

    const result = validateParams(params, mockSchema);
    expect(result.valid).toBe(true);
    expect(consoleWarnSpy).toHaveBeenCalledWith('Unknown parameter: unknown_param');

    consoleWarnSpy.mockRestore();
  });
});

describe('getParamLabel', () => {
  it('includes unit for params with unit', () => {
    const label = getParamLabel(mockParamDefs.qty);
    expect(label).toBe('qty (base asset)');
  });

  it('excludes unit for params without unit', () => {
    const label = getParamLabel(mockParamDefs.bot_on);
    expect(label).toBe('bot_on');
  });
});

describe('formatParamValue', () => {
  it('formats integer values', () => {
    const formatted = formatParamValue('90', mockParamDefs.deadline_s);
    expect(formatted).toBe('90');
  });

  it('formats float values with trimmed decimals', () => {
    const formatted = formatParamValue('10.5000', mockParamDefs.qty);
    expect(formatted).toBe('10.5');
  });

  it('formats select values with labels', () => {
    const formatted = formatParamValue('1', mockParamDefs.bot_on);
    expect(formatted).toBe('On (1)');
  });
});

describe('getParamTooltip', () => {
  it('includes first sentence and unit', () => {
    const tooltip = getParamTooltip(mockParamDefs.qty);
    expect(tooltip).toContain('Target base quantity per trade');
    expect(tooltip).toContain('(base asset)');
  });

  it('handles params without unit', () => {
    const tooltip = getParamTooltip(mockParamDefs.bot_on);
    expect(tooltip).toContain('Toggle strategy on');
    expect(tooltip).not.toContain('(undefined)');
  });

  it('does not truncate on decimal points', () => {
    const tooltip = getParamTooltip(mockParamDefs.cb_threshold);
    expect(tooltip).toContain('0.0-10.0');
  });
});

describe('edge cases', () => {
  it('handles null value', () => {
    const result = validateParam('qty', null, mockParamDefs.qty);
    expect(result.valid).toBe(false);
  });

  it('handles undefined value', () => {
    const result = validateParam('qty', undefined, mockParamDefs.qty);
    expect(result.valid).toBe(false);
  });

  it('handles very large numbers', () => {
    const result = validateParam('qty', '1e10', mockParamDefs.qty);
    expect(result.valid).toBe(false);  // Exceeds max
  });

  it('handles scientific notation within bounds', () => {
    const result = validateParam('qty', '1e2', mockParamDefs.qty);
    expect(result.valid).toBe(true);  // 100 is valid
  });

  it('handles whitespace-only value', () => {
    const result = validateParam('qty', '   ', mockParamDefs.qty);
    expect(result.valid).toBe(false);
  });

  it('handles Infinity', () => {
    const result = validateParam('qty', 'Infinity', mockParamDefs.qty);
    expect(result.valid).toBe(false);
  });

  it('handles -Infinity', () => {
    const result = validateParam('qty', '-Infinity', mockParamDefs.qty);
    expect(result.valid).toBe(false);
  });

  it('handles NaN', () => {
    const result = validateParam('qty', 'NaN', mockParamDefs.qty);
    expect(result.valid).toBe(false);
  });
});
