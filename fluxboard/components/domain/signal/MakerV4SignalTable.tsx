import { useMemo, type ReactNode } from 'react';
import { type ColumnDef } from '@tanstack/react-table';

import { DataTable } from '@/components/ui/table/DataTable';
import { SimpleTooltip } from '@/components/ui/tooltip/Tooltip';
import { StatusPill } from '@/components/shared/StatusPill';
import { colors } from '@/lib/tokens';
import { fmtAgeSec, fmtPriceSignal } from '@/utils';
import { formatLocal } from '@/utils/time';
import { deriveStrategyStatus, describeTradingStatus } from '@/utils/strategyStatus';
import { resolveSignalRunning } from '@/utils/signalRunState';
import type { MakerV4LegSnapshot, MakerV4OperatorPayload, MakerV4QuoteSnapshot, SignalStrategy } from '@/types';

type MakerV4DisplayRow = SignalStrategy & {
  _quoteSnapshot: MakerV4QuoteSnapshot | null;
  _makerLeg: MakerV4LegSnapshot | null;
  _hedgeLeg: MakerV4LegSnapshot | null;
  _operator: MakerV4OperatorPayload | null;
  _statusLabel: ReturnType<typeof describeTradingStatus>;
  _lastUpdateMs: number | null;
  _lastAgeMs: number | null;
};

function coerceNumber(value: unknown): number | null {
  if (typeof value === 'number') {
    return Number.isFinite(value) ? value : null;
  }
  if (typeof value === 'string') {
    const trimmed = value.trim();
    if (!trimmed) return null;
    const parsed = Number(trimmed);
    return Number.isFinite(parsed) ? parsed : null;
  }
  return null;
}

function formatBps(value: number | null | undefined): string {
  if (value == null || !Number.isFinite(value)) return '—';
  return `${value.toFixed(1)} bps`;
}

function formatCompactBps(value: number | null | undefined): string {
  if (value == null || !Number.isFinite(value)) return '—';
  return value.toFixed(1);
}

function formatExecutionMode(mode: string | null | undefined): string {
  return String(mode || '').trim().toLowerCase() === 'take_take' ? 'Take-Take' : 'Maker-Hedge';
}

function formatStateWord(value: unknown): string | null {
  const text = String(value ?? '').trim().toLowerCase();
  return text ? text : null;
}

function formatCancelAfter(cancelAfterMs: number | null | undefined): string {
  if (cancelAfterMs == null || !Number.isFinite(cancelAfterMs)) return '—';
  if (cancelAfterMs < 1000) return `${cancelAfterMs} ms`;
  return `${(cancelAfterMs / 1000).toFixed(cancelAfterMs % 1000 === 0 ? 0 : 1)} s`;
}

function instrumentRouteSuffix(instrumentId: string | null | undefined): string | null {
  const raw = instrumentId?.trim();
  if (!raw) return null;
  const suffix = raw.split('.').pop()?.trim();
  return suffix || null;
}

function resolveConfiguredHedgeRoute(row: MakerV4DisplayRow): string {
  const operatorRoute = row._operator?.hedge_policy?.route?.trim();
  if (operatorRoute) return operatorRoute;
  const snapshotRoute = row._quoteSnapshot?.hedge_route?.trim();
  if (snapshotRoute) return snapshotRoute;
  const hedgeRoute = row._hedgeLeg?.route;
  if (typeof hedgeRoute === 'string' && hedgeRoute.trim()) return hedgeRoute.trim();

  const legEntries = row.legs_order?.map((key) => row.legs?.[key] ?? null)
    ?? Object.values(row.legs ?? {});
  for (const leg of legEntries) {
    if (!leg) continue;
    const exchange = String(leg.exchange || '').trim().toLowerCase();
    if (exchange !== 'ibkr') continue;
    const explicitRoute = typeof leg.route === 'string' ? leg.route.trim() : '';
    if (explicitRoute) return explicitRoute;
    const suffix = instrumentRouteSuffix(typeof leg.instrument_id === 'string' ? leg.instrument_id : null);
    if (suffix) return suffix;
  }

  return '—';
}

