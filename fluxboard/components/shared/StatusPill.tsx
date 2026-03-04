import { useEffect, useMemo, useState, type CSSProperties, type ReactNode } from 'react';
import { SimpleTooltip } from '@/components/ui/tooltip';
import Badge from '@/components/ui/badge/Badge';
import { colors } from '@/lib/tokens';
import { cn } from '@/lib/utils';
import { featureFlags } from '@/config/featureFlags';
import { STATUS_THEME, type StatusKind } from './status';

export type StatusPillVariant = 'live' | 'pending' | 'inactive';
export type StatusPillTone = 'subtle' | 'solid';
export type StatusPillSize = 'xs' | 'sm' | 'md';

type VariantMeta = {
  icon: ReactNode;
  status: StatusKind;
  label: string;
};

const VARIANT_META: Record<StatusPillVariant, VariantMeta> = {
  live: {
    icon: '✅',
    status: 'ok',
    label: 'Live',
  },
  pending: {
    icon: '🕓',
    status: 'warning',
    label: 'Pending',
  },
  inactive: {
    icon: '⏸️',
    status: 'muted',
    label: 'Inactive',
  },
};

type StatusPillStyle = CSSProperties & { '--status-glow'?: string };

function usePrefersReducedMotion(): boolean {
  const [reduced, setReduced] = useState(false);

  useEffect(() => {
    try {
      const media = window.matchMedia?.('(prefers-reduced-motion: reduce)');
      if (!media) return;
      setReduced(media.matches);
      const handler = (event: MediaQueryListEvent) => setReduced(event.matches);
      media.addEventListener?.('change', handler);
      return () => media.removeEventListener?.('change', handler);
    } catch {
      return undefined;
    }
  }, []);

  return reduced;
}

function usePageVisibility(): boolean {
  const [visible, setVisible] = useState(() => {
    if (typeof document === 'undefined' || typeof document.visibilityState === 'undefined') {
      return true;
    }
    return document.visibilityState === 'visible';
  });

  useEffect(() => {
    if (typeof document === 'undefined') return;
    const handler = () => {
      setVisible(document.visibilityState === 'visible');
    };
    document.addEventListener('visibilitychange', handler);
    return () => document.removeEventListener('visibilitychange', handler);
  }, []);

  return visible;
}

export type StatusPillLayout = 'stacked' | 'inline';

export interface StatusPillProps {
  status?: StatusKind;
  variant?: StatusPillVariant;
  label?: string;
  subLabel?: string;
  tooltip?: string;
  icon?: ReactNode;
  className?: string;
  ariaLabel?: string;
  animate?: boolean;
  layout?: StatusPillLayout;
  size?: StatusPillSize;
  tone?: StatusPillTone;
}

const STATUS_ICONS: Record<StatusKind, ReactNode> = {
  ok: '✅',
  warning: '🟡',
  critical: '⛔',
  info: 'ℹ️',
  muted: '⏸️',
};

const SIZE_CLASS: Record<StatusPillSize, string> = {
  xs: 'text-[11px] px-1.5 py-0.5',
  sm: 'text-[12px] px-2 py-1',
  md: 'text-[13px] px-2.5 py-1.5',
};

export function StatusPill({
  status,
  variant,
  label,
  subLabel,
  tooltip,
  icon,
  className,
  ariaLabel,
  animate = true,
  layout = 'stacked',
  size = 'xs',
  tone = 'subtle',
}: StatusPillProps) {
  const legacyMeta = variant ? VARIANT_META[variant] : undefined;
  const effectiveStatus: StatusKind = status ?? legacyMeta?.status ?? 'muted';
  const effectiveLabel = label ?? legacyMeta?.label ?? effectiveStatus.toUpperCase();
  const theme = STATUS_THEME[effectiveStatus] ?? {
    color: colors.text.secondary,
    text: colors.text.secondary,
    bg: colors.bg.hover,
    border: colors.border.DEFAULT,
    glow: 'rgba(124, 129, 140, 0.18)',
  };
  const prefersReducedMotion = usePrefersReducedMotion();
  const pageVisible = usePageVisibility();
  const computedAriaLabel = ariaLabel ?? tooltip ?? effectiveLabel;

  // Animation disabled for compact visual density
  const shouldAnimate = false;

  const pillStyle = useMemo<StatusPillStyle>(() => ({
    backgroundColor: tone === 'solid' ? theme.color : theme.bg,
    borderColor: tone === 'solid' ? theme.color : theme.border,
    color: theme.text,
    '--status-glow': theme.glow,
  }), [theme.bg, theme.border, theme.color, theme.text, theme.glow, tone]);

  const textContainerClass =
    layout === 'inline'
      ? 'flex flex-row items-center gap-1 text-[11px] font-semibold'
      : 'flex flex-col leading-tight';

  const subLabelClass =
    layout === 'inline'
      ? 'text-[11px] font-medium text-neutral-200 flex items-center gap-1'
      : 'text-[10px] uppercase tracking-wide text-neutral-400';

  const content = featureFlags.tradingStatusPills ? (
    <div
      role="status"
      aria-live="polite"
      aria-label={computedAriaLabel}
      className={cn(
        'status-pill inline-flex items-center gap-1.5 rounded-full font-semibold tracking-tight',
        SIZE_CLASS[size],
        shouldAnimate && 'status-pill--animate',
        className
      )}
      style={pillStyle}
      data-variant={variant}
      data-status={effectiveStatus}
    >
      <span aria-hidden="true" className="text-[12px] leading-none">
        {icon ?? legacyMeta?.icon ?? STATUS_ICONS[effectiveStatus]}
      </span>
      <div className={cn(textContainerClass)}>
        <span className="text-[11.5px] font-bold leading-none">{effectiveLabel}</span>
        {subLabel && (
          <span className={cn(subLabelClass)}>
            {layout === 'inline' ? (
              <>
                <span aria-hidden="true">•</span>
                <span>{subLabel}</span>
              </>
            ) : (
              subLabel
            )}
          </span>
        )}
      </div>
    </div>
  ) : (
    <Badge
      variant={effectiveStatus === 'ok'
        ? 'success'
        : effectiveStatus === 'warning'
          ? 'warning'
          : effectiveStatus === 'critical'
            ? 'danger'
            : effectiveStatus === 'info'
              ? 'info'
              : 'neutral'}
      size="xs"
      aria-label={computedAriaLabel}
      className={className}
      data-status={effectiveStatus}
    >
      {effectiveLabel}
    </Badge>
  );

  if (!tooltip) {
    return content;
  }

  return (
    <SimpleTooltip content={tooltip} delay={150}>
      {content}
    </SimpleTooltip>
  );
}
