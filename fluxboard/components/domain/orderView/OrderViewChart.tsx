import { memo, useEffect, useMemo, useRef, useState } from 'react';
import {
  CandlestickSeries,
  ColorType,
  LineSeries,
  LineStyle,
  createChart,
  createSeriesMarkers,
  type CandlestickData,
  type IChartApi,
  type IPriceLine,
  type ISeriesApi,
  type SeriesMarker,
  type UTCTimestamp,
} from 'lightweight-charts';

import { colors } from '@/lib/tokens';
import type {
  OrderViewFocusState,
  OrderViewLifetimeSegment,
  OrderViewPricePoint,
} from '@/stores/orderViewStore';
import type { OrderViewCandleRow, OrderViewEvent } from '@/types';

type OrderViewChartProps = {
  leg: 'maker' | 'hedge' | 'both';
  priceSeries: OrderViewPricePoint[];
  lifetimeSegments: OrderViewLifetimeSegment[];
  candles?: OrderViewCandleRow[];
  candleSource?: string | null;
  fills: OrderViewEvent[];
  focus: OrderViewFocusState;
  showBids: boolean;
  showAsks: boolean;
  showFills: boolean;
  showBboLines: boolean;
  bestBid: number | null;
  bestAsk: number | null;
};

const ORDER_VIEW_PRICE_PRECISION = 6;
const ORDER_VIEW_PRICE_MIN_MOVE = 10 ** -ORDER_VIEW_PRICE_PRECISION;
const ORDER_VIEW_MAX_SEGMENTS_RENDERED = 400;
const ORDER_VIEW_FOCUS_MATCH_EPSILON = ORDER_VIEW_PRICE_MIN_MOVE / 2;
const ORDER_VIEW_FALLBACK_LOCALE = 'en-US';

const normalizeLocaleCandidate = (value: string): string => value.replace(/_/g, '-').replace(/@.*$/, '').trim();

const toValidLocale = (value: unknown): string => {
  if (typeof value !== 'string') return ORDER_VIEW_FALLBACK_LOCALE;
  const normalized = normalizeLocaleCandidate(value);
  if (!normalized) return ORDER_VIEW_FALLBACK_LOCALE;
  if (typeof Intl === 'undefined' || typeof Intl.getCanonicalLocales !== 'function') {
    return normalized;
  }
  try {
    const [canonical] = Intl.getCanonicalLocales(normalized);
    return canonical || ORDER_VIEW_FALLBACK_LOCALE;
  } catch {
    return ORDER_VIEW_FALLBACK_LOCALE;
  }
};

const resolveChartLocale = (): string =>
  typeof navigator === 'undefined'
    ? ORDER_VIEW_FALLBACK_LOCALE
    : toValidLocale((navigator as Navigator).language);

const toFiniteNumber = (value: unknown): number | null => {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : null;
};

const quantizePrice = (value: number): number =>
  Math.round(value / ORDER_VIEW_PRICE_MIN_MOVE) * ORDER_VIEW_PRICE_MIN_MOVE;

const toUtcTimestamp = (tsMs: number): UTCTimestamp => Math.trunc(tsMs / 1000) as UTCTimestamp;

const toMidValue = (point: OrderViewPricePoint, leg: 'maker' | 'hedge' | 'both'): number | null => {
  if (leg === 'maker') {
    return toFiniteNumber(point.maker_mid);
  }
  if (leg === 'hedge') {
    return toFiniteNumber(point.hedge_mid);
  }
  return toFiniteNumber(point.maker_mid) ?? toFiniteNumber(point.hedge_mid);
};

const toCandlestickDataFromPriceSeries = (
  priceSeries: OrderViewPricePoint[],
  leg: 'maker' | 'hedge' | 'both'
): CandlestickData<UTCTimestamp>[] => {
  const points: CandlestickData<UTCTimestamp>[] = [];
  for (let index = 0; index < priceSeries.length; index += 1) {
    const point = priceSeries[index];
    const value = toMidValue(point, leg);
    if (value === null) continue;
    const tsMs = toFiniteNumber(point.ts_ms);
    if (tsMs === null || tsMs <= 0) continue;
    const time = toUtcTimestamp(tsMs);
    const quantizedValue = quantizePrice(value);
    const lastPoint = points[points.length - 1];
    if (lastPoint && lastPoint.time === time) {
      points[points.length - 1] = {
        time,
        open: lastPoint.open,
        high: Math.max(lastPoint.high, quantizedValue),
        low: Math.min(lastPoint.low, quantizedValue),
        close: quantizedValue,
      };
      continue;
    }
    points.push({
      time,
      open: quantizedValue,
      high: quantizedValue,
      low: quantizedValue,
      close: quantizedValue,
    });
  }
  return points;
};

