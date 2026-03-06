import { useMemo } from 'react';
import { renderToString } from 'katex';
import 'katex/dist/katex.min.css';

import { Select } from '@/components/ui';
import type { FvTermBreakdown } from '@/types';
import {
  FV_INFO_TEXT_CLASS,
  FV_SECTION_CARD_CLASS,
  FV_SECTION_TITLE_CLASS,
  FV_SUBSECTION_TITLE_CLASS,
  FV_SUBSECTION_CLASS,
} from './styles';

const fmt = (value: number | null | undefined, digits = 8): string =>
  typeof value === 'number' && Number.isFinite(value) ? value.toFixed(digits) : '—';

const fmtTs = (value: number | null | undefined): string =>
  typeof value === 'number' && Number.isFinite(value) ? new Date(value).toISOString() : '—';

const fmtBool = (value: boolean | null | undefined): string =>
  typeof value === 'boolean' ? (value ? 'true' : 'false') : '—';

const cfgText = (
  config: Record<string, unknown> | undefined,
  key: string
): string | undefined => {
  if (!config) return undefined;
  const value = config[key];
  return typeof value === 'string' && value.trim() ? value.trim() : undefined;
};

const normalizeTermTrigger = (value: string | null | undefined): string => {
  if (!value) return '—';
  const normalized = value.trim().toLowerCase();
  switch (normalized) {
    case 'mid':
    case 'onchmid':
      return 'onChMid';
    case 'trade':
    case 'ontrade':
      return 'onTrade';
    case 'timer':
    case 'ontimer':
      return 'onTimer';
    default:
      return value;
  }
};

const sensitivityLabel = (term: FvTermBreakdown): string => {
  if (term.mode === 'power') {
    const value = fmt(term.beta, 4);
    return value === '—' ? '—' : `${value} (power)`;
  }
  if (term.mode === 'linear') {
    const value = fmt(term.gain, 4);
    return value === '—' ? '—' : `${value} (linear)`;
  }
  const beta = fmt(term.beta, 4);
  if (beta !== '—') return `${beta} (${term.mode || 'power'})`;
  const gain = fmt(term.gain, 4);
  if (gain !== '—') return `${gain} (${term.mode || 'linear'})`;
  return '—';
};

const sourceDisplay = (
  source: string | null | undefined,
  config?: Record<string, unknown>
): string => {
  if (!source) return '—';
  const normalized = source.trim().toLowerCase();
  const p2fChannel = cfgText(config, 'p2f_channel');
  const perpChannel = cfgText(config, 'perp_mid_channel');
  const tradeChannel = cfgText(config, 'trade_channel');
  const perpLastKey = cfgText(config, 'perp_mid_last_key');

  switch (normalized) {
    case 'spot_p2f_mid_100k':
    case 'p2f_mid_100k':
      return `Spot P2F mid (100k) (${p2fChannel || source})`;
    case 'p2f_mid_50k':
      return `Spot P2F mid (50k) (${p2fChannel || source})`;
    case 'perp_mid_channel':
      return `Perp mid (${perpChannel || source})`;
    case 'p2f_payload_perp_mid':
      return `Perp mid from P2F payload (${p2fChannel || source})`;
    case 'p2f_payload_mid_fallback':
      return `Perp mid fallback from P2F payload (${p2fChannel || source})`;
    case 'trade_price_fallback':
      return `Trade price fallback (${tradeChannel || source})`;
    case 'perp_mid_last_key':
      return `Perp mid last-key fallback (${perpLastKey || source})`;
    default:
      return source;
  }
};

const inferTheo2Fallback = (term: FvTermBreakdown): boolean => {
  if (typeof term.theo2_is_fallback === 'boolean') {
    return term.theo2_is_fallback;
  }
  const source = (term.theo2_source || '').toLowerCase();
  return source === 'perp_mid_last_key' || source.includes('fallback');
};

const triggerWithTimestamp = (
  trigger: string | null | undefined,
  tsMs: number | null | undefined
): string => {
  const normalized = normalizeTermTrigger(trigger);
  if (normalized === '—') return '—';
  const ts = fmtTs(tsMs);
  if (ts === '—') return normalized;
  return `${normalized} @ ${ts}`;
};

