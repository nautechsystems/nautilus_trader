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
import type { MakerV4LegSnapshot, MakerV4QuoteSnapshot, SignalStrategy } from '@/types';

type MakerV4DisplayRow = SignalStrategy & {
  _quoteSnapshot: MakerV4QuoteSnapshot | null;
  _makerLeg: MakerV4LegSnapshot | null;
  _hedgeLeg: MakerV4LegSnapshot | null;
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
  const lines = [
    formatLegLine(leg),
    `Bid / Mid / Ask: ${formatLegPrices(leg)}`,
    `Age: ${leg.age_ms != null ? fmtAgeSec(Number(leg.age_ms)) : fallback}`,
  ];
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
  return (
    <SimpleTooltip content={tooltip ? <pre className="whitespace-pre-wrap">{tooltip}</pre> : undefined} delay={150}>
      <div className="flex min-w-[250px] cursor-help flex-col gap-0.5">
        <span className="text-xs font-semibold text-neutral-100">{formatLegLine(leg)}</span>
        <span className="font-mono text-xs text-neutral-300">{formatLegPrices(leg)}</span>
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
      cell: ({ row }) => <span className="font-mono text-xs text-neutral-100">{row.original.id}</span>,
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
      accessorFn: (row) => coerceNumber(row._quoteSnapshot?.effective_spread_bps) ?? Number.NEGATIVE_INFINITY,
      id: 'effective_spread',
      header: 'Effective Spread',
      cell: ({ row }) => {
        const effectiveSpreadBps = coerceNumber(row.original._quoteSnapshot?.effective_spread_bps);
        const quotedSpreadBps = coerceNumber(row.original._quoteSnapshot?.quoted_spread_bps);
        const makerFeeBps = coerceNumber(row.original._quoteSnapshot?.expected_maker_fee_bps);
        const hedgeFeeBps = coerceNumber(row.original._quoteSnapshot?.assumed_hedge_fee_bps);
        const tooltip = [
          `Effective spread: ${formatBps(effectiveSpreadBps)}`,
          `Quoted spread: ${formatBps(quotedSpreadBps)}`,
          `Expected maker fee: ${formatBps(makerFeeBps)}`,
          `Assumed hedge fee: ${formatBps(hedgeFeeBps)}`,
        ].join('\n');
        return (
          <SimpleTooltip content={<pre className="whitespace-pre-wrap">{tooltip}</pre>} delay={150}>
            <span
              className="cursor-help font-mono text-xs"
              style={{
                color:
                  effectiveSpreadBps == null
                    ? colors.text.muted
                    : effectiveSpreadBps >= 0
                      ? colors.semantic.success.DEFAULT
                      : colors.semantic.danger.DEFAULT,
              }}
            >
              {formatBps(effectiveSpreadBps)}
            </span>
          </SimpleTooltip>
        );
      },
    },
    {
      id: 'hedge_status',
      header: 'Hedge',
      cell: ({ row }) => {
        const snapshot = row.original._quoteSnapshot;
        const hedgeReady = snapshot?.hedge_ready === true;
        const disabledReason = snapshot?.hedge_disabled_reason ?? (row.original.tradeable === false ? 'blocked' : '—');
        const route = snapshot?.hedge_route ?? '—';
        const hedgeLatencyMs = coerceNumber(snapshot?.hedge_latency_ms);
        const hedgeSlippageBps = coerceNumber(snapshot?.hedge_slippage_bps_vs_mid);
        const subLabel = hedgeLatencyMs != null ? `${route} · ${hedgeLatencyMs} ms` : route;
        const tooltip = [
          `Route: ${route}`,
          `Reason: ${disabledReason}`,
          `Hedge latency: ${hedgeLatencyMs != null ? `${hedgeLatencyMs} ms` : '—'}`,
          `Hedge slippage vs mid: ${formatBps(hedgeSlippageBps)}`,
        ].join('\n');
        return (
          <StatusPill
            status={hedgeReady ? 'ok' : snapshot?.hedge_disabled_reason ? 'warning' : 'muted'}
            label={hedgeReady ? 'Ready' : 'Blocked'}
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
