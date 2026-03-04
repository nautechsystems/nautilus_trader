import { describe, it, expect } from 'vitest';
import { countDirtyCells, countDirtyInSelection } from '../utils/paramsState';

describe('paramsState helpers', () => {
  it('countDirtyCells returns 0 for empty map', () => {
    expect(countDirtyCells(new Map())).toBe(0);
  });

  it('counts single dirty cell', () => {
    const m = new Map<string, Set<string>>([
      ['stratA', new Set(['qty'])],
    ]);
    expect(countDirtyCells(m)).toBe(1);
  });

  it('counts multiple dirty cells across strategies', () => {
    const m = new Map<string, Set<string>>([
      ['stratA', new Set(['qty', 'bot_on'])],
      ['stratB', new Set(['cooldown'])],
    ]);
    expect(countDirtyCells(m)).toBe(3);
  });

  it('countDirtyInSelection filters by selection', () => {
    const m = new Map<string, Set<string>>([
      ['A', new Set(['x', 'y'])],
      ['B', new Set(['y'])],
      ['C', new Set(['z'])],
    ]);
    expect(countDirtyInSelection(m, ['A', 'C'])).toBe(3);
    expect(countDirtyInSelection(m, ['B'])).toBe(1);
    expect(countDirtyInSelection(m, [])).toBe(0);
  });
});

