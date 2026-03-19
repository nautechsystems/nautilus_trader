import { useMemo } from 'react';

import type { SignalStrategy } from '@/types';

type EquitiesVariant = 'Maker' | 'Taker';

type DisplayRow = {
  strategy: SignalStrategy;
  asset: string;
  variant: EquitiesVariant;
  makerMarket: string;
  hedgeMarket: string;
  mode: string;
  midSpread: string;
  arbSpread: string;
  feeSummary: string;
};

const VARIANT_ORDER: Record<EquitiesVariant, number> = {
  Maker: 0,
  Taker: 1,
};

function resolveVariant(strategy: SignalStrategy): EquitiesVariant {
  return strategy.strategy_family === 'equities_taker' ? 'Taker' : 'Maker';
}

function resolveAsset(strategy: SignalStrategy): string {
  const metaAsset = String(strategy.meta?.base_asset ?? '').trim().toUpperCase();
  if (metaAsset) return metaAsset;
  return String(strategy.id ?? '').split('_')[0]?.trim().toUpperCase() || '—';
}

function resolveLegInstrument(strategy: SignalStrategy, role: 'maker' | 'hedge'): string {
  const snapshot =
    role === 'maker'
      ? strategy.equities_arb?.quote_snapshot?.maker_leg
      : strategy.equities_arb?.quote_snapshot?.hedge_leg;
  const snapshotInstrument = String(snapshot?.instrument_id ?? '').trim();
  if (snapshotInstrument) return snapshotInstrument;

  const roleKey =
    role === 'maker'
      ? strategy.maker_role_map?.maker_leg ?? 'maker'
      : strategy.maker_role_map?.hedge_leg ?? 'hedge';
  const legInstrument = String(strategy.legs?.[roleKey ?? '']?.instrument_id ?? '').trim();
  return legInstrument || '—';
}

function formatBps(value: unknown): string {
  const numeric = Number(value);
  return Number.isFinite(numeric) ? `${numeric.toFixed(1)} bps` : '—';
}

function formatMode(strategy: SignalStrategy): string {
  const mode = String(strategy.equities_arb?.operator?.execution_mode ?? '').trim();
  if (mode === 'take_take') return 'Take/Take';
  if (mode === 'maker_hedge') return 'Maker/Hedge';
  return mode || '—';
}

function formatFeeSummary(strategy: SignalStrategy): string {
  const fees = strategy.equities_arb?.operator?.fee_assumptions;
  const assumed = Number(fees?.assumed_hedge_fee_bps);
  const route = String(strategy.equities_arb?.operator?.hedge_policy?.route ?? '').trim();
  const feeText = Number.isFinite(assumed) ? `${assumed.toFixed(2)} bps` : '—';
  return route ? `${feeText} · ${route}` : feeText;
}

export default function EquitiesArbSignalTable({
  rows,
  strategies,
  loading = false,
}: {
  rows?: SignalStrategy[];
  strategies?: SignalStrategy[];
  loading?: boolean;
}) {
  const sourceRows = rows ?? strategies ?? [];

  const displayRows = useMemo<DisplayRow[]>(() => {
    return [...sourceRows]
      .map((strategy) => {
        const variant = resolveVariant(strategy);
        return {
          strategy,
          asset: resolveAsset(strategy),
          variant,
          makerMarket: resolveLegInstrument(strategy, 'maker'),
          hedgeMarket: resolveLegInstrument(strategy, 'hedge'),
          mode: formatMode(strategy),
          midSpread: formatBps(strategy.equities_arb?.quote_snapshot?.mid_spread_bps),
          arbSpread: formatBps(
            strategy.equities_arb?.quote_snapshot?.effective_spread_bps
              ?? strategy.equities_arb?.quote_snapshot?.arb_bid_spread_bps
              ?? strategy.equities_arb?.quote_snapshot?.arb_ask_spread_bps,
          ),
          feeSummary: formatFeeSummary(strategy),
        };
      })
      .sort((left, right) => {
        const assetCompare = left.asset.localeCompare(right.asset, undefined, { sensitivity: 'base' });
        if (assetCompare !== 0) return assetCompare;
        const variantCompare = VARIANT_ORDER[left.variant] - VARIANT_ORDER[right.variant];
        if (variantCompare !== 0) return variantCompare;
        return left.strategy.id.localeCompare(right.strategy.id, undefined, { sensitivity: 'base' });
      });
  }, [sourceRows]);

  return (
    <div data-testid="equities-arb-signal-table" className="h-full overflow-auto">
      <table className="min-w-[1200px] w-full text-left text-xs text-neutral-200">
        <thead className="sticky top-0 bg-bg-elevated z-10">
          <tr className="border-b border-border-subtle text-neutral-400">
            <th className="px-3 py-2 font-semibold">Strategy</th>
            <th className="px-3 py-2 font-semibold">Variant</th>
            <th className="px-3 py-2 font-semibold">Trading</th>
            <th className="px-3 py-2 font-semibold">Mode</th>
            <th className="px-3 py-2 font-semibold">Maker Market</th>
            <th className="px-3 py-2 font-semibold">Hedge Market</th>
            <th className="px-3 py-2 font-semibold">Mid Spread</th>
            <th className="px-3 py-2 font-semibold">Arb Spread</th>
            <th className="px-3 py-2 font-semibold">Fees / Route</th>
          </tr>
        </thead>
        <tbody>
          {displayRows.length === 0 ? (
            <tr>
              <td className="px-3 py-4 text-neutral-500" colSpan={9}>
                {loading ? 'Loading equities signals…' : 'No equities signals'}
              </td>
            </tr>
          ) : (
            displayRows.map((row) => (
              <tr key={row.strategy.id} className="border-b border-border-subtle/60">
                <td className="px-3 py-2 font-mono">{row.strategy.id}</td>
                <td className="px-3 py-2">{row.variant}</td>
                <td className="px-3 py-2">{row.strategy.tradeable ? 'Enabled' : row.strategy.blocked ? 'Pending' : 'Paused'}</td>
                <td className="px-3 py-2">{row.mode}</td>
                <td className="px-3 py-2 font-mono">{row.makerMarket}</td>
                <td className="px-3 py-2 font-mono">{row.hedgeMarket}</td>
                <td className="px-3 py-2 font-mono">{row.midSpread}</td>
                <td className="px-3 py-2 font-mono">{row.arbSpread}</td>
                <td className="px-3 py-2 font-mono">{row.feeSummary}</td>
              </tr>
            ))
          )}
        </tbody>
      </table>
    </div>
  );
}