function deriveLastUpdateMs(snapshot: MakerV4QuoteSnapshot | null): number | null {
  if (!snapshot) return null;
  const candidates = [
    coerceNumber(snapshot.ts_ms),
    coerceNumber(snapshot.maker_leg?.ts_ms),
    coerceNumber(snapshot.hedge_leg?.ts_ms),
    coerceNumber(snapshot.ref_leg?.ts_ms),
  ].filter((value): value is number => value != null);
  if (candidates.length === 0) return null;
  return Math.max(...candidates);
}

function deriveLastAgeMs(snapshot: MakerV4QuoteSnapshot | null, lastUpdateMs: number | null): number | null {
  const snapshotAges = [
    coerceNumber(snapshot?.maker_leg?.age_ms),
    coerceNumber(snapshot?.hedge_leg?.age_ms),
    coerceNumber(snapshot?.ref_leg?.age_ms),
  ].filter((value): value is number => value != null);
  if (snapshotAges.length > 0) {
    return Math.min(...snapshotAges);
  }
  if (lastUpdateMs == null) return null;
  return Math.max(0, Date.now() - lastUpdateMs);
}

function formatLegLine(leg: MakerV4LegSnapshot | null | undefined): string {
  if (!leg) return '—';
  const venue = leg.venue?.trim() || '—';
  const instrument = leg.instrument_id?.trim() || leg.symbol?.trim() || '—';
  return `${venue} ${instrument}`;
}

function formatLegPrices(leg: MakerV4LegSnapshot | null | undefined): string {
  if (!leg) return '—';
  const bid = coerceNumber(leg.bid);
  const ask = coerceNumber(leg.ask);
  const mid = coerceNumber(leg.mid) ?? ((bid != null && ask != null) ? (bid + ask) / 2 : null);
  if (bid == null && ask == null && mid == null) return '—';

  const parts = [bid, mid, ask].map((value) => (value == null ? '—' : fmtPriceSignal(value)));
  return `${parts[0]} / ${parts[1]} / ${parts[2]}`;
}

function buildLegTooltip(leg: MakerV4LegSnapshot | null | undefined, fallback: string): string | undefined {
  if (!leg) return undefined;
  const feedState = formatStateWord(leg.feed_state);
  const quoteState = formatStateWord(leg.quote_state);
  const pricingUsable = typeof leg.pricing_usable === 'boolean' ? leg.pricing_usable : null;
  const hedgeUsable = typeof leg.hedge_usable === 'boolean' ? leg.hedge_usable : null;
  const reasonCode = String(leg.reason_code ?? '').trim();
  const lines = [
    formatLegLine(leg),
    `Bid / Mid / Ask: ${formatLegPrices(leg)}`,
    `Age: ${leg.age_ms != null ? fmtAgeSec(Number(leg.age_ms)) : fallback}`,
  ];
  if (feedState || quoteState) {
    lines.push(`Feed: ${feedState ?? '—'} · Quote: ${quoteState ?? '—'}`);
  }
  if (pricingUsable != null || hedgeUsable != null) {
    lines.push(
      `Pricing usable: ${pricingUsable == null ? '—' : pricingUsable ? 'yes' : 'no'} · Hedge usable: ${hedgeUsable == null ? '—' : hedgeUsable ? 'yes' : 'no'}`,
    );
  }
  if (reasonCode) {
    lines.push(`Reason: ${reasonCode}`);
  }
  if (leg.symbol && leg.instrument_id && leg.symbol !== leg.instrument_id) {
    lines.splice(1, 0, `Symbol: ${leg.symbol}`);
  }
  return lines.join('\n');
}