export function FVFormulaInspector({
  terms,
  selectedTermId,
  onSelectTerm,
  snapshotTrigger,
  snapshotTsMs,
  config,
}: {
  terms: FvTermBreakdown[];
  selectedTermId?: number;
  onSelectTerm: (termId: number) => void;
  snapshotTrigger?: string;
  snapshotTsMs?: number;
  config?: Record<string, unknown>;
}) {
  const selected = terms.find((term) => term.id === selectedTermId) || terms[0];

  const latexHtml = useMemo(() => {
    const source = selected?.formula_latex;
    if (!source) return null;
    const candidates = [source, source.replace(/\\\\/g, '\\')].filter(
      (value, idx, all) => value.length > 0 && all.indexOf(value) === idx
    );

    for (const latex of candidates) {
      try {
        return renderToString(latex, {
          displayMode: true,
          throwOnError: true,
          strict: 'ignore',
        });
      } catch {
        // Try the next candidate.
      }
    }
    return null;
  }, [selected?.formula_latex]);

  const detailRows: Array<{ label: string; value: string }> = useMemo(() => {
    if (!selected) return [];
    const rows: Array<{ label: string; value: string }> = [
      { label: 'Trigger Gate', value: selected.trigger || '—' },
      {
        label: 'Latest Global Event',
        value: snapshotTrigger && snapshotTsMs
          ? `${snapshotTrigger} @ ${new Date(snapshotTsMs).toISOString()}`
          : '—',
      },
      { label: 'Mode', value: selected.mode || '—' },
      { label: 'Sensitivity (Amplitude/β)', value: sensitivityLabel(selected) },
      {
        label: 'Term Last Trigger',
        value: triggerWithTimestamp(
          selected.term_last_trigger || selected.last_trigger_event,
          selected.term_last_trigger_ts_ms ?? selected.last_trigger_ts_ms
        ),
      },
      { label: 'Theo1 Sample', value: fmt(selected.theo1_sample) },
      { label: 'Theo1 Source', value: sourceDisplay(selected.theo1_source, config) },
      { label: 'Theo1 Sample TS', value: fmtTs(selected.theo1_ts_ms) },
      { label: 'Theo2 Sample', value: fmt(selected.theo2_sample) },
      { label: 'Theo2 Source', value: sourceDisplay(selected.theo2_source, config) },
      { label: 'Theo2 Fallback', value: fmtBool(inferTheo2Fallback(selected)) },
      { label: 'Theo2 Sample TS', value: fmtTs(selected.theo2_ts_ms) },
      { label: 'EMA1', value: fmt(selected.ema1) },
      { label: 'EMA2', value: fmt(selected.ema2) },
      { label: 'Ratio (Theo2 / EMA2)', value: fmt(selected.ratio) },
      { label: 'Multiplier', value: fmt(selected.multiplier) },
      { label: 'Baseline', value: fmt(selected.baseline) },
      { label: 'Term Value', value: fmt(selected.value) },
      { label: 'Contribution', value: fmt(selected.contribution) },
      { label: 'Contribution Delta', value: fmt(selected.contribution_delta) },
      {
        label: 'Sample-And-Hold Theo2',
        value: selected.theo2_sample !== null && selected.theo2_sample !== undefined ? 'yes' : 'no',
      },
    ];
    if (selected.mode === 'raw') {
      rows.push({ label: 'P2F Bid', value: fmt(selected.p2f_bid) });
      rows.push({ label: 'P2F Ask', value: fmt(selected.p2f_ask) });
      rows.push({ label: 'P2F Mid', value: fmt(selected.p2f_mid) });
      rows.push({ label: 'P2F Depth Sufficient', value: fmtBool(selected.p2f_depth_ok) });
      rows.push({ label: 'P2F Levels Used', value: fmt(selected.p2f_levels_used, 0) });
      rows.push({ label: 'P2F TS', value: fmtTs(selected.p2f_ts_ms) });
    }
    return rows;
  }, [config, selected, snapshotTrigger, snapshotTsMs]);

  if (!selected) {
    return (
      <div className={`${FV_SECTION_CARD_CLASS} ${FV_INFO_TEXT_CLASS}`}>
        No term formulas available.
      </div>
    );
  }

  return (
    <div className={FV_SECTION_CARD_CLASS}>
      <div className="mb-3 flex items-center justify-between gap-3">
        <div className={FV_SECTION_TITLE_CLASS}>Formula Inspector</div>
        <Select
          size="xs"
          value={String(selected.id)}
          options={terms.map((term) => ({
            label: `${term.id}. ${term.name}`,
            value: String(term.id),
          }))}
          onChange={(value) => onSelectTerm(Number(value))}
        />
      </div>
      <div className="space-y-4">
        <div>
          <div className={FV_SUBSECTION_TITLE_CLASS}>Text</div>
          <div className="mt-1 text-sm text-text-primary">{selected.formula_text || '—'}</div>
        </div>
        <div>
          <div className={FV_SUBSECTION_TITLE_CLASS}>LaTeX</div>
          {latexHtml ? (
            <div
              className={`mt-1 overflow-x-auto px-3 py-4 text-sm text-text-primary ${FV_SUBSECTION_CLASS}`}
              dangerouslySetInnerHTML={{ __html: latexHtml }}
            />
          ) : (
            <div className={`mt-1 px-3 py-4 text-xs text-text-secondary ${FV_SUBSECTION_CLASS}`}>
              {selected.formula_latex || '—'}
            </div>
          )}
          {selected.formula_latex && (
            <div className="mt-1 break-all font-mono text-[11px] text-text-muted">
              source: {selected.formula_latex}
            </div>
          )}
        </div>
        <div>
          <div className={FV_SUBSECTION_TITLE_CLASS}>Reconciliation</div>
          <div className={`mt-1 grid grid-cols-1 gap-x-4 gap-y-1 p-3 md:grid-cols-2 ${FV_SUBSECTION_CLASS}`}>
            {detailRows.map((row) => (
              <div key={row.label} className="flex items-center justify-between gap-2 text-[11px]">
                <span className="text-text-muted">{row.label}</span>
                <span className="max-w-[28rem] text-right font-mono tabular-nums text-text-primary">{row.value}</span>
              </div>
            ))}
          </div>
        </div>
      </div>
    </div>
  );
}
