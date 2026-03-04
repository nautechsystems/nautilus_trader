/**
 * DecisionModal Component Tests
 *
 * Tests for DecisionModal covering:
 * - Rendering with decision data
 * - Tab switching functionality
 * - Scrolling behavior within modal
 * - Height constraints and overflow handling
 * - Modal close behavior
 * - Data parsing and error handling
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { DecisionModal } from '@/components/trades/DecisionModal';
import type { Trade } from '@/types';

// Mock trade generator with decision data
function generateMockTrade(overrides: Partial<Trade> = {}): Trade {
  return {
    time: new Date().toISOString(),
    coin: 'PLUME',
    exchange: 'bybit',
    side: 'buy',
    price: 100,
    qty: 10,
    mv: 1000,
    fee: 0.1,
    trade_id: 'trade_123',
    exch_id: 'exch_123',
    order_id: 'order_123',
    signal_id: 'signal_123',
    decision: JSON.stringify({
      version: '1.0',
      summary: 'Test arbitrage decision',
      decision_timestamp: {
        iso: new Date().toISOString(),
        unix_ms: Date.now(),
      },
      market_data: {
        leg1: {
          exchange: 'rooster',
          symbol: 'WPLUME_USDC',
          type: 'dex',
          age_ms: 100,
          raw: { bid: 1.23, ask: 1.25, mid: 1.24 },
        },
        leg2: {
          exchange: 'bybit',
          symbol: 'PLUME_USDT',
          type: 'cex',
          age_ms: 50,
          raw: { bid: 1.22, ask: 1.24, mid: 1.23 },
        },
      },
      fair_values: {
        leg1: { fv_bid: 1.22, fv_ask: 1.24 },
        leg2: { fv_bid: 1.21, fv_ask: 1.23 },
      },
      fees: {
        gas_quote_per_unit: 0.001,
        leg1: { pool_fee_bps: 30 },
        leg2: { taker_fee_bps: 10 },
      },
      edge_parameters: {
        min_edge_bps: 20,
        gas_buffer_bps: 10,
      },
      strategy_parameters: {
        base_qty: 100,
        max_position: 1000,
      },
      opportunity: {
        case: 1,
        spread_bps: 50,
        edge_bps_net: 30,
        required_bps: 20,
        gas_bps: 5,
        leg1_action: 'buy',
        leg2_action: 'sell',
      },
    }),
    ...overrides,
  } as Trade;
}

describe('DecisionModal', () => {
  const mockOnClose = vi.fn();

  beforeEach(() => {
    mockOnClose.mockClear();
  });

  describe('Rendering', () => {
    it('renders modal with trade ID in title', () => {
      const trade = generateMockTrade();
      render(<DecisionModal trade={trade} onClose={mockOnClose} />);

      expect(screen.getByText(/Decision: trade_12/)).toBeInTheDocument();
    });

    it('renders all tab triggers', () => {
      const trade = generateMockTrade();
      render(<DecisionModal trade={trade} onClose={mockOnClose} />);

      // Use role selector to find tabs specifically (avoids "Summary" label in content)
      const tabs = screen.getAllByRole('tab');
      expect(tabs).toHaveLength(5);
      expect(tabs.map(t => t.textContent)).toEqual(['Summary', 'Legs', 'Fees', 'Params', 'Raw']);
    });

    it('renders footer buttons', () => {
      const trade = generateMockTrade();
      render(<DecisionModal trade={trade} onClose={mockOnClose} />);

      expect(screen.getByText('Copy JSON')).toBeInTheDocument();
      expect(screen.getByText('Close')).toBeInTheDocument();
    });

    it('renders Summary tab content by default', () => {
      const trade = generateMockTrade();
      render(<DecisionModal trade={trade} onClose={mockOnClose} />);

      // Summary tab should show key metrics
      expect(screen.getByText('Key Metrics')).toBeInTheDocument();
      expect(screen.getByText(/Case/)).toBeInTheDocument();
      expect(screen.getByText(/Spread \(bps\)/)).toBeInTheDocument();
    });

    it('handles missing decision data gracefully', () => {
      const trade = generateMockTrade({ decision: undefined });
      render(<DecisionModal trade={trade} onClose={mockOnClose} />);

      expect(screen.getByText('No decision data available')).toBeInTheDocument();
    });

    it('handles JSON parse errors gracefully', () => {
      const trade = generateMockTrade({ decision: '{invalid json}' });
      render(<DecisionModal trade={trade} onClose={mockOnClose} />);

      expect(screen.getByText(/Error parsing decision JSON/)).toBeInTheDocument();
    });

    it('handles decision as object (not string)', () => {
      const decisionObj = {
        version: '1.0',
        opportunity: { case: 1, spread_bps: 50 },
      };
      const trade = generateMockTrade({ decision: decisionObj as any });
      render(<DecisionModal trade={trade} onClose={mockOnClose} />);

      expect(screen.getByText('Key Metrics')).toBeInTheDocument();
    });
  });

  describe('Tab Switching', () => {
    it('switches to Legs tab when clicked', async () => {
      const user = userEvent.setup();
      const trade = generateMockTrade();
      render(<DecisionModal trade={trade} onClose={mockOnClose} />);

      await user.click(screen.getByText('Legs'));

      await waitFor(() => {
        expect(screen.getByText('Leg 1 (Market Data)')).toBeVisible();
        expect(screen.getByText('Leg 2 (Market Data)')).toBeVisible();
      });
    });

    it('switches to Fees tab when clicked', async () => {
      const user = userEvent.setup();
      const trade = generateMockTrade();
      render(<DecisionModal trade={trade} onClose={mockOnClose} />);

      await user.click(screen.getByText('Fees'));

      await waitFor(() => {
        expect(screen.getByText('Gas Quote Per Unit')).toBeVisible();
        expect(screen.getByText('Leg 1 Fees')).toBeVisible();
      });
    });

    it('switches to Params tab when clicked', async () => {
      const user = userEvent.setup();
      const trade = generateMockTrade();
      render(<DecisionModal trade={trade} onClose={mockOnClose} />);

      await user.click(screen.getByText('Params'));

      await waitFor(() => {
        expect(screen.getByText('Edge Parameters')).toBeVisible();
        expect(screen.getByText('Strategy Parameters')).toBeVisible();
      });
    });

    it('switches to Raw tab when clicked', async () => {
      const user = userEvent.setup();
      const trade = generateMockTrade();
      render(<DecisionModal trade={trade} onClose={mockOnClose} />);

      await user.click(screen.getByText('Raw'));

      await waitFor(() => {
        const preElement = screen.getByText(/"version": "1.0"/);
        expect(preElement).toBeVisible();
      });
    });

    it('maintains active tab styling', async () => {
      const user = userEvent.setup();
      const trade = generateMockTrade();
      const { container } = render(<DecisionModal trade={trade} onClose={mockOnClose} />);

      const legsTab = screen.getByText('Legs');
      await user.click(legsTab);

      await waitFor(() => {
        // Active tab should have data-state=active
        expect(legsTab).toHaveAttribute('data-state', 'active');
      });
    });
  });

  describe('Height Constraints and Overflow', () => {
    it('TabsRoot has height and overflow constraints', () => {
      const trade = generateMockTrade();
      const { container } = render(<DecisionModal trade={trade} onClose={mockOnClose} />);

      // Find TabsRoot by looking for the tabs container
      const tabsRoot = container.querySelector('[role="tablist"]')?.parentElement;
      expect(tabsRoot).toHaveClass('flex', 'flex-col', 'h-full', 'overflow-hidden');
    });

    it('TabsContent has overflow-y-auto for scrolling', async () => {
      const trade = generateMockTrade();
      const { container } = render(<DecisionModal trade={trade} onClose={mockOnClose} />);

      // Find active TabsContent
      const tabContent = container.querySelector('[role="tabpanel"]');
      expect(tabContent).toHaveClass('overflow-y-auto', 'h-full');
    });

    it('switches tabs and maintains scroll classes on new content', async () => {
      const user = userEvent.setup();
      const trade = generateMockTrade();
      const { container } = render(<DecisionModal trade={trade} onClose={mockOnClose} />);

      // Switch to Legs tab
      await user.click(screen.getByText('Legs'));

      await waitFor(() => {
        const tabContent = container.querySelector('[role="tabpanel"]');
        expect(tabContent).toHaveClass('overflow-y-auto', 'h-full');
      });

      // Switch to Raw tab
      await user.click(screen.getByText('Raw'));

      await waitFor(() => {
        const tabContent = container.querySelector('[role="tabpanel"]');
        expect(tabContent).toHaveClass('overflow-y-auto', 'h-full');
      });
    });

    it('RawTab pre element does not have nested overflow-auto', async () => {
      const user = userEvent.setup();
      const trade = generateMockTrade();
      const { container } = render(<DecisionModal trade={trade} onClose={mockOnClose} />);

      await user.click(screen.getByText('Raw'));

      await waitFor(() => {
        const preElement = container.querySelector('pre');
        expect(preElement).not.toHaveClass('overflow-auto');
        expect(preElement).toHaveClass('text-xs', 'rounded', 'p-4');
      });
    });

    it('handles large decision JSON that requires scrolling', async () => {
      const user = userEvent.setup();

      // Generate large decision with lots of data
      const largeDecision = {
        version: '1.0',
        summary: 'Test decision with lots of data',
        opportunity: { case: 1, spread_bps: 50 },
        market_data: {
          leg1: { exchange: 'rooster', symbol: 'WPLUME_USDC' },
          leg2: { exchange: 'bybit', symbol: 'PLUME_USDT' },
        },
        // Add many fields to force overflow
        ...Object.fromEntries(
          Array.from({ length: 50 }, (_, i) => [`field_${i}`, `value_${i}`])
        ),
      };

      const trade = generateMockTrade({ decision: JSON.stringify(largeDecision) });
      const { container } = render(<DecisionModal trade={trade} onClose={mockOnClose} />);

      await user.click(screen.getByText('Raw'));

      await waitFor(() => {
        const tabContent = container.querySelector('[role="tabpanel"]');
        expect(tabContent).toHaveClass('overflow-y-auto');

        // Verify content exists and is scrollable
        const preElement = container.querySelector('pre');
        expect(preElement).toBeInTheDocument();
        expect(preElement?.textContent).toContain('field_49');
      });
    });
  });

  describe('Close Behavior', () => {
    it('calls onClose when Close button is clicked', async () => {
      const user = userEvent.setup();
      const trade = generateMockTrade();
      render(<DecisionModal trade={trade} onClose={mockOnClose} />);

      await user.click(screen.getByText('Close'));

      expect(mockOnClose).toHaveBeenCalledTimes(1);
    });

    it('calls onClose when Escape is pressed', async () => {
      const user = userEvent.setup();
      const trade = generateMockTrade();
      render(<DecisionModal trade={trade} onClose={mockOnClose} />);

      await user.keyboard('{Escape}');

      await waitFor(() => {
        // Radix may call onClose multiple times during animation
        expect(mockOnClose).toHaveBeenCalled();
      });
    });
  });

  describe('Copy Functionality', () => {
    it('Copy JSON button copies full decision to clipboard', async () => {
      const user = userEvent.setup();
      const mockWriteText = vi.fn().mockResolvedValue(undefined);

      // Properly mock navigator.clipboard
      Object.defineProperty(navigator, 'clipboard', {
        value: { writeText: mockWriteText },
        writable: true,
        configurable: true,
      });

      const trade = generateMockTrade();
      render(<DecisionModal trade={trade} onClose={mockOnClose} />);

      await user.click(screen.getByText('Copy JSON'));

      expect(mockWriteText).toHaveBeenCalledTimes(1);
      const copiedText = mockWriteText.mock.calls[0][0];
      expect(copiedText).toContain('"version": "1.0"');
      expect(copiedText).toContain('"opportunity"');
    });

    it('Copy TSV button copies summary metrics', async () => {
      const user = userEvent.setup();
      const mockWriteText = vi.fn().mockResolvedValue(undefined);

      // Properly mock navigator.clipboard
      Object.defineProperty(navigator, 'clipboard', {
        value: { writeText: mockWriteText },
        writable: true,
        configurable: true,
      });

      const trade = generateMockTrade();
      render(<DecisionModal trade={trade} onClose={mockOnClose} />);

      // Summary tab has Copy TSV button
      await user.click(screen.getByText('Copy TSV'));

      expect(mockWriteText).toHaveBeenCalledTimes(1);
      const copiedText = mockWriteText.mock.calls[0][0];
      expect(copiedText).toContain('case\t1');
      expect(copiedText).toContain('spread_bps\t50');
      expect(copiedText).toContain('edge_bps_net\t30');
    });
  });

  describe('Summary Tab Content', () => {
    it('displays key metrics from opportunity data', () => {
      const trade = generateMockTrade();
      render(<DecisionModal trade={trade} onClose={mockOnClose} />);

      expect(screen.getByText('Key Metrics')).toBeInTheDocument();
      expect(screen.getByText(/Case/)).toBeInTheDocument();
      expect(screen.getByText(/Spread \(bps\)/)).toBeInTheDocument();
      expect(screen.getByText(/Edge Net \(bps\)/)).toBeInTheDocument();
      expect(screen.getByText(/Required \(bps\)/)).toBeInTheDocument();
      expect(screen.getByText(/Gas \(bps\)/)).toBeInTheDocument();
    });

    it('displays summary text when present', () => {
      const trade = generateMockTrade();
      render(<DecisionModal trade={trade} onClose={mockOnClose} />);

      // Look for the summary section label (not the tab)
      const summaryLabels = screen.getAllByText('Summary');
      expect(summaryLabels.length).toBeGreaterThan(0);
      expect(screen.getByText('Test arbitrage decision')).toBeInTheDocument();
    });

    it('handles missing opportunity data', () => {
      const trade = generateMockTrade({
        decision: JSON.stringify({ version: '1.0' }),
      });
      render(<DecisionModal trade={trade} onClose={mockOnClose} />);

      expect(screen.getByText('No opportunity data')).toBeInTheDocument();
    });
  });

  describe('Legs Tab Content', () => {
    it('displays market data for both legs', async () => {
      const user = userEvent.setup();
      const trade = generateMockTrade();
      render(<DecisionModal trade={trade} onClose={mockOnClose} />);

      await user.click(screen.getByText('Legs'));

      await waitFor(() => {
        expect(screen.getByText('Leg 1 (Market Data)')).toBeVisible();
        expect(screen.getByText('Leg 2 (Market Data)')).toBeVisible();
        expect(screen.getByText('rooster')).toBeVisible();
        expect(screen.getByText('bybit')).toBeVisible();
      });
    });

    it('handles missing legs data', async () => {
      const user = userEvent.setup();
      const trade = generateMockTrade({
        decision: JSON.stringify({ version: '1.0', opportunity: { case: 1 } }),
      });
      render(<DecisionModal trade={trade} onClose={mockOnClose} />);

      await user.click(screen.getByText('Legs'));

      await waitFor(() => {
        expect(screen.getByText('No legs data')).toBeVisible();
      });
    });
  });

  describe('Fees Tab Content', () => {
    it('displays fee data for both legs', async () => {
      const user = userEvent.setup();
      const trade = generateMockTrade();
      render(<DecisionModal trade={trade} onClose={mockOnClose} />);

      await user.click(screen.getByText('Fees'));

      await waitFor(() => {
        expect(screen.getByText('Gas Quote Per Unit')).toBeVisible();
        expect(screen.getByText('Leg 1 Fees')).toBeVisible();
        expect(screen.getByText('Leg 2 Fees')).toBeVisible();
      });
    });

    it('handles missing fees data', async () => {
      const user = userEvent.setup();
      const trade = generateMockTrade({
        decision: JSON.stringify({ version: '1.0', opportunity: { case: 1 } }),
      });
      render(<DecisionModal trade={trade} onClose={mockOnClose} />);

      await user.click(screen.getByText('Fees'));

      await waitFor(() => {
        expect(screen.getByText('No fees data')).toBeVisible();
      });
    });
  });

  describe('Params Tab Content', () => {
    it('displays edge and strategy parameters', async () => {
      const user = userEvent.setup();
      const trade = generateMockTrade();
      render(<DecisionModal trade={trade} onClose={mockOnClose} />);

      await user.click(screen.getByText('Params'));

      await waitFor(() => {
        expect(screen.getByText('Edge Parameters')).toBeVisible();
        expect(screen.getByText('Strategy Parameters')).toBeVisible();
      });
    });

    it('handles missing params data', async () => {
      const user = userEvent.setup();
      const trade = generateMockTrade({
        decision: JSON.stringify({ version: '1.0', opportunity: { case: 1 } }),
      });
      render(<DecisionModal trade={trade} onClose={mockOnClose} />);

      await user.click(screen.getByText('Params'));

      await waitFor(() => {
        expect(screen.getByText('No params data')).toBeVisible();
      });
    });
  });

  describe('Raw Tab Content', () => {
    it('displays formatted JSON', async () => {
      const user = userEvent.setup();
      const trade = generateMockTrade();
      render(<DecisionModal trade={trade} onClose={mockOnClose} />);

      await user.click(screen.getByText('Raw'));

      await waitFor(() => {
        const preElement = screen.getByText(/"version": "1.0"/);
        expect(preElement).toBeVisible();
        // Check JSON is formatted (has newlines)
        expect(preElement.textContent).toContain('\n');
      });
    });

    it('displays complete decision structure in JSON', async () => {
      const user = userEvent.setup();
      const trade = generateMockTrade();
      render(<DecisionModal trade={trade} onClose={mockOnClose} />);

      await user.click(screen.getByText('Raw'));

      await waitFor(() => {
        const content = screen.getByRole('tabpanel').textContent;
        expect(content).toContain('"opportunity"');
        expect(content).toContain('"market_data"');
        expect(content).toContain('"fees"');
        expect(content).toContain('"edge_parameters"');
        expect(content).toContain('"strategy_parameters"');
      });
    });
  });
});