const toCandlestickDataFromRows = (rows: OrderViewCandleRow[] | undefined): CandlestickData<UTCTimestamp>[] => {
  if (!Array.isArray(rows) || rows.length === 0) return [];
  const normalized = rows
    .map((row) => {
      const tsMs = toFiniteNumber(row.ts_ms);
      const open = toFiniteNumber(row.open);
      const high = toFiniteNumber(row.high);
      const low = toFiniteNumber(row.low);
      const close = toFiniteNumber(row.close);
      if (tsMs === null || open === null || high === null || low === null || close === null || tsMs <= 0) {
        return null;
      }
      return {
        time: toUtcTimestamp(tsMs),
        open: quantizePrice(open),
        high: quantizePrice(high),
        low: quantizePrice(low),
        close: quantizePrice(close),
      };
    })
    .filter(Boolean) as CandlestickData<UTCTimestamp>[];
  normalized.sort((lhs, rhs) => Number(lhs.time) - Number(rhs.time));
  const deduped: CandlestickData<UTCTimestamp>[] = [];
  for (const row of normalized) {
    const last = deduped[deduped.length - 1];
    if (last && last.time === row.time) {
      deduped[deduped.length - 1] = row;
    } else {
      deduped.push(row);
    }
  }
  return deduped;
};

const formatTooltipPrice = (value: number): string => value.toFixed(ORDER_VIEW_PRICE_PRECISION);

const withAlpha = (color: string, alpha: number): string => {
  const safeAlpha = Math.max(0, Math.min(1, alpha));
  const hex = color.trim();
  if (/^#([A-Fa-f0-9]{6})$/.test(hex)) {
    const r = Number.parseInt(hex.slice(1, 3), 16);
    const g = Number.parseInt(hex.slice(3, 5), 16);
    const b = Number.parseInt(hex.slice(5, 7), 16);
    return `rgba(${r}, ${g}, ${b}, ${safeAlpha.toFixed(3)})`;
  }
  return color;
};

const normalizeOrderSide = (value: unknown): 'bid' | 'ask' | null => {
  const text = String(value || '')
    .trim()
    .toLowerCase();
  if (text === 'buy' || text === 'bid') return 'bid';
  if (text === 'sell' || text === 'ask') return 'ask';
  return null;
};

const toOrderKey = (value: { order_id?: unknown; client_order_id?: unknown }): string | null => {
  const orderId = String(value.order_id || '').trim();
  if (orderId) return orderId;
  const clientOrderId = String(value.client_order_id || '').trim();
  if (clientOrderId) return clientOrderId;
  return null;
};

const samePrice = (lhs: number | null, rhs: number | null): boolean => {
  if (lhs === null || rhs === null) return false;
  return Math.abs(lhs - rhs) <= ORDER_VIEW_FOCUS_MATCH_EPSILON;
};

const isFocusActive = (focus: OrderViewFocusState): boolean =>
  Boolean(focus.orderKey || focus.eventKey || focus.side || focus.price !== null);

const segmentMatchesFocus = (
  segment: OrderViewLifetimeSegment,
  focus: OrderViewFocusState,
  focusActive: boolean
): boolean => {
  if (!focusActive) return true;

  let matched = false;
  const segmentPrice = toFiniteNumber(segment.price);
  if (focus.orderKey) {
    matched = matched || String(segment.order_key || '') === focus.orderKey;
  }
  if (focus.side && focus.price !== null) {
    matched = matched || (segment.side === focus.side && samePrice(segmentPrice, focus.price));
  } else if (focus.price !== null) {
    matched = matched || samePrice(segmentPrice, focus.price);
  } else if (focus.side) {
    matched = matched || segment.side === focus.side;
  }
  return matched;
};

