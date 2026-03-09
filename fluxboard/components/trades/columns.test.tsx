import { describe, expect, it, vi } from 'vitest';
import { createColumns } from './columns';

describe('createColumns', () => {
  it('does not include gas columns in tokenmm trades table defaults', () => {
    const columns = createColumns(vi.fn());
    const columnIds = columns.map((column) => String(column.id ?? ''));

    expect(columnIds).not.toContain('gas_used');
    expect(columnIds).not.toContain('tx_hash');
  });
});
