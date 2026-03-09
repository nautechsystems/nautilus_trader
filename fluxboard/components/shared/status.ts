import type { FxPair } from '@/types';
import { deriveFxStatus } from '@/utils';
import { severity, colors } from '@/lib/tokens';

export type StatusKind = 'ok' | 'warning' | 'critical' | 'info' | 'muted';

export type StatusDescriptor = {
  status: StatusKind;
  label: string;
};

export type AgeStatusOptions = {
  warningMs?: number;
  criticalMs?: number;
};

export type MarkStatusOptions = {
  warningDeviation?: number;
  criticalDeviation?: number;
};

export const STATUS_THEME: Record<
  StatusKind,
  { color: string; text: string; bg: string; border: string; glow: string }
> = {
  ok: {
    color: severity.success.color,
    text: severity.success.text,
    bg: severity.success.bg,
    border: severity.success.border,
    glow: 'rgba(15, 143, 107, 0.25)',
  },
  warning: {
    color: severity.warning.color,
    text: severity.warning.text,
    bg: severity.warning.bg,
    border: severity.warning.border,
    glow: 'rgba(201, 154, 46, 0.25)',
  },
  critical: {
    color: severity.critical.color,
    text: severity.critical.text,
    bg: severity.critical.bg,
    border: severity.critical.border,
    glow: 'rgba(224, 75, 73, 0.25)',
  },
  info: {
    color: severity.info.color,
    text: severity.info.text,
    bg: severity.info.bg,
    border: severity.info.border,
    glow: 'rgba(76, 122, 214, 0.22)',
  },
  muted: {
    color: colors.text.muted,
    text: colors.text.secondary,
    bg: colors.bg.hover,
    border: colors.border.DEFAULT,
    glow: 'rgba(124, 129, 140, 0.18)',
  },
};

const DEFAULT_AGE_WARNING_MS = 15 * 60 * 1000;
const DEFAULT_AGE_CRITICAL_MS = 30 * 60 * 1000;

export const statusFromFxPair = (pair: FxPair): StatusDescriptor => {
  const fxStatus = deriveFxStatus(pair);
  if (fxStatus === 'green') {
    return { status: 'ok', label: pair.stale ? 'STALE' : 'LIVE' };
  }
  if (fxStatus === 'yellow') {
    return { status: 'warning', label: pair.fallback ? 'FALLBACK' : 'WARN' };
  }
  if (fxStatus === 'red') {
    return { status: 'critical', label: pair.stale ? 'STALE' : 'BREACH' };
  }
  return { status: 'muted', label: 'UNKNOWN' };
};

export const statusFromAge = (
  timestampMs: number | null | undefined,
  options: AgeStatusOptions = {}
): StatusDescriptor => {
  if (!timestampMs || timestampMs <= 0) {
    return { status: 'muted', label: 'NO DATA' };
  }

  const warningMs = options.warningMs ?? DEFAULT_AGE_WARNING_MS;
  const criticalMs = options.criticalMs ?? DEFAULT_AGE_CRITICAL_MS;
  const ageMs = Date.now() - timestampMs;

  if (ageMs >= criticalMs) {
    return { status: 'critical', label: 'STALE' };
  }

  if (ageMs >= warningMs) {
    return { status: 'warning', label: 'DEGRADED' };
  }

  return { status: 'ok', label: 'LIVE' };
};

export const statusFromMark = (
  mark: number | null | undefined,
  stable: boolean,
  options: MarkStatusOptions = {}
): StatusDescriptor => {
  if (!stable || mark === null || mark === undefined || Number.isNaN(mark)) {
    return { status: 'muted', label: 'N/A' };
  }
  const warningDeviation = options.warningDeviation ?? 0.02; // 2%
  const criticalDeviation = options.criticalDeviation ?? 0.05; // 5%
  const deviation = Math.abs(1 - mark);

  if (deviation >= criticalDeviation) {
    return { status: 'critical', label: 'OFF' };
  }

  if (deviation >= warningDeviation) {
    return { status: 'warning', label: 'SKEW' };
  }

  return { status: 'ok', label: 'ON' };
};