const fillMatchesFocus = (
  fill: OrderViewEvent,
  focus: OrderViewFocusState,
  focusActive: boolean
): boolean => {
  if (!focusActive) return true;

  let matched = false;
  const fillOrderKey = toOrderKey(fill);
  if (focus.orderKey && fillOrderKey) {
    matched = matched || fillOrderKey === focus.orderKey;
  }
  if (focus.eventKey) {
    matched = matched || String(fill.event_key || '') === focus.eventKey;
  }
  const fillSide = normalizeOrderSide(fill.side);
  const fillPrice = toFiniteNumber(fill.px);
  if (focus.side && focus.price !== null) {
    matched = matched || (fillSide === focus.side && samePrice(fillPrice, focus.price));
  } else if (focus.price !== null) {
    matched = matched || samePrice(fillPrice, focus.price);
  } else if (focus.side) {
    matched = matched || fillSide === focus.side;
  }
  return matched;
};

function OrderViewChartImpl({
  leg,
  priceSeries,
  lifetimeSegments,
  candles,
  candleSource,
  fills,
  focus,
  showBids,
  showAsks,
  showFills,
  showBboLines,
  bestBid,
  bestAsk,
}: OrderViewChartProps) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const chartRef = useRef<IChartApi | null>(null);
  const seriesRef = useRef<ISeriesApi<'Candlestick', UTCTimestamp> | null>(null);
  const markersRef = useRef<{ setMarkers: (markers: SeriesMarker<UTCTimestamp>[]) => void } | null>(null);
  const lifetimeSeriesRef = useRef<Map<string, ISeriesApi<'Line', UTCTimestamp>>>(new Map());
  const lastPriceLineRef = useRef<IPriceLine | null>(null);
  const bestBidLineRef = useRef<IPriceLine | null>(null);
  const bestAskLineRef = useRef<IPriceLine | null>(null);
  const lastDataLengthRef = useRef(0);
  const lastDataTimeRef = useRef<UTCTimestamp | null>(null);
  const didInitialFitRef = useRef(false);
  const [hoveredCandle, setHoveredCandle] = useState<CandlestickData<UTCTimestamp> | null>(null);
  const chartLocale = useMemo(() => resolveChartLocale(), []);

  const candleData = useMemo(() => {
    const fromRows = toCandlestickDataFromRows(candles);
    if (fromRows.length > 0) return fromRows;
    return toCandlestickDataFromPriceSeries(priceSeries, leg);
  }, [candles, leg, priceSeries]);
  const latestCandle = candleData.length > 0 ? candleData[candleData.length - 1] : null;

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const chart = createChart(container, {
      width: Math.max(container.clientWidth, 640),
      height: Math.max(container.clientHeight, 280),
      localization: {
        locale: chartLocale,
      },
      layout: {
        background: { type: ColorType.Solid, color: colors.bg.surface },
        textColor: colors.text.secondary,
      },
      grid: {
        vertLines: { color: colors.border.DEFAULT },
        horzLines: { color: colors.border.DEFAULT },
      },
      rightPriceScale: {
        borderColor: colors.border.DEFAULT,
      },
      timeScale: {
        borderColor: colors.border.DEFAULT,
        timeVisible: true,
        secondsVisible: true,
      },
      handleScroll: {
        mouseWheel: true,
        pressedMouseMove: true,
        horzTouchDrag: true,
        vertTouchDrag: true,
      },
      handleScale: {
        axisPressedMouseMove: true,
        mouseWheel: true,
        pinch: true,
      },
      crosshair: {
        vertLine: { color: colors.accent.muted },
        horzLine: { color: colors.accent.muted },
      },
    });

    const candleSeries = chart.addSeries(CandlestickSeries, {
      upColor: colors.semantic.success.DEFAULT,
      downColor: colors.semantic.danger.DEFAULT,
      wickUpColor: colors.semantic.success.light,
      wickDownColor: colors.semantic.danger.light,
      borderUpColor: colors.semantic.success.DEFAULT,
      borderDownColor: colors.semantic.danger.DEFAULT,
      priceLineVisible: true,
      lastValueVisible: true,
      priceFormat: {
        type: 'price',
        precision: ORDER_VIEW_PRICE_PRECISION,
        minMove: ORDER_VIEW_PRICE_MIN_MOVE,
      },
    });

    chartRef.current = chart;
    seriesRef.current = candleSeries as ISeriesApi<'Candlestick', UTCTimestamp>;
    markersRef.current = createSeriesMarkers(candleSeries as ISeriesApi<'Candlestick', UTCTimestamp>);

    const resizeObserver = new ResizeObserver(() => {
      if (!chartRef.current || !containerRef.current) return;
      chartRef.current.applyOptions({
        width: Math.max(containerRef.current.clientWidth, 320),
        height: Math.max(containerRef.current.clientHeight, 220),
      });
    });
    resizeObserver.observe(container);

    return () => {
      resizeObserver.disconnect();
      if (seriesRef.current) {
        if (lastPriceLineRef.current) {
          seriesRef.current.removePriceLine(lastPriceLineRef.current);
          lastPriceLineRef.current = null;
        }
        if (bestBidLineRef.current) {
          seriesRef.current.removePriceLine(bestBidLineRef.current);
          bestBidLineRef.current = null;
        }
        if (bestAskLineRef.current) {
          seriesRef.current.removePriceLine(bestAskLineRef.current);
          bestAskLineRef.current = null;
        }
      }
      if (chartRef.current) {
        for (const [, lifetimeSeries] of lifetimeSeriesRef.current) {
          (chartRef.current as any).removeSeries?.(lifetimeSeries);
        }
      }
      lifetimeSeriesRef.current.clear();
      markersRef.current = null;
      seriesRef.current = null;
      lastDataLengthRef.current = 0;
      lastDataTimeRef.current = null;
      didInitialFitRef.current = false;
      chart.remove();
      chartRef.current = null;
    };
  }, [chartLocale]);

  useEffect(() => {
    const series = seriesRef.current;
    const chart = chartRef.current;
    if (!series || !chart) return;

    if (candleData.length === 0) {
      series.setData([]);
      if (lastPriceLineRef.current) {
        series.removePriceLine(lastPriceLineRef.current);
        lastPriceLineRef.current = null;
      }
      lastDataLengthRef.current = 0;
      lastDataTimeRef.current = null;
      didInitialFitRef.current = false;
      setHoveredCandle(null);
      return;
    }

    const latest = candleData[candleData.length - 1];
    const canApplyIncremental =
      (candleData.length === lastDataLengthRef.current + 1 &&
        (lastDataTimeRef.current === null || latest.time > lastDataTimeRef.current)) ||
      (candleData.length === lastDataLengthRef.current &&
        lastDataTimeRef.current !== null &&
        latest.time === lastDataTimeRef.current);

    if (canApplyIncremental && lastDataLengthRef.current > 0) {
      series.update(latest);
    } else {
      const timeScale = chart.timeScale();
      const visibleRange =
        typeof (timeScale as any).getVisibleRange === 'function'
          ? (timeScale as any).getVisibleRange()
          : null;
      series.setData(candleData);
      if (
        visibleRange &&
        typeof (timeScale as any).setVisibleRange === 'function' &&
        Number.isFinite(Number((visibleRange as any).from)) &&
        Number.isFinite(Number((visibleRange as any).to))
      ) {
        (timeScale as any).setVisibleRange(visibleRange);
      }
    }
    if (lastPriceLineRef.current) {
      series.removePriceLine(lastPriceLineRef.current);
    }
    lastPriceLineRef.current = series.createPriceLine({
      price: latest.close,
      color: colors.accent.DEFAULT,
      lineWidth: 1,
      lineStyle: LineStyle.Solid,
      axisLabelVisible: true,
      title: 'Last',
    });

    if (!didInitialFitRef.current && candleData.length > 1) {
      chart.timeScale().fitContent();
      didInitialFitRef.current = true;
    }
    lastDataLengthRef.current = candleData.length;
    lastDataTimeRef.current = latest.time;
  }, [candleData]);

  useEffect(() => {
    const series = seriesRef.current;
    if (!series) return;

    const clearBidLine = () => {
      if (!bestBidLineRef.current) return;
      series.removePriceLine(bestBidLineRef.current);
      bestBidLineRef.current = null;
    };
    const clearAskLine = () => {
      if (!bestAskLineRef.current) return;
      series.removePriceLine(bestAskLineRef.current);
      bestAskLineRef.current = null;
    };

    if (!showBboLines) {
      clearBidLine();
      clearAskLine();
      return;
    }

    const bid = toFiniteNumber(bestBid);
    if (bid === null || bid <= 0) {
      clearBidLine();
    } else {
      clearBidLine();
      bestBidLineRef.current = series.createPriceLine({
        price: quantizePrice(bid),
        color: colors.semantic.success.light,
        lineWidth: 1,
        lineStyle: LineStyle.Dashed,
        axisLabelVisible: true,
        title: 'Bid',
      });
    }

    const ask = toFiniteNumber(bestAsk);
    if (ask === null || ask <= 0) {
      clearAskLine();
    } else {
      clearAskLine();
      bestAskLineRef.current = series.createPriceLine({
        price: quantizePrice(ask),
        color: colors.semantic.danger.light,
        lineWidth: 1,
        lineStyle: LineStyle.Dashed,
        axisLabelVisible: true,
        title: 'Ask',
      });
    }
  }, [bestAsk, bestBid, showBboLines]);

  useEffect(() => {
    const chart = chartRef.current;
    const series = seriesRef.current;
    if (!chart || !series || typeof chart.subscribeCrosshairMove !== 'function') return;
    const handleCrosshairMove = (param: any) => {
      const row = param?.seriesData?.get?.(series);
      if (!row || typeof row !== 'object') {
        setHoveredCandle(null);
        return;
      }
      const open = toFiniteNumber((row as Record<string, unknown>).open);
      const high = toFiniteNumber((row as Record<string, unknown>).high);
      const low = toFiniteNumber((row as Record<string, unknown>).low);
      const close = toFiniteNumber((row as Record<string, unknown>).close);
      const time = toFiniteNumber((row as Record<string, unknown>).time);
      if (open === null || high === null || low === null || close === null || time === null) {
        setHoveredCandle(null);
        return;
      }
      setHoveredCandle({
        time: Math.trunc(time) as UTCTimestamp,
        open,
        high,
        low,
        close,
      });
    };
    chart.subscribeCrosshairMove(handleCrosshairMove);
    return () => {
      if (typeof chart.unsubscribeCrosshairMove === 'function') {
        chart.unsubscribeCrosshairMove(handleCrosshairMove);
      }
    };
  }, []);

  useEffect(() => {
    const chart = chartRef.current;
    if (!chart) return;
    const activeIds = new Set<string>();
    const latestCandleTsMs = latestCandle ? Number(latestCandle.time) * 1000 : null;
    const visibleSegments = lifetimeSegments.slice(-ORDER_VIEW_MAX_SEGMENTS_RENDERED);
    const focusActive = isFocusActive(focus);

    for (const segment of visibleSegments) {
      if (segment.side === 'bid' && !showBids) continue;
      if (segment.side === 'ask' && !showAsks) continue;
      const price = toFiniteNumber(segment.price);
      if (price == null || price <= 0) continue;
      let endTsMs = segment.end_ts_ms;
      if (endTsMs == null) {
        endTsMs =
          latestCandleTsMs != null
            ? Math.max(latestCandleTsMs, segment.start_ts_ms)
            : segment.start_ts_ms;
      }
      const startTsMs = Math.max(1, Math.trunc(segment.start_ts_ms));
      const safeEndTsMs = Math.max(startTsMs, Math.trunc(endTsMs));
      const segmentId = String(segment.segment_id || `${segment.order_key}:${startTsMs}:${safeEndTsMs}`);
      activeIds.add(segmentId);

      let lineSeries = lifetimeSeriesRef.current.get(segmentId);
      const baseColor =
        segment.side === 'bid'
          ? colors.semantic.success.light
          : segment.side === 'ask'
            ? colors.semantic.danger.light
            : colors.text.muted;
      const matchesFocus = segmentMatchesFocus(segment, focus, focusActive);
      const isDimmed = focusActive && !matchesFocus;
      if (!lineSeries) {
        lineSeries = chart.addSeries(LineSeries, {
          color: baseColor,
          lineWidth: 1,
          lineStyle: LineStyle.Dashed,
          priceLineVisible: false,
          lastValueVisible: false,
          crosshairMarkerVisible: false,
        }) as ISeriesApi<'Line', UTCTimestamp>;
        lifetimeSeriesRef.current.set(segmentId, lineSeries);
      }
      if (typeof (lineSeries as any).applyOptions === 'function') {
        (lineSeries as any).applyOptions({
          color: isDimmed ? withAlpha(baseColor, 0.24) : baseColor,
          lineWidth: focusActive && matchesFocus ? 2 : 1,
          lineStyle: focusActive && matchesFocus ? LineStyle.Solid : LineStyle.Dashed,
        });
      }
      lineSeries.setData([
        { time: toUtcTimestamp(startTsMs), value: quantizePrice(price) },
        { time: toUtcTimestamp(safeEndTsMs), value: quantizePrice(price) },
      ] as any);
    }

    for (const [segmentId, lineSeries] of lifetimeSeriesRef.current.entries()) {
      if (activeIds.has(segmentId)) continue;
      (chart as any).removeSeries?.(lineSeries);
      lifetimeSeriesRef.current.delete(segmentId);
    }
  }, [focus, latestCandle, lifetimeSegments, showAsks, showBids]);

  useEffect(() => {
    const markersApi = markersRef.current;
    if (!markersApi) return;
    if (!showFills) {
      markersApi.setMarkers([]);
      return;
    }

    const markers: SeriesMarker<UTCTimestamp>[] = [];
    const maxMarkers = 500;
    const focusActive = isFocusActive(focus);
    for (let index = 0; index < fills.length && markers.length < maxMarkers; index += 1) {
      const fill = fills[index];
      const tsMs = toFiniteNumber(fill.ts_ms);
      const price = toFiniteNumber(fill.px);
      if (tsMs === null || tsMs <= 0 || price === null) continue;
      const side = String(fill.side || '').toLowerCase();
      const baseColor =
        side === 'sell' || side === 'ask'
          ? colors.semantic.danger.light
          : colors.semantic.success.light;
      const matchesFocus = fillMatchesFocus(fill, focus, focusActive);
      const isDimmed = focusActive && !matchesFocus;
      markers.push({
        time: toUtcTimestamp(tsMs),
        position: side === 'sell' || side === 'ask' ? 'atPriceBottom' : 'atPriceTop',
        shape: side === 'sell' || side === 'ask' ? 'arrowDown' : 'arrowUp',
        color: isDimmed ? withAlpha(baseColor, 0.26) : baseColor,
        text: '',
        price: quantizePrice(price),
      });
    }

    markers.sort((lhs, rhs) => Number(lhs.time) - Number(rhs.time));
    markersApi.setMarkers(markers);
  }, [fills, focus, showFills]);

  const tooltipCandle = hoveredCandle ?? latestCandle;

  return (
    <div className="relative w-full h-full min-h-[260px]">
      <div
        data-testid="order-view-chart-tooltip"
        className="absolute top-2 left-2 z-10 rounded border px-2 py-1 text-[11px]"
        style={{
          borderColor: colors.border.DEFAULT,
          backgroundColor: colors.bg.surface,
          color: colors.text.secondary,
        }}
      >
        {tooltipCandle ? (
          <span>
            O: {formatTooltipPrice(tooltipCandle.open)} H: {formatTooltipPrice(tooltipCandle.high)} L:{' '}
            {formatTooltipPrice(tooltipCandle.low)} C: {formatTooltipPrice(tooltipCandle.close)}
          </span>
        ) : (
          <span>No candles</span>
        )}{' '}
        <span style={{ color: colors.text.muted }}>
          ({String(candleSource || 'unknown').toUpperCase()})
        </span>
      </div>
      <button
        type="button"
        className="absolute top-2 right-2 z-10 rounded border px-2 py-1 text-[11px]"
        style={{
          borderColor: colors.border.DEFAULT,
          backgroundColor: colors.bg.surface,
          color: colors.text.primary,
        }}
        onClick={() => chartRef.current?.timeScale().fitContent()}
      >
        Reset Zoom
      </button>
      <div ref={containerRef} data-testid="order-view-chart" className="w-full h-full min-h-[260px]" />
    </div>
  );
}

export const OrderViewChart = memo(OrderViewChartImpl);
