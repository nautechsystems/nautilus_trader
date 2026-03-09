import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';

// Mock the summary data type
interface PnLSummary {
  weighted_pnl_bps: number;
  weighted_pnl_usd?: number;
  fees_bps?: number;
  fees_usd?: number;
  net_pnl_bps: number;
  net_pnl_usd?: number;
  total_hedged_qty: number;
  total_notional: number;
}

// Mock the formatting functions
const fmtPrimary = (bps: number, usd?: number, unitPrimary: 'bps' | 'usd' = 'bps') => {
  if (unitPrimary === 'usd' && usd !== undefined) {
    return `$${usd.toFixed(2)}`;
  }
  return `${bps.toFixed(2)} bps`;
};

// Mock summary cards component (simplified version)
const SummaryCards = ({
  summary,
  unitPrimary
}: {
  summary: PnLSummary;
  unitPrimary: 'bps' | 'usd'
}) => {
  return (
    <div>
      {/* Gross Card */}
      <div data-testid="gross-card">
        <div>Gross</div>
        <div data-testid="gross-value">
          {fmtPrimary(summary.weighted_pnl_bps, summary.weighted_pnl_usd, unitPrimary)}
        </div>
        <div data-testid="gross-secondary">
          {unitPrimary === 'usd'
            ? `${summary.weighted_pnl_bps.toFixed(2)} bps`
            : `$${summary.weighted_pnl_usd?.toFixed(2) || '—'}`}
        </div>
      </div>

      {/* Fees Card */}
      <div data-testid="fees-card">
        <div>Fees</div>
        <div data-testid="fees-value">
          {unitPrimary === 'usd'
            ? `$${summary.fees_usd?.toFixed(2) || '—'}`
            : `${summary.fees_bps?.toFixed(1) || '—'} bps`}
        </div>
      </div>

      {/* Net Card */}
      <div data-testid="net-card">
        <div>Net</div>
        <div data-testid="net-value">
          {fmtPrimary(summary.net_pnl_bps, summary.net_pnl_usd, unitPrimary)}
        </div>
        <div data-testid="net-secondary">
          {unitPrimary === 'usd'
            ? `${summary.net_pnl_bps.toFixed(2)} bps`
            : `$${summary.net_pnl_usd?.toFixed(2) || '—'}`}
        </div>
      </div>
    </div>
  );
};

describe('PnL Summary Cards', () => {
  const mockSummary: PnLSummary = {
    weighted_pnl_bps: 14.09,
    weighted_pnl_usd: 538.5,
    fees_bps: 7.0,
    fees_usd: 270.9,
    net_pnl_bps: 7.09,
    net_pnl_usd: 270.9,
    total_hedged_qty: 1.51,
    total_notional: 38200.0,
  };

  describe('when unitPrimary is bps', () => {
    it('displays gross pnl in bps', () => {
      render(<SummaryCards summary={mockSummary} unitPrimary="bps" />);

      expect(screen.getByTestId('gross-value')).toHaveTextContent('14.09 bps');
      expect(screen.getByTestId('gross-secondary')).toHaveTextContent('$538.50');
    });

    it('displays fees in bps', () => {
      render(<SummaryCards summary={mockSummary} unitPrimary="bps" />);

      expect(screen.getByTestId('fees-value')).toHaveTextContent('7.0 bps');
    });

    it('displays net pnl in bps', () => {
      render(<SummaryCards summary={mockSummary} unitPrimary="bps" />);

      expect(screen.getByTestId('net-value')).toHaveTextContent('7.09 bps');
      expect(screen.getByTestId('net-secondary')).toHaveTextContent('$270.90');
    });
  });

  describe('when unitPrimary is usd', () => {
    it('displays gross pnl in usd', () => {
      render(<SummaryCards summary={mockSummary} unitPrimary="usd" />);

      expect(screen.getByTestId('gross-value')).toHaveTextContent('$538.50');
      expect(screen.getByTestId('gross-secondary')).toHaveTextContent('14.09 bps');
    });

    it('displays fees in usd', () => {
      render(<SummaryCards summary={mockSummary} unitPrimary="usd" />);

      expect(screen.getByTestId('fees-value')).toHaveTextContent('$270.90');
    });

    it('displays net pnl in usd', () => {
      render(<SummaryCards summary={mockSummary} unitPrimary="usd" />);

      expect(screen.getByTestId('net-value')).toHaveTextContent('$270.90');
      expect(screen.getByTestId('net-secondary')).toHaveTextContent('7.09 bps');
    });
  });

  describe('when usd values are undefined', () => {
    const summaryWithoutUsd: PnLSummary = {
      weighted_pnl_bps: 14.09,
      net_pnl_bps: 7.09,
      total_hedged_qty: 1.51,
      total_notional: 38200.0,
    };

    it('falls back to bps display', () => {
      render(<SummaryCards summary={summaryWithoutUsd} unitPrimary="usd" />);

      expect(screen.getByTestId('gross-value')).toHaveTextContent('14.09 bps');
      expect(screen.getByTestId('gross-secondary')).toHaveTextContent('14.09 bps');
      expect(screen.getByTestId('fees-value')).toHaveTextContent('$—');
    });
  });
});
