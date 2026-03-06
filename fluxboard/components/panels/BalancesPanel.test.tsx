import { describe, expect, it } from 'vitest';

import { BalancesPanel } from './BalancesPanel';

describe('BalancesPanel sizing', () => {
  it('allows dashboard resize up to full width', () => {
    expect(BalancesPanel.defaultSize).toMatchObject({
      w: 4,
      h: 4,
      minW: 3,
      maxW: 12,
    });
  });
});
