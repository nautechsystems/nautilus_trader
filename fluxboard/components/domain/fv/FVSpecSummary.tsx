import { Info } from 'lucide-react';

import { SimpleTooltip } from '@/components/ui/tooltip';
import type { FvSnapshot } from '@/types';
import {
  FV_ERROR_BANNER_CLASS,
  FV_HELP_ICON_CLASS,
  FV_SECTION_CARD_CLASS,
  FV_SECTION_TITLE_CLASS,
} from './styles';

const asNumber = (value: unknown): number | undefined => {
  if (typeof value === 'number' && Number.isFinite(value)) return value;
  if (typeof value === 'string') {
    const parsed = Number(value);
    if (Number.isFinite(parsed)) return parsed;
  }
  return undefined;
};

const asText = (value: unknown): string => {
  if (value === null || value === undefined) return '—';
  if (typeof value === 'string') return value || '—';
  if (typeof value === 'number' || typeof value === 'boolean') return String(value);
  return '—';
};

function SpecRow({
  label,
  value,
  tooltip,
}: {
  label: string;
  value: string;
  tooltip: string;
}) {
  return (
    <div className="flex items-center justify-between gap-3 border-b border-border/60 px-2 py-1.5 text-sm last:border-b-0">
      <span className="inline-flex items-center gap-1 text-text-muted">
        <span>{label}</span>
        <SimpleTooltip content={tooltip} delay={200}>
          <button
            type="button"
            className={FV_HELP_ICON_CLASS}
            aria-label={`Help: ${label}`}
          >
            <Info className="h-3 w-3" />
          </button>
        </SimpleTooltip>
      </span>
      <span className="font-mono tabular-nums text-text-primary">{value}</span>
    </div>
  );
}

const fmtNumber = (value: unknown, digits = 6): string => {
  const parsed = asNumber(value);
  if (parsed === undefined) return '—';
  return parsed.toFixed(digits);
};

const fmtIsoTime = (value: unknown): string => {
  const parsed = asNumber(value);
  if (parsed === undefined) return '—';
  return new Date(parsed).toISOString();
};

const sensitivityFromConfig = (
  config: Record<string, unknown> | undefined,
  modeText: string
): string => {
  const normalizedMode = modeText === '—' ? 'power' : modeText.toLowerCase();
  const sensitivityKey = normalizedMode === 'linear' ? 'gain' : 'beta';
  const termsRaw = config?.terms;
  const values: number[] = [];

  if (Array.isArray(termsRaw)) {
    for (const rawTerm of termsRaw) {
      if (!rawTerm || typeof rawTerm !== 'object') continue;
      const term = rawTerm as Record<string, unknown>;
      const termKind = asText(term.kind).toLowerCase();
      const termMode = asText(term.mode).toLowerCase();
      const termValue =
        sensitivityKey === 'gain' ? asNumber(term.gain) : asNumber(term.beta);
      const isCompTheo = termKind === 'comp_theo' || termMode === 'power' || termMode === 'linear';
      if (isCompTheo && termValue !== undefined) {
        values.push(termValue);
      }
    }
  }

  if (!values.length) {
    const fallbackSensitivity =
      sensitivityKey === 'gain'
        ? asNumber(config?.comp_theo_gain)
        : asNumber(config?.comp_theo_beta);
    if (fallbackSensitivity !== undefined) {
      values.push(fallbackSensitivity);
    }
  }

  if (!values.length) return '—';
  const uniqueValues = Array.from(new Set(values.map((value) => value.toFixed(4))));
  const valueText = uniqueValues.length === 1 ? uniqueValues[0] : uniqueValues.join('/');
  return `${valueText} (${normalizedMode})`;
};

