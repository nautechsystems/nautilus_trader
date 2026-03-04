import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { OrderViewChart } from '../components/domain/orderView/OrderViewChart';

const chartSpies = vi.hoisted(() => ({
  createChart: vi.fn(),
  addSeries: vi.fn(),
  setData: vi.fn(),
  update: vi.fn(),
  applySeriesOptions: vi.fn(),
  createPriceLine: vi.fn(),
  removePriceLine: vi.fn(),
  applyPriceLineOptions: vi.fn(),
  setMarkers: vi.fn(),
  fitContent: vi.fn(),
  getVisibleRange: vi.fn(),
  setVisibleRange: vi.fn(),
  applyOptions: vi.fn(),
  removeChart: vi.fn(),
}));

vi.mock('lightweight-charts', () => {
  const seriesApi = {
    setData: chartSpies.setData,
    update: chartSpies.update,
    applyOptions: chartSpies.applySeriesOptions,
    createPriceLine: chartSpies.createPriceLine.mockImplementation(() => ({
      applyOptions: chartSpies.applyPriceLineOptions,
    })),
    removePriceLine: chartSpies.removePriceLine,
  };
  const chartApi = {
    addSeries: chartSpies.addSeries.mockImplementation(() => seriesApi),
    timeScale: () => ({
      fitContent: chartSpies.fitContent,
      getVisibleRange: chartSpies.getVisibleRange,
      setVisibleRange: chartSpies.setVisibleRange,
    }),
    applyOptions: chartSpies.applyOptions,
    remove: chartSpies.removeChart,
  };

  chartSpies.createChart.mockImplementation(() => chartApi);

  return {
    ColorType: { Solid: 'solid' },
    CandlestickSeries: {},
    LineSeries: {},
    LineStyle: { Solid: 0, Dashed: 2 },
    createChart: chartSpies.createChart,
    createSeriesMarkers: () => ({ setMarkers: chartSpies.setMarkers }),
  };
});