function LegSummary({
  leg,
  fallbackAgeLabel,
}: {
  leg: MakerV4LegSnapshot | null | undefined;
  fallbackAgeLabel: string;
}): ReactNode {
  if (!leg) {
    return <span className="text-xs text-neutral-500">—</span>;
  }
  const tooltip = buildLegTooltip(leg, fallbackAgeLabel);
  const feedState = formatStateWord(leg.feed_state);
  const quoteState = formatStateWord(leg.quote_state);
  const healthLabel = feedState || quoteState
    ? `Feed ${feedState ?? '—'} · Quote ${quoteState ?? '—'}`
    : null;
  return (
    <SimpleTooltip content={tooltip ? <pre className="whitespace-pre-wrap">{tooltip}</pre> : undefined} delay={150}>
      <div className="flex min-w-[250px] cursor-help flex-col gap-0.5">
        <span className="text-xs font-semibold text-neutral-100">{formatLegLine(leg)}</span>
        <span className="font-mono text-xs text-neutral-300">{formatLegPrices(leg)}</span>
        {healthLabel && (
          <span className="text-[10px] text-neutral-500">{healthLabel}</span>
        )}
      </div>
    </SimpleTooltip>
  );
}

export default function MakerV4SignalTable({
  rows,
  strategies,
  loading = false,
  nowProvider = () => Date.now(),
}: {
  rows?: SignalStrategy[];
  strategies?: SignalStrategy[];
  loading?: boolean;
  nowProvider?: () => number;
}) {
  const sourceRows = rows ?? strategies ?? [];
  const data = useMemo<MakerV4DisplayRow[]>(() => {
    return sourceRows.map((row) => {
      const quoteSnapshot = row.maker_v4?.quote_snapshot ?? null;
      const lastUpdateMs = deriveLastUpdateMs(quoteSnapshot);
      const lastAgeMs = deriveLastAgeMs(quoteSnapshot, lastUpdateMs) ?? (lastUpdateMs != null ? Math.max(0, nowProvider() - lastUpdateMs) : null);
      const status = deriveStrategyStatus({
        running: resolveSignalRunning(row, nowProvider()),
        trading: row.params?.bot_on,
        blocked: Boolean(row.blocked) || quoteSnapshot?.hedge_ready === false,
      });

      return {
        ...row,
        _quoteSnapshot: quoteSnapshot,
        _makerLeg: quoteSnapshot?.maker_leg ?? null,
        _hedgeLeg: quoteSnapshot?.hedge_leg ?? null,
        _operator: row.maker_v4?.operator ?? null,
        _statusLabel: describeTradingStatus(status),
        _lastUpdateMs: lastUpdateMs,
        _lastAgeMs: lastAgeMs,
      };
    });
  }, [nowProvider, sourceRows]);

  const columns = useMemo<ColumnDef<MakerV4DisplayRow>[]>(() => [
    {
      accessorKey: 'id',
      header: 'Strategy',
      cell: ({ row }) => {
        const feeAssumptions = row.original._operator?.fee_assumptions;
        const tooltip = [
          `Strategy: ${row.original.id}`,
          `IBKR fee plan: ${feeAssumptions?.ibkr_fee_plan ?? '—'}`,
          `IBKR fee min: ${
            feeAssumptions?.ibkr_fee_min_usd == null
              ? '—'
              : `$${Number(feeAssumptions.ibkr_fee_min_usd).toFixed(2)}`
          }`,
          `HL taker fee: ${
            feeAssumptions?.hl_taker_fee_bps == null ? '—' : `${Number(feeAssumptions.hl_taker_fee_bps).toFixed(2)} bps`
          }`,
          `HL maker fee: ${
            feeAssumptions?.hl_maker_fee_bps == null ? '—' : `${Number(feeAssumptions.hl_maker_fee_bps).toFixed(2)} bps`
          }`,
          `Assumed hedge fee: ${
            feeAssumptions?.assumed_hedge_fee_bps == null
              ? '—'
              : `${Number(feeAssumptions.assumed_hedge_fee_bps).toFixed(2)} bps`
          }`,
        ].join('\n');
        return (
          <SimpleTooltip content={<pre className="whitespace-pre-wrap">{tooltip}</pre>} delay={150}>
            <span className="cursor-help font-mono text-xs text-neutral-100" title={tooltip}>
              {row.original.id}
            </span>
          </SimpleTooltip>
        );
      },
    },
    {
      id: 'trading',
      header: 'Trading',
      cell: ({ row }) => (
        <StatusPill
          variant={row.original._statusLabel.variant}
          label={row.original._statusLabel.label}
          subLabel={row.original._statusLabel.subLabel}
          size="xs"
          tone="subtle"
        />
      ),
    },
    {
      id: 'mode',
      header: 'Mode',
      cell: ({ row }) => {
        const operator = row.original._operator;
        const hedgePolicy = operator?.hedge_policy;
        const route = resolveConfiguredHedgeRoute(row.original);
        const tif = hedgePolicy?.time_in_force ?? '—';
        const tooltip = [
          `Execution mode: ${formatExecutionMode(operator?.execution_mode)}`,
          `Behavior: ${operator?.behavior ?? 'maker'}`,
          `Route: ${route}`,
          `Time in force: ${tif}`,
          `Outside RTH: ${hedgePolicy?.outside_rth == null ? '—' : hedgePolicy.outside_rth ? 'true' : 'false'}`,
          `Include overnight: ${hedgePolicy?.include_overnight == null ? '—' : hedgePolicy.include_overnight ? 'true' : 'false'}`,
          `Cancel after: ${formatCancelAfter(hedgePolicy?.cancel_after_ms)}`,
        ].join('\n');
        return (
          <SimpleTooltip content={<pre className="whitespace-pre-wrap">{tooltip}</pre>} delay={150}>
            <div className="flex min-w-[140px] cursor-help flex-col gap-0.5">
              <span className="text-xs font-semibold text-neutral-100">
                {formatExecutionMode(operator?.execution_mode)}
              </span>
              <span className="font-mono text-xs text-neutral-400">
                {route} · {tif}
              </span>
            </div>
          </SimpleTooltip>
        );
      },
    },
    {
      id: 'maker_market',
      header: 'Maker Market',
      cell: ({ row }) => (
        <LegSummary
          leg={row.original._makerLeg}
          fallbackAgeLabel={row.original._lastAgeMs != null ? fmtAgeSec(row.original._lastAgeMs) : '—'}
        />
      ),
    },
    {
      id: 'hedge_market',
      header: 'Hedge Market',
      cell: ({ row }) => (
        <LegSummary
          leg={row.original._hedgeLeg}
          fallbackAgeLabel={
            row.original._quoteSnapshot?.ibkr_quote_age_ms != null
              ? fmtAgeSec(Number(row.original._quoteSnapshot.ibkr_quote_age_ms))
              : (row.original._lastAgeMs != null ? fmtAgeSec(row.original._lastAgeMs) : '—')
          }
        />
      ),
    },
    {
      accessorFn: (row) => coerceNumber(row._quoteSnapshot?.mid_spread_bps) ?? Number.NEGATIVE_INFINITY,
      id: 'mid_spread',
      header: 'Mid Spread',
      cell: ({ row }) => {
        const midSpreadBps = coerceNumber(row.original._quoteSnapshot?.mid_spread_bps);
        const makerMid = coerceNumber(row.original._makerLeg?.mid);
        const hedgeMid = coerceNumber((row.original._hedgeLeg ?? row.original._quoteSnapshot?.ref_leg)?.mid);
        const tooltip = [
          'Strategy-published maker-vs-hedge midpoint spread',
          `Mid spread: ${formatBps(midSpreadBps)}`,
          `Maker mid: ${makerMid == null ? '—' : fmtPriceSignal(makerMid)}`,
          `Hedge mid: ${hedgeMid == null ? '—' : fmtPriceSignal(hedgeMid)}`,
        ].join('\n');
        return (
          <SimpleTooltip content={<pre className="whitespace-pre-wrap">{tooltip}</pre>} delay={150}>
            <span
              className="cursor-help font-mono text-xs"
              style={{
                color:
                  midSpreadBps == null
                    ? colors.text.muted
                    : midSpreadBps >= 0
                      ? colors.semantic.success.DEFAULT
                      : colors.semantic.danger.DEFAULT,
              }}
            >
              {formatBps(midSpreadBps)}
            </span>
          </SimpleTooltip>
        );
      },
    },
    {
      accessorFn: (row) => {
        const bid = coerceNumber(row._quoteSnapshot?.arb_bid_spread_bps);
        const ask = coerceNumber(row._quoteSnapshot?.arb_ask_spread_bps);
        return Math.max(bid ?? Number.NEGATIVE_INFINITY, ask ?? Number.NEGATIVE_INFINITY);
      },
      id: 'arb_spread',
      header: 'Arb Spread',
      cell: ({ row }) => {
        const arbBidSpreadBps = coerceNumber(row.original._quoteSnapshot?.arb_bid_spread_bps);
        const arbAskSpreadBps = coerceNumber(row.original._quoteSnapshot?.arb_ask_spread_bps);
        const makerBid = coerceNumber(row.original._makerLeg?.bid);
        const makerAsk = coerceNumber(row.original._makerLeg?.ask);
        const hedgeBid = coerceNumber((row.original._hedgeLeg ?? row.original._quoteSnapshot?.ref_leg)?.bid);
        const hedgeAsk = coerceNumber((row.original._hedgeLeg ?? row.original._quoteSnapshot?.ref_leg)?.ask);
        const tooltip = [
          'Strategy-published arbitrage bounds',
          `Bid arb spread: ${formatBps(arbBidSpreadBps)} (hedge bid vs maker ask)`,
          `Ask arb spread: ${formatBps(arbAskSpreadBps)} (maker bid vs hedge ask)`,
          `Maker bid / ask: ${makerBid == null ? '—' : fmtPriceSignal(makerBid)} / ${makerAsk == null ? '—' : fmtPriceSignal(makerAsk)}`,
          `Hedge bid / ask: ${hedgeBid == null ? '—' : fmtPriceSignal(hedgeBid)} / ${hedgeAsk == null ? '—' : fmtPriceSignal(hedgeAsk)}`,
        ].join('\n');
        return (
          <SimpleTooltip content={<pre className="whitespace-pre-wrap">{tooltip}</pre>} delay={150}>
            <div className="flex cursor-help flex-col font-mono text-[11px] leading-tight">
              <span style={{ color: arbBidSpreadBps != null && arbBidSpreadBps >= 0 ? colors.semantic.success.DEFAULT : colors.text.default }}>
                {`B ${formatCompactBps(arbBidSpreadBps)}`}
              </span>
              <span style={{ color: arbAskSpreadBps != null && arbAskSpreadBps >= 0 ? colors.semantic.success.DEFAULT : colors.text.default }}>
                {`A ${formatCompactBps(arbAskSpreadBps)}`}
              </span>
            </div>
          </SimpleTooltip>
        );
      },
    },
    {
      id: 'hedge_status',
      header: 'Hedge',
      cell: ({ row }) => {
        const snapshot = row.original._quoteSnapshot;
        const hedgePolicy = row.original._operator?.hedge_policy;
        const hedgeBacklog = row.original._operator?.hedge_backlog;
        const hedgeReady = snapshot?.hedge_ready === true;
        const disabledReason = snapshot?.hedge_disabled_reason ?? (row.original.tradeable === false ? 'blocked' : '—');
        const route = resolveConfiguredHedgeRoute(row.original);
        const timeInForce = hedgePolicy?.time_in_force ?? '—';
        const hedgeLatencyMs = coerceNumber(snapshot?.hedge_latency_ms);
        const hedgeSlippageBps = coerceNumber(snapshot?.hedge_slippage_bps_vs_mid);
        const backlogQty = hedgeBacklog?.requested_qty == null ? '—' : String(hedgeBacklog.requested_qty);
        const backlogSide = hedgeBacklog?.side?.trim() || '—';
        const hasBacklog = Boolean(hedgeBacklog?.blocked_reason);
        const subLabel = hasBacklog
          ? `${backlogSide} ${backlogQty}`
          : (hedgeLatencyMs != null ? `${route} · ${hedgeLatencyMs} ms` : `${route} · ${timeInForce}`);
        const tooltip = [
          `Route: ${route}`,
          `Time in force: ${timeInForce}`,
          `Reason: ${disabledReason}`,
          `Hedge backlog: ${hasBacklog ? `${backlogSide} ${backlogQty} (${hedgeBacklog?.blocked_reason ?? 'blocked'})` : '—'}`,
          `Hedge latency: ${hedgeLatencyMs != null ? `${hedgeLatencyMs} ms` : '—'}`,
          `Hedge slippage vs mid: ${formatBps(hedgeSlippageBps)}`,
          `Outside RTH: ${hedgePolicy?.outside_rth == null ? '—' : hedgePolicy.outside_rth ? 'true' : 'false'}`,
          `Include overnight: ${hedgePolicy?.include_overnight == null ? '—' : hedgePolicy.include_overnight ? 'true' : 'false'}`,
          `Cancel after: ${formatCancelAfter(hedgePolicy?.cancel_after_ms)}`,
        ].join('\n');
        return (
          <StatusPill
            status={hedgeReady ? 'ok' : hasBacklog || snapshot?.hedge_disabled_reason ? 'warning' : 'muted'}
            label={hedgeReady ? 'Ready' : hasBacklog ? 'Backlog' : 'Blocked'}
            subLabel={subLabel}
            tooltip={tooltip}
            size="xs"
            tone="subtle"
          />
        );
      },
    },
    {
      accessorFn: (row) => row._lastUpdateMs ?? Number.NEGATIVE_INFINITY,
      id: 'last_update',
      header: 'Last Updated',
      cell: ({ row }) => {
        const lastUpdateMs = row.original._lastUpdateMs;
        const ageMs = row.original._lastAgeMs;
        if (lastUpdateMs == null) {
          return <span className="text-xs text-neutral-500">—</span>;
        }
        const tooltip = [
          `Last update: ${formatLocal(lastUpdateMs)}`,
          `Age: ${ageMs != null ? fmtAgeSec(ageMs) : '—'}`,
          `IBKR quote age: ${
            row.original._quoteSnapshot?.ibkr_quote_age_ms != null
              ? fmtAgeSec(Number(row.original._quoteSnapshot.ibkr_quote_age_ms))
              : '—'
          }`,
        ].join('\n');
        return (
          <SimpleTooltip content={<pre className="whitespace-pre-wrap">{tooltip}</pre>} delay={150}>
            <span className="cursor-help text-xs text-neutral-300">
              {formatLocal(lastUpdateMs)}
              {ageMs != null && <span className="text-neutral-500"> ({Math.max(0, Math.floor(ageMs / 1000))}s ago)</span>}
            </span>
          </SimpleTooltip>
        );
      },
    },
  ], []);

  return (
    <div data-testid="maker-v4-signal-table">
      <DataTable
        data={data}
        columns={columns}
        getRowId={(row) => row.id}
        sortable
        dense={false}
        loading={loading}
        emptyMessage={loading ? 'Loading strategies...' : 'No Maker V4 strategies found'}
        className="min-w-[1100px]"
        widthMode="content"
        columnWidthMode="explicit"
        mobileMode="table"
      />
    </div>
  );
}
