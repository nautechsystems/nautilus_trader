import { describe, expect, it } from 'vitest';

import {
  deriveStrategyStatus,
  describeTradingStatus,
  statusToFilterValue,
} from './strategyStatus';

describe('strategyStatus', () => {
  it('treats bot_on=0 with a live runner as paused, not pending', () => {
    const status = deriveStrategyStatus({ running: true, trading: '0' });

    expect(statusToFilterValue(status)).toBe('Paused');
    expect(describeTradingStatus(status)).toEqual({
      variant: 'inactive',
      label: 'Paused',
      subLabel: 'Runner On',
    });
  });

  it('treats bot_on=1 with runner off as pending attention', () => {
    const status = deriveStrategyStatus({ running: false, trading: '1' });

    expect(statusToFilterValue(status)).toBe('Pending');
    expect(describeTradingStatus(status)).toEqual({
      variant: 'pending',
      label: 'Pending',
      subLabel: 'Runner Off',
    });
  });

  it('treats bot_on=1 with a live runner as enabled', () => {
    const status = deriveStrategyStatus({ running: true, trading: '1' });

    expect(statusToFilterValue(status)).toBe('Enabled');
    expect(describeTradingStatus(status)).toEqual({
      variant: 'live',
      label: 'Enabled',
      subLabel: 'Runner On',
    });
  });

  it('treats enabled-but-blocked strategies as pending', () => {
    const status = deriveStrategyStatus({ running: true, trading: '1', blocked: true });

    expect(statusToFilterValue(status)).toBe('Pending');
    expect(describeTradingStatus(status)).toEqual({
      variant: 'pending',
      label: 'Pending',
      subLabel: 'Runner On',
    });
  });
});
