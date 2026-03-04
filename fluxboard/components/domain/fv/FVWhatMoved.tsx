import { Info } from 'lucide-react';

import { SimpleTooltip } from '@/components/ui/tooltip';
import type { FvSnapshot, FvWhatMoved as FvWhatMovedType } from '@/types';
import {
  FV_HELP_ICON_CLASS,
  FV_INFO_TEXT_CLASS,
  FV_SECTION_CARD_CLASS,
  FV_SECTION_TITLE_CLASS,
} from './styles';

const fmt = (value: number | null | undefined, digits = 6): string => {
  if (typeof value !== 'number' || !Number.isFinite(value)) return '—';
  return value.toFixed(digits);
};

const fmtSigned = (value: number | null | undefined, digits = 6): string => {
  if (typeof value !== 'number' || !Number.isFinite(value)) return '—';
  const sign = value >= 0 ? '+' : '-';
  return `${sign}${Math.abs(value).toFixed(digits)}`;
};

const sameSnapshotStream = (a?: FvSnapshot, b?: FvSnapshot): boolean => {
  if (!a || !b) return false;
  return a.symbol === b.symbol && (a.fv_profile || 'fv1') === (b.fv_profile || 'fv1');
};

export function FVWhatMoved({
  whatMoved,
  currentSnapshot,
  previousSnapshot,
}: {
  whatMoved?: FvWhatMovedType;
  currentSnapshot?: FvSnapshot;
  previousSnapshot?: FvSnapshot;
}) {
  const kind = whatMoved?.kind || 'term';
  const hasDeltaContext = sameSnapshotStream(previousSnapshot, currentSnapshot);
  const baseDelta = hasDeltaContext
    ? (currentSnapshot?.base || 0) - (previousSnapshot?.base || 0)
    : undefined;
  const finalDelta = hasDeltaContext
    ? (currentSnapshot?.final || 0) - (previousSnapshot?.final || 0)
    : undefined;
  const overlayDeltaPct = hasDeltaContext
    ? ((currentSnapshot?.overlay_pct || 0) - (previousSnapshot?.overlay_pct || 0)) * 100
    : undefined;
  const deltaContribution = whatMoved?.delta_contribution || 0;
  const deltaContributionClass = deltaContribution < 0 ? 'text-danger-light' : 'text-success-light';

  const renderHeader = (tooltipContent: string) => (
    <div className={`mb-3 ${FV_SECTION_TITLE_CLASS}`}>
      <span>What moved FV?</span>
      <SimpleTooltip content={tooltipContent} delay={200}>
        <button
          type="button"
          className={FV_HELP_ICON_CLASS}
          aria-label="Help: What moved FV?"
        >
          <Info className="h-3 w-3" />
        </button>
      </SimpleTooltip>
    </div>
  );

  const renderDeltaContext = () => {
    if (!hasDeltaContext) return null;

    return (
      <div className="mt-3 border-t border-border pt-3 text-xs text-text-secondary">
        <span className="font-mono tabular-nums">
          base {fmt(previousSnapshot!.base)} → {fmt(currentSnapshot!.base)} ({fmtSigned(baseDelta)})
        </span>
        {' · '}
        <span className="font-mono tabular-nums">
          final {fmt(previousSnapshot!.final)} → {fmt(currentSnapshot!.final)} ({fmtSigned(finalDelta)})
        </span>
        {' · '}
        <span className="font-mono tabular-nums">
          overlay Δ={fmtSigned(overlayDeltaPct, 4)}%
        </span>
      </div>
    );
  };

  if (!whatMoved) {
    return (
      <div className={`${FV_SECTION_CARD_CLASS} ${FV_INFO_TEXT_CLASS}`}>
        What moved FV? Waiting for contribution deltas.
      </div>
    );
  }

  if (kind === 'none') {
    return (
      <div className={FV_SECTION_CARD_CLASS}>
        {renderHeader('Shows the biggest mover on the latest FV update. If nothing clears the mover threshold, we show the overall deltas instead.')}
        <div className="text-sm text-text-primary">
          <span className="font-semibold">No dominant mover</span>
          {' · '}
          <span className="text-text-secondary">trigger={whatMoved.trigger || '—'}</span>
          {' · '}
          <span className="font-mono tabular-nums text-text-secondary">
            Δbase={fmtSigned(whatMoved.delta_base)}
          </span>
          {' · '}
          <span className="font-mono tabular-nums text-warning-light">
            Δoverlay={fmtSigned(whatMoved.delta_overlay_pct)}
          </span>
          {' · '}
          <span className="font-mono tabular-nums text-success-light">
            Δfinal={fmtSigned(whatMoved.delta_final)}
          </span>
        </div>
        {renderDeltaContext()}
      </div>
    );
  }

  if (kind === 'overlay') {
    return (
      <div className={FV_SECTION_CARD_CLASS}>
        {renderHeader('Shows the signed-volume overlay when it dominates the latest final FV delta.')}
        <div className="text-sm text-text-primary">
          <span className="font-semibold">{whatMoved.term_name || 'Signed Volume Overlay'}</span>
          {' · '}
          <span className="text-text-secondary">trigger={whatMoved.trigger || '—'}</span>
          {' · '}
          <span className="font-mono tabular-nums text-warning-light">
            Δoverlay={fmt(whatMoved.delta_overlay_pct)}
          </span>
          {' · '}
          <span className="font-mono tabular-nums text-success-light">
            Δfinal={fmt(whatMoved.delta_final)}
          </span>
        </div>
        {renderDeltaContext()}
      </div>
    );
  }

  if (!whatMoved.term_id) {
    return (
      <div className={`${FV_SECTION_CARD_CLASS} ${FV_INFO_TEXT_CLASS}`}>
        What moved FV? Waiting for contribution deltas.
      </div>
    );
  }

  return (
    <div className={FV_SECTION_CARD_CLASS}>
      {renderHeader('Shows the term that contributed the biggest delta on the latest update trigger.')}
      <div className="text-sm text-text-primary">
        <span className="font-semibold">{whatMoved.term_name || `Term ${whatMoved.term_id}`}</span>
        {' · '}
        <span className="text-text-secondary">trigger={whatMoved.trigger || '—'}</span>
        {' · '}
        <span className={`font-mono tabular-nums ${deltaContributionClass}`}>Δcontrib={fmt(whatMoved.delta_contribution)}</span>
      </div>
      {(whatMoved.side || whatMoved.notional_usd !== undefined) && (
        <div className="mt-2 text-xs text-text-secondary">
          side={whatMoved.side || '—'} · <span className="font-mono tabular-nums">notional={fmt(whatMoved.notional_usd, 2)}</span>
        </div>
      )}
      {renderDeltaContext()}
    </div>
  );
}