export function FVSpecSummary({
  snapshot,
  config,
  loading = false,
  error,
}: {
  snapshot?: FvSnapshot;
  config?: Record<string, unknown>;
  loading?: boolean;
  error?: string;
}) {
  if (!snapshot && !config && !loading && !error) {
    return null;
  }

  const weights = (snapshot?.terms || [])
    .map((term) => asNumber(term.weight))
    .filter((value): value is number => value !== undefined);
  const weightSum = weights.reduce((sum, value) => sum + value, 0);
  const overlayCapPct = asNumber(config?.overlay_max_pct);
  const profile = asText(snapshot?.fv_profile || config?.fv_profile);
  const calcType = asText(snapshot?.calc_type || config?.calc_type);
  const version = asText(snapshot?.fv_version || config?.fv_version);
  const compTheoMode = asText(config?.comp_theo_mode);
  const sensitivityText = sensitivityFromConfig(config, compTheoMode);
  const tickMs = asText(config?.tick_interval_ms);
  const signedVolumeHlMs = asText(config?.signed_volume_half_life_ms);
  const p2fChannel = asText(config?.p2f_channel);
  const perpChannel = asText(config?.perp_mid_channel);
  const tradeChannel = asText(config?.trade_channel);
  const overlayCapText = overlayCapPct === undefined ? '—' : `${(overlayCapPct * 100).toFixed(2)}%`;
  const weightsText = weights.length ? weights.map((value) => value.toFixed(0)).join('/') : '—';
  const svState = snapshot?.signed_volume_state;
  const svClampText = fmtNumber(svState?.clamp_notional_usd, 0);
  const svImpulseText = fmtNumber(svState?.last_impulse, 6);
  const svPreText = fmtNumber(svState?.last_pre_decay, 6);
  const svPostText = fmtNumber(svState?.last_post_decay, 6);
  const svDtText = fmtNumber(svState?.last_dt_ms, 0);
  const svNotionalText = fmtNumber(svState?.last_notional_usd, 2);
  const svLastSide = asText(svState?.last_side);
  const svUpdateTime = fmtIsoTime(svState?.last_update_ts_ms);

  return (
    <div className={FV_SECTION_CARD_CLASS}>
      <div className={`mb-3 ${FV_SECTION_TITLE_CLASS}`}>
        <span>Spec Snapshot</span>
        <SimpleTooltip
          content="Effective runtime/profile settings for operator reconciliation (not a full proof of term internals)."
          delay={200}
        >
          <button
            type="button"
            className={FV_HELP_ICON_CLASS}
            aria-label="Help: Spec Snapshot"
          >
            <Info className="h-3 w-3" />
          </button>
        </SimpleTooltip>
      </div>
      {loading && (
        <div className="mb-3 text-xs text-text-muted">Loading runtime config…</div>
      )}
      {error && (
        <div className={`mb-3 ${FV_ERROR_BANNER_CLASS}`}>
          config: {error}
        </div>
      )}
      <div className="overflow-hidden rounded-md border border-border bg-bg-base/70">
        <SpecRow label="Profile" value={profile} tooltip="Active FV profile namespace (e.g., fv1)." />
        <SpecRow label="Version" value={version} tooltip="FV version for routing/compatibility." />
        <SpecRow label="Calc Type" value={calcType} tooltip="Calculation type label emitted by FV server." />
        <SpecRow label="Weights" value={weightsText} tooltip="Term weights in display order." />
        <SpecRow
          label="Weight Sum"
          value={weights.length ? `${weightSum.toFixed(2)}%` : '—'}
          tooltip="Should normally be 100% for base blend terms."
        />
        <SpecRow label="CompTheo Mode" value={compTheoMode} tooltip="CompTheo transform mode (power or linear)." />
        <SpecRow
          label="Sensitivity (Amplitude/β)"
          value={sensitivityText}
          tooltip="CompTheo sensitivity knob: beta in power mode and gain in linear mode."
        />
        <SpecRow label="Tick (ms)" value={tickMs} tooltip="Timer tick interval used by FV runtime." />
        <SpecRow
          label="SV Half-Life (ms)"
          value={signedVolumeHlMs}
          tooltip="Signed-volume decay half-life in milliseconds."
        />
        <SpecRow label="Overlay Cap" value={overlayCapText} tooltip="Absolute max overlay percentage cap." />
        <SpecRow label="P2F Channel" value={p2fChannel} tooltip="Spot/P2F source channel." />
        <SpecRow label="Perp Channel" value={perpChannel} tooltip="Perp-theo source channel." />
        <SpecRow label="Trade Channel" value={tradeChannel} tooltip="Trade event channel for trigger-gated terms." />
        <SpecRow
          label="Theo Samples"
          value={snapshot?.terms?.some((term) => term.theo1_sample != null || term.theo2_sample != null) ? 'present' : '—'}
          tooltip="Whether snapshots expose term-local Theo1/Theo2 sample values for reconciliation."
        />
        <SpecRow
          label="SV Clamp USD"
          value={svClampText}
          tooltip="Per-trade notional clamp used before signed-volume impulse is applied."
        />
        <SpecRow
          label="SV Last Side"
          value={svLastSide}
          tooltip="Last trade side used for signed-volume impulse (buy/sell)."
        />
        <SpecRow
          label="SV Last Notional"
          value={svNotionalText}
          tooltip="Last trade notional in USD used to compute signed-volume impulse."
        />
        <SpecRow
          label="SV Last Impulse"
          value={svImpulseText}
          tooltip="Signed impulse added after decay step on the latest signed-volume update."
        />
        <SpecRow
          label="SV Decay Step"
          value={`${svPreText} -> ${svPostText}`}
          tooltip="Signed-volume value before and after decay, prior to applying trade impulse."
        />
        <SpecRow
          label="SV Last dt (ms)"
          value={svDtText}
          tooltip="Elapsed milliseconds used for the latest signed-volume decay step."
        />
        <SpecRow
          label="SV Last Update"
          value={svUpdateTime}
          tooltip="Timestamp of the most recent signed-volume update."
        />
      </div>
    </div>
  );
}
