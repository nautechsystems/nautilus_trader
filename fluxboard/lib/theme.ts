/**
 * Fluxboard Theme Configuration
 *
 * Provides theme utilities and Tailwind integration for the design token system.
 */

import { colors, typography, spacing, borderRadius, elevation, generateCSSVariables } from './tokens';

// =============================================================================
// THEME OBJECT
// =============================================================================

export const theme = {
  colors,
  typography,
  spacing,
  borderRadius,
  elevation,
} as const;

// =============================================================================
// TAILWIND PLUGIN
// =============================================================================

/**
 * Tailwind plugin that injects CSS variables and extends the theme
 */
export function fluxboardThemePlugin({ addBase, theme: tailwindTheme }: any) {
  // Inject CSS variables at :root level
  addBase({
    ':root': generateCSSVariables(),
  });

  // Inject global base styles
  addBase({
    'html, body, #root': {
      height: '100%',
      background: colors.bg.base,
      color: colors.text.primary,
    },
    '*': {
      'box-sizing': 'border-box',
    },
  });
}

// =============================================================================
// TAILWIND THEME EXTENSION
// =============================================================================

/**
 * Tailwind theme extension object for tailwind.config.ts
 */
export const tailwindExtend = {
  colors: {
    // Map semantic colors to Tailwind classes
    bg: colors.bg,
    accent: colors.accent,
    semantic: colors.semantic,

    // Override Tailwind's neutral palette with our custom one
    neutral: colors.neutral,
  },

  fontFamily: {
    sans: typography.fontFamily.sans.split(', '),
    mono: typography.fontFamily.mono.split(', '),
  },

  fontSize: {
    ...typography.fontSize,
  },

  spacing: {
    // Add custom spacing tokens
    'panel-x': spacing.panel.paddingHorizontal,
    'panel-y': spacing.panel.paddingVertical,
    'panel-gap': spacing.panel.gap,
    'row-compact': spacing.row.compact,
    'row-normal': spacing.row.normal,
    'row-header': spacing.row.header,
  },

  borderRadius: {
    ...borderRadius,
  },

  zIndex: {
    ...elevation,
  },

  keyframes: {
    // Keep existing flash animation for new trades
    flash: {
      '0%': { backgroundColor: 'rgb(6 78 59 / 0.3)' },
      '50%': { backgroundColor: 'rgb(6 78 59 / 0.5)' },
      '100%': { backgroundColor: 'transparent' },
    },
    // Add pulse animation for freshness indicator
    pulse: {
      '0%, 100%': { opacity: '1' },
      '50%': { opacity: '0.5' },
    },
  },

  animation: {
    flash: 'flash 500ms ease-out',
    pulse: 'pulse 2s cubic-bezier(0.4, 0, 0.6, 1) infinite',
  },
} as const;

// =============================================================================
// UTILITY FUNCTIONS
// =============================================================================

/**
 * Get color for a severity level
 */
export function getSeverityColor(severity: 'critical' | 'warning' | 'info' | 'success') {
  const severityMap = {
    critical: colors.semantic.danger.DEFAULT,
    warning: colors.semantic.warning.DEFAULT,
    info: colors.semantic.info.DEFAULT,
    success: colors.semantic.success.DEFAULT,
  };
  return severityMap[severity];
}

/**
 * Get color for trade side
 */
export function getTradeSideColor(side: 'BUY' | 'SELL' | 'buy' | 'sell') {
  const normalizedSide = side.toLowerCase();
  return normalizedSide === 'buy'
    ? colors.semantic.success.light
    : colors.semantic.danger.light;
}

/**
 * Get background color for trade side badge
 */
export function getTradeSideBadgeColors(side: 'BUY' | 'SELL' | 'buy' | 'sell') {
  const normalizedSide = side.toLowerCase();
  return normalizedSide === 'buy'
    ? {
        bg: colors.semantic.success.bg,
        text: colors.semantic.success.light,
        border: colors.semantic.success.dark,
      }
    : {
        bg: colors.semantic.danger.bg,
        text: colors.semantic.danger.light,
        border: colors.semantic.danger.dark,
      };
}

/**
 * Format edge value with color
 * Positive = teal, Negative = crimson, Zero = gray
 */
export function getEdgeColor(edge: number): string {
  if (edge > 0) return colors.semantic.success.light;
  if (edge < 0) return colors.semantic.danger.light;
  return colors.text.muted;
}

/**
 * Format PnL value with color (same as edge)
 */
export function getPnLColor(pnl: number): string {
  return getEdgeColor(pnl);
}

// =============================================================================
// DENSITY MODE UTILITIES
// =============================================================================

/**
 * Get padding classes based on dense mode
 */
export function getPaddingClasses(dense: boolean): string {
  return dense ? 'px-2 py-1' : 'px-3 py-2';
}

/**
 * Get text size classes based on dense mode
 */
export function getTextSizeClasses(dense: boolean): string {
  return dense ? 'text-xs' : 'text-sm';
}

/**
 * Get row height based on density
 */
export function getRowHeight(density: 'compact' | 'normal'): string {
  return density === 'compact' ? spacing.row.compact : spacing.row.normal;
}

// =============================================================================
// EXPORTS
// =============================================================================

export { colors, typography, spacing, borderRadius, elevation };
export type Theme = typeof theme;
