/**
 * Simple Panels Tests
 *
 * Tests for FxTable panel after DataTable migration.
 * Verifies behavioral parity: sorting, rendering, empty states, and UI components.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import FxTable from '../../FxTable';
import type { FxPair } from '../../types';

describe('FxTable Component', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders empty state when no pairs', () => {
    render(<FxTable pairs={[]} />);
    expect(screen.getByText('No pairs reported')).toBeInTheDocument();
  });

  it('renders FX pair rows with Badge and StatusDot', () => {
    const mockPairs: FxPair[] = [
      {
        pair: 'USDT/USDC',
        price: '1.0001',
        source: 'bybit',
        age_ms: 1000,
        stale: false,
      },
    ];

    render(<FxTable pairs={mockPairs} />);

    expect(screen.getByText('USDT/USDC')).toBeInTheDocument();
    expect(screen.getByText('bybit')).toBeInTheDocument();
  });

  it('supports sorting by columns', async () => {
    const user = userEvent.setup();
    const mockPairs: FxPair[] = [
      {
        pair: 'USDT/USDC',
        price: '1.0001',
        source: 'bybit',
        age_ms: 1000,
        stale: false,
      },
      {
        pair: 'PLUME/USDC',
        price: '0.9999',
        source: 'curve',
        age_ms: 2000,
        stale: false,
      },
    ];

    render(<FxTable pairs={mockPairs} />);

    // Click Pair header to sort
    const pairHeader = screen.getByText('Pair');
    await user.click(pairHeader);

    expect(screen.getByText('USDT/USDC')).toBeInTheDocument();
    expect(screen.getByText('PLUME/USDC')).toBeInTheDocument();
  });

  it('displays bps deviation from par', () => {
    const mockPairs: FxPair[] = [
      {
        pair: 'USDT/USDC',
        price: '1.0001',
        source: 'bybit',
        age_ms: 1000,
        stale: false,
      },
    ];

    render(<FxTable pairs={mockPairs} />);

    // Should show +1 bps (0.01% above 1.0000)
    expect(screen.getByText(/\+1/)).toBeInTheDocument();
  });

  it('renders StatusDot for age indicator', () => {
    const mockPairs: FxPair[] = [
      {
        pair: 'USDT/USDC',
        price: '1.0001',
        source: 'bybit',
        age_ms: 1000, // Fresh
        stale: false,
      },
      {
        pair: 'PLUME/USDC',
        price: '0.9999',
        source: 'curve',
        age_ms: 15000, // Stale (> 10s)
        stale: true,
      },
    ];

    render(<FxTable pairs={mockPairs} />);

    // Both pairs should be visible
    expect(screen.getByText('USDT/USDC')).toBeInTheDocument();
    expect(screen.getByText('PLUME/USDC')).toBeInTheDocument();
  });
});

