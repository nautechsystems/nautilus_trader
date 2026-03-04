import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import { LegCell } from '@/components/domain/scanners/ScannersTable';
import type { SignalLeg } from '@/types';

describe('LegCell layout', () => {
  it('stacks market label above bid/mid/ask row', () => {
    const leg: SignalLeg = {
      exchange: 'PCS',
      coin: 'WBNB-USDT',
      decision_bid: 585.0,
      decision_ask: 585.2,
    };

    render(<LegCell leg={leg} />);

    const marketLabel = screen.getByText('PCS WBNB-USDT');
    expect(marketLabel).toBeInTheDocument();

    const container = marketLabel.parentElement;
    expect(container?.className).toContain('flex');
    expect(container?.className).toContain('flex-col');

    const priceRow = container?.querySelector('.tabular-nums');
    expect(priceRow).toBeInTheDocument();
  });
});