describe('OrderViewChart', () => {
  beforeEach(() => {
    Object.values(chartSpies).forEach((spy) => spy.mockClear());
  });

  const defaultChartProps = {
    lifetimeSegments: [],
    fills: [],
    focus: { orderKey: null, eventKey: null, side: null, price: null },
    showBids: true,
    showAsks: true,
    showFills: true,
    showBboLines: false,
    bestBid: null as number | null,
    bestAsk: null as number | null,
  };

  it('sanitizes invalid browser locale before chart initialization', async () => {
    const navigatorPrototype = Object.getPrototypeOf(window.navigator) as Navigator;
    const originalLanguageDescriptor = Object.getOwnPropertyDescriptor(navigatorPrototype, 'language');

    Object.defineProperty(navigatorPrototype, 'language', {
      configurable: true,
      get: () => 'en-US@posix',
    });

    try {
      render(
        <OrderViewChart
          {...defaultChartProps}
          leg="maker"
          priceSeries={[
            { ts_ms: 1_700_000_000_000, maker_mid: 0.00881 },
            { ts_ms: 1_700_000_001_000, maker_mid: 0.00882 },
          ]}
        />
      );

      await waitFor(() =>
        expect(chartSpies.createChart).toHaveBeenCalledWith(
          expect.any(HTMLElement),
          expect.objectContaining({
            localization: expect.objectContaining({ locale: 'en-US' }),
          })
        )
      );
    } finally {
      if (originalLanguageDescriptor) {
        Object.defineProperty(navigatorPrototype, 'language', originalLanguageDescriptor);
      }
    }
  });

  it('does not repeatedly refit content during sliding-window setData updates', async () => {
    const baseProps = {
      leg: 'maker' as const,
      lifetimeSegments: [],
      fills: [],
      focus: { orderKey: null, eventKey: null, side: null, price: null },
      showBids: true,
      showAsks: true,
      showFills: true,
      showBboLines: false,
      bestBid: null as number | null,
      bestAsk: null as number | null,
    };
    const { rerender } = render(
      <OrderViewChart
        {...baseProps}
        priceSeries={[
          { ts_ms: 1_700_000_000_000, maker_mid: 0.00881 },
          { ts_ms: 1_700_000_001_000, maker_mid: 0.00882 },
        ]}
      />
    );

    await waitFor(() => expect(chartSpies.fitContent).toHaveBeenCalledTimes(1));

    rerender(
      <OrderViewChart
        {...baseProps}
        priceSeries={[
          { ts_ms: 1_700_000_001_000, maker_mid: 0.00882 },
          { ts_ms: 1_700_000_002_000, maker_mid: 0.00883 },
        ]}
      />
    );

    await waitFor(() => expect(chartSpies.setData).toHaveBeenCalledTimes(2));
    expect(chartSpies.fitContent).toHaveBeenCalledTimes(1);
  });

  it('restores visible viewport range after full setData reconcile', async () => {
    chartSpies.getVisibleRange.mockReturnValue({ from: 100, to: 200 });
    const baseProps = {
      leg: 'maker' as const,
      ...defaultChartProps,
    };

    const { rerender } = render(
      <OrderViewChart
        {...baseProps}
        candles={[
          { ts_ms: 1_700_000_000_000, open: 30000, high: 30010, low: 29990, close: 30005, volume: 1.0 },
          { ts_ms: 1_700_000_001_000, open: 30005, high: 30012, low: 30000, close: 30010, volume: 1.1 },
          { ts_ms: 1_700_000_002_000, open: 30010, high: 30020, low: 30005, close: 30015, volume: 1.2 },
        ]}
        priceSeries={[]}
      />
    );

    await waitFor(() => expect(chartSpies.setData).toHaveBeenCalled());

    rerender(
      <OrderViewChart
        {...baseProps}
        candles={[
          { ts_ms: 1_700_000_001_000, open: 30005, high: 30012, low: 30000, close: 30010, volume: 1.1 },
          { ts_ms: 1_700_000_002_000, open: 30010, high: 30020, low: 30005, close: 30015, volume: 1.2 },
        ]}
        priceSeries={[]}
      />
    );

    await waitFor(() =>
      expect(chartSpies.setVisibleRange).toHaveBeenCalledWith({ from: 100, to: 200 })
    );
  });

  it('renders a reset control and refits content on demand', async () => {
    render(
      <OrderViewChart
        {...defaultChartProps}
        leg="maker"
        priceSeries={[
          { ts_ms: 1_700_000_000_000, maker_mid: 0.00881 },
          { ts_ms: 1_700_000_001_000, maker_mid: 0.00882 },
        ]}
      />
    );

    await waitFor(() => expect(chartSpies.fitContent).toHaveBeenCalledTimes(1));
    fireEvent.click(screen.getByRole('button', { name: /reset zoom/i }));
    expect(chartSpies.fitContent).toHaveBeenCalledTimes(2);
  });

  it('adds a last-price guide line for the newest plotted point', async () => {
    render(
      <OrderViewChart
        {...defaultChartProps}
        leg="maker"
        priceSeries={[
          { ts_ms: 1_700_000_000_000, maker_mid: 0.00881 },
          { ts_ms: 1_700_000_001_000, maker_mid: 0.00882 },
        ]}
        showBboLines
        bestBid={0.00881}
        bestAsk={0.00883}
      />
    );

    await waitFor(() =>
      expect(chartSpies.createPriceLine).toHaveBeenCalledWith(
        expect.objectContaining({ title: 'Last' })
      )
    );
    expect(chartSpies.createPriceLine).toHaveBeenCalledWith(
      expect.objectContaining({ title: 'Bid' })
    );
    expect(chartSpies.createPriceLine).toHaveBeenCalledWith(
      expect.objectContaining({ title: 'Ask' })
    );
  });

  it('handles 200+ overlay lines without refit thrash', async () => {
    const lifetimeSegments = Array.from({ length: 220 }, (_, index) => ({
      segment_id: `seg-${index}`,
      order_key: `oid-${index}`,
      side: index % 2 === 0 ? ('bid' as const) : ('ask' as const),
      price: 30000 + index * 0.01,
      start_ts_ms: 1_700_000_000_000 + index,
      end_ts_ms: 1_700_000_001_000 + index,
      close_reason: 'fill' as const,
      lifetime_start_unknown: false,
    }));
    render(
      <OrderViewChart
        {...defaultChartProps}
        leg="maker"
        priceSeries={[
          { ts_ms: 1_700_000_000_000, maker_mid: 30000.0 },
          { ts_ms: 1_700_000_001_000, maker_mid: 30000.5 },
        ]}
        lifetimeSegments={lifetimeSegments}
      />
    );

    await waitFor(() => {
      // 220 overlay segments + 1 candlestick series
      expect(chartSpies.addSeries).toHaveBeenCalled();
      expect(chartSpies.setData).toHaveBeenCalled();
    });
    expect(chartSpies.fitContent).toHaveBeenCalledTimes(1);
  });

  it('dims unrelated overlays when cross-panel focus is active', async () => {
    render(
      <OrderViewChart
        {...defaultChartProps}
        leg="maker"
        priceSeries={[
          { ts_ms: 1_700_000_000_000, maker_mid: 30000.0 },
          { ts_ms: 1_700_000_001_000, maker_mid: 30000.5 },
        ]}
        lifetimeSegments={[
          {
            segment_id: 'seg-focus',
            order_key: 'oid-focus',
            side: 'bid',
            price: 30000,
            start_ts_ms: 1_700_000_000_000,
            end_ts_ms: 1_700_000_001_000,
            close_reason: 'fill',
            lifetime_start_unknown: false,
          },
          {
            segment_id: 'seg-other',
            order_key: 'oid-other',
            side: 'ask',
            price: 30001,
            start_ts_ms: 1_700_000_000_000,
            end_ts_ms: 1_700_000_001_000,
            close_reason: 'fill',
            lifetime_start_unknown: false,
          },
        ]}
        fills={[
          {
            event_key: 'evt-focus',
            ts_ms: 1_700_000_000_200,
            type: 'fill',
            side: 'buy',
            px: '30000',
            order_id: 'oid-focus',
          },
          {
            event_key: 'evt-other',
            ts_ms: 1_700_000_000_300,
            type: 'fill',
            side: 'sell',
            px: '30001',
            order_id: 'oid-other',
          },
        ]}
        focus={{ orderKey: 'oid-focus', eventKey: null, side: 'bid', price: 30000 }}
      />
    );

    await waitFor(() => expect(chartSpies.setMarkers).toHaveBeenCalled());
    expect(chartSpies.applySeriesOptions).toHaveBeenCalledWith(
      expect.objectContaining({ lineWidth: 2, lineStyle: 0 })
    );
    expect(chartSpies.applySeriesOptions).toHaveBeenCalledWith(
      expect.objectContaining({ lineWidth: 1, lineStyle: 2 })
    );
    const latestMarkers = chartSpies.setMarkers.mock.calls[chartSpies.setMarkers.mock.calls.length - 1]?.[0] as Array<{ color: string }>;
    expect(latestMarkers).toHaveLength(2);
    expect(latestMarkers.some((marker) => marker.color.includes('rgba('))).toBe(true);
  });
});
