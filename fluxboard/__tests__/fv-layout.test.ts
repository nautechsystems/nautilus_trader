import { describe, expect, it } from 'vitest';

import { PANEL_REGISTRY } from '../components/layout/PanelRegistry';
import { PRESETS } from '../components/layout/presets';

describe('FV layout wiring', () => {
  it('registers fv panel in registry', () => {
    expect(PANEL_REGISTRY).toHaveProperty('fv');
  });

  it('includes fv panel in default dashboard preset', () => {
    const defaultLayout = PRESETS.default || [];
    expect(defaultLayout.some((item) => item.i === 'fv')).toBe(true);
  });
});
