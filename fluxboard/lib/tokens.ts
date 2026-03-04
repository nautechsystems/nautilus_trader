/**
 * Fluxboard Design Tokens
 *
 * Centralized design system tokens for colors, typography, spacing, and semantic values.
 * All tokens follow the spec from the UI/UX standardization document.
 */

// =============================================================================
// COLOR TOKENS
// =============================================================================

/**
 * Background hierarchy (darkest → lighter for layering)
 */
export const colors = {
  // Base backgrounds
  bg: {
    base: '#0b0b0c',      // Base
    surface: '#101112',   // Primary panels
    hover: '#151618',     // Hover state
    active: '#1b1c1f',    // Active/pressed state
    zebra: 'rgba(255, 255, 255, 0.02)', // Subtle alternating row background
  },

  // Neutral grays (for borders, text, disabled states)
  neutral: {
    50: '#f2f3f5',
    100: '#e4e6ea',
    200: '#c7ccd3',
    300: '#aab1ba',
    400: '#8f959f',
    500: '#7a7f88',
    600: '#656973',
    700: '#4f525a',
    800: '#3b3d44',
    900: '#25272d',
    950: '#15161c',
  },

  // Semantic colors
  semantic: {
    success: {
      DEFAULT: '#2f9b74',
      light: '#38a47c',
      dark: '#24795f',
      darker: '#1c5f4b',
      bg: 'rgba(47, 155, 116, 0.12)',
      border: 'rgba(47, 155, 116, 0.28)',
    },
    danger: {
      DEFAULT: '#c64c58',
      light: '#d75f6a',
      dark: '#a63c45',
      darker: '#843038',
      bg: 'rgba(198, 76, 88, 0.12)',
      border: 'rgba(198, 76, 88, 0.26)',
    },
    warning: {
      DEFAULT: '#c18a3a',
      light: '#d09a4a',
      dark: '#9f6f2f',
      bg: 'rgba(193, 138, 58, 0.14)',
      border: 'rgba(193, 138, 58, 0.28)',
    },
    info: {
      DEFAULT: '#5f7ac3',
      light: '#7a94d9',
      dark: '#4a629d',
      bg: 'rgba(95, 122, 195, 0.12)',
      border: 'rgba(95, 122, 195, 0.26)',
    },
  },

  // Accent color (primary brand color - emerald green)
  accent: {
    DEFAULT: '#2f9b74',
    muted: '#287d5e',
    hover: '#38a47c',
  },

  // Text color tokens
  text: {
    primary: '#e6e7ea',
    secondary: '#c2c4c8',
    tertiary: '#9b9fa5',
    muted: '#80838b',
    disabled: '#595c63',
  },

  // Border colors
  border: {
    DEFAULT: '#1f2024',
    hover: '#2a2c31',
    focus: '#2f9b74',
  },

  // Table-specific colors (PnL panel grouped headers)
  table: {
    groupHeader: {
      qty: 'rgba(95, 122, 195, 0.04)',
      prices: 'rgba(58, 143, 123, 0.04)',
      pnl: 'rgba(212, 95, 99, 0.05)',
      ops: 'rgba(200, 148, 60, 0.05)',
      details: 'rgba(84, 92, 108, 0.04)',
    },
  },
} as const;

// Semantic aliases (mapping to core palette for easy theming)
export const semanticTokens = {
  surface: colors.bg.surface,
  surfaceAlt: colors.bg.hover,
  textPrimary: colors.text.primary,
  textMuted: colors.text.muted,
  status: {
    success: colors.semantic.success.light, // emerald-400 equivalent
    danger: colors.semantic.danger.light,   // rose-400 equivalent
    warning: colors.semantic.warning.light, // amber-400 equivalent
  },
} as const;

// =============================================================================
// TYPOGRAPHY TOKENS
// =============================================================================

export const typography = {
  // Font families
  fontFamily: {
    sans: '"IBM Plex Sans", Inter, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif',
    mono: '"JetBrains Mono", "SFMono-Regular", Menlo, monospace',
  },

  // Font sizes (pixel values)
  fontSize: {
    '2xs': '10px',    // Micro labels
    'xs': '11px',     // Small UI elements
    'sm': '12px',     // Dense table data
    'base': '13px',   // Standard UI text
    'md': '14px',     // Headers / Comfortable text
    'lg': '16px',     // Section headers
    'xl': '19px',     // Page headers
    '2xl': '22px',    // Large stats
  },

  // Font weights
  fontWeight: {
    normal: 400,
    medium: 500,
    semibold: 600,
    bold: 700,
  },

  // Line heights
  lineHeight: {
    tight: 1.28,
    normal: 1.45,
    relaxed: 1.7,
  },
} as const;

// =============================================================================
// SPACING TOKENS
// =============================================================================

export const spacing = {
  // Panel spacing
  panel: {
    paddingHorizontal: '20px',
    paddingVertical: '18px',
    gap: '14px',
  },

  // Row heights
  row: {
    compact: '28px',    // Dense mode
    normal: '32px',     // Standard mode
    header: '42px',     // Header height
  },

  // Common gaps
  gap: {
    xs: '0.375rem',     // 6px - Button groups
    sm: '0.625rem',     // 10px - Form fields
    md: '0.75rem',      // 12px - Panel spacing
    lg: '1rem',         // 16px - Section spacing
    xl: '1.5rem',       // 24px - Large spacing
  },

  // Padding presets
  padding: {
    xs: '4px',
    sm: '8px',
    dense: '6px',
    normal: '12px',
    md: '14px',
    comfortable: '18px',
  },
} as const;

// =============================================================================
// DENSITY GUIDANCE (DESKTOP VS MOBILE)
// =============================================================================

export const densityScale = {
  desktop: {
    fontSize: {
      normal: typography.fontSize.base,
      dense: typography.fontSize.sm,
    },
    padding: {
      normal: spacing.padding.normal,
      dense: spacing.padding.dense,
    },
    rowHeight: {
      normal: spacing.row.normal,
      dense: spacing.row.compact,
    },
    controlHeight: 32,
    tapTarget: 32,
  },
  mobile: {
    fontSize: {
      normal: typography.fontSize.md,
      dense: typography.fontSize.base,
    },
    padding: {
      normal: '12px',
      dense: '8px',
    },
    rowHeight: {
      normal: '40px',
      dense: '32px',
    },
    controlHeight: 44,
    tapTarget: 44,
  },
} as const;

// =============================================================================
// BORDER RADIUS TOKENS
// =============================================================================

export const borderRadius = {
  none: '0',
  sm: '0.125rem',     // 2px
  DEFAULT: '0.1875rem', // 3px
  md: '0.25rem',      // 4px
  lg: '0.375rem',     // 6px
  xl: '0.5rem',       // 8px
  full: '9999px',     // Fully rounded (pills)
} as const;

// =============================================================================
// ELEVATION (Z-INDEX) SYSTEM
// =============================================================================

export const elevation = {
  base: 0,           // Default layer
  panel: 1,          // Panels
  header: 10,        // Sticky headers
  dropdown: 50,      // Dropdowns, popovers
  overlay: 100,      // Overlays, backdrops
  modal: 200,        // Modals, dialogs
  toast: 300,        // Toast notifications
  tooltip: 400,      // Tooltips (highest)
} as const;

// =============================================================================
// ANIMATION TOKENS
// =============================================================================

export const animation = {
  duration: {
    fast: '150ms',
    normal: '250ms',
    slow: '500ms',
  },
  easing: {
    easeIn: 'cubic-bezier(0.4, 0, 1, 1)',
    easeOut: 'cubic-bezier(0, 0, 0.2, 1)',
    easeInOut: 'cubic-bezier(0.4, 0, 0.2, 1)',
  },
} as const;

// =============================================================================
// LIVE UPDATE INTERVALS
// =============================================================================

/**
 * Standard intervals for polling and live updates (milliseconds).
 *
 * Tiers are intentionally opinionated so every panel can speak a consistent timing language:
 * - `CRITICAL` (1s) – trade streams, hedger heartbeat, anything that backs a kill-switch.
 * - `FAST` (2s) – frequently changing data such as FX dashboards or params auto-refresh.
 * - `NORMAL` (3s) – default polling for alerts, discovery feeds, and healthy-but-not-critical data.
 * - `SLOW` (5s) – heavy payloads (balances, FX fallback) that can tolerate slight lag.
 * - `MANUAL` (10s) – low-frequency tasks (PnL reports, manual refresh helpers).
 *
 * Pair these with `STALE_THRESHOLDS` below so freshness indicators stay aligned across screens.
 */
export const INTERVALS = {
  CRITICAL: 1000,   // 1s - Trade blotter, critical alerts
  FAST: 2000,       // 2s - Strategy params, signal data
  NORMAL: 3000,     // 3s - Alerts, market data fallback
  SLOW: 5000,       // 5s - Balances, FX rates
  MANUAL: 10000,    // 10s - PnL calculations, manual refresh
} as const;

/**
 * Stale thresholds for freshness indicators (milliseconds).
 *
 * Each threshold maps directly to a polling tier:
 * - `REALTIME` ≈ 2× `CRITICAL` (2s) so 1 missed update immediately surfaces.
 * - `FAST` ≈ 2–3× `FAST` (5s) for FX and params grids.
 * - `NORMAL` ≈ 3× `NORMAL` (10s) for alerts/market data.
 * - `SLOW` (~30s) for expensive refreshes like balances.
 *
 * Use these instead of ad-hoc multipliers—Panels should never guess their stale window.
 */
export const STALE_THRESHOLDS = {
  REALTIME: 2000,   // 2s - Real-time data (trades, signals)
  FAST: 5000,       // 5s - Fast-updating data (params, market data)
  NORMAL: 10000,    // 10s - Normal data (balances, FX)
  SLOW: 30000,      // 30s - Slow-updating data (PnL, reports)
} as const;

// =============================================================================
// SEMANTIC TOKEN HELPERS
// =============================================================================

/**
 * Severity levels for alerts and status indicators
 */
export const severity = {
  critical: {
    color: colors.semantic.danger.light,
    text: colors.semantic.danger.light,
    bg: colors.semantic.danger.bg,
    border: colors.semantic.danger.dark,
  },
  warning: {
    color: colors.semantic.warning.light,
    text: colors.semantic.warning.light,
    bg: colors.semantic.warning.bg,
    border: colors.semantic.warning.dark,
  },
  info: {
    color: colors.semantic.info.light,
    text: colors.semantic.info.light,
    bg: colors.semantic.info.bg,
    border: colors.semantic.info.dark,
  },
  success: {
    color: colors.semantic.success.light,
    text: colors.semantic.success.light,
    bg: colors.semantic.success.bg,
    border: colors.semantic.success.dark,
  },
} as const;

/**
 * Trade side colors (buy/sell)
 */
export const tradeSide = {
  buy: {
    color: colors.semantic.success.light,
    bg: colors.semantic.success.bg,
    badge: colors.semantic.success.dark,
  },
  sell: {
    color: colors.semantic.danger.light,
    bg: colors.semantic.danger.bg,
    badge: colors.semantic.danger.dark,
  },
} as const;

// =============================================================================
// CSS VARIABLE EXPORT
// =============================================================================

/**
 * Generate CSS custom properties for use in Tailwind config
 */
export function generateCSSVariables() {
  return {
    '--color-bg': colors.bg.base,
    '--color-surface': colors.bg.surface,
    '--bg-surface': colors.bg.surface,
    '--bg-surface-alt': colors.bg.hover,
    '--color-bg-hover': colors.bg.hover,
    '--color-bg-active': colors.bg.active,
    '--color-accent': colors.accent.DEFAULT,
    '--color-negative': colors.semantic.danger.DEFAULT,
    '--color-positive': colors.semantic.success.DEFAULT,
    '--color-muted': colors.text.muted,
    '--color-text-primary': colors.text.primary,
    '--text-primary': colors.text.primary,
    '--text-muted': colors.text.muted,
    '--color-text-secondary': colors.text.secondary,
    '--color-text-tertiary': colors.text.tertiary,
    '--color-text-disabled': colors.text.disabled,
    '--color-border': colors.border.DEFAULT,
    '--color-border-hover': colors.border.hover,
    '--color-border-focus': colors.border.focus,
    '--status-success': semanticTokens.status.success,
    '--status-danger': semanticTokens.status.danger,
    '--status-warning': semanticTokens.status.warning,

    '--font-sans': typography.fontFamily.sans,
    '--font-mono': typography.fontFamily.mono,

    '--row-compact': spacing.row.compact,
    '--row-normal': spacing.row.normal,
    '--row-header': spacing.row.header,
  };
}

// =============================================================================
// TAILWIND THEME INTEGRATION
// =============================================================================

/**
 * Generate Tailwind theme configuration from design tokens.
 * Import this in tailwind.config.ts to integrate the token system.
 *
 * @example
 * ```ts
 * // tailwind.config.ts
 * import { getTailwindTheme } from '@/lib/tokens';
 *
 * export default {
 *   theme: {
 *     extend: getTailwindTheme(),
 *   },
 * };
 * ```
 */
export function getTailwindTheme() {
  return {
    colors: {
      bg: colors.bg,
      neutral: colors.neutral,
      semantic: colors.semantic,
      accent: colors.accent,
      surface: {
        DEFAULT: colors.bg.surface,
        alt: colors.bg.hover,
      },
      status: {
        success: semanticTokens.status.success,
        danger: semanticTokens.status.danger,
        warning: semanticTokens.status.warning,
      },
      textTokens: {
        primary: colors.text.primary,
        muted: colors.text.muted,
      },
      text: colors.text,
      border: colors.border,
      table: colors.table,
      // Flatten semantic colors for easier Tailwind access
      success: {
        DEFAULT: colors.semantic.success.DEFAULT,
        light: colors.semantic.success.light,
        dark: colors.semantic.success.dark,
        darker: colors.semantic.success.darker,
        bg: colors.semantic.success.bg,
        border: colors.semantic.success.border,
      },
      danger: {
        DEFAULT: colors.semantic.danger.DEFAULT,
        light: colors.semantic.danger.light,
        dark: colors.semantic.danger.dark,
        darker: colors.semantic.danger.darker,
        bg: colors.semantic.danger.bg,
        border: colors.semantic.danger.border,
      },
      warning: {
        DEFAULT: colors.semantic.warning.DEFAULT,
        light: colors.semantic.warning.light,
        dark: colors.semantic.warning.dark,
        bg: colors.semantic.warning.bg,
        border: colors.semantic.warning.border,
      },
      info: {
        DEFAULT: colors.semantic.info.DEFAULT,
        light: colors.semantic.info.light,
        dark: colors.semantic.info.dark,
        bg: colors.semantic.info.bg,
        border: colors.semantic.info.border,
      },
    },
    fontFamily: typography.fontFamily,
    fontSize: typography.fontSize,
    fontWeight: Object.fromEntries(
      Object.entries(typography.fontWeight).map(([k, v]) => [k, String(v)])
    ),
    lineHeight: Object.fromEntries(
      Object.entries(typography.lineHeight).map(([k, v]) => [k, String(v)])
    ),
    borderRadius: borderRadius,
    spacing: {
      // Add custom spacing values
      ...spacing.gap,
      'panel-x': spacing.panel.paddingHorizontal,
      'panel-y': spacing.panel.paddingVertical,
      'panel-gap': spacing.panel.gap,
    },
    zIndex: Object.fromEntries(
      Object.entries(elevation).map(([k, v]) => [k, String(v)])
    ),
    transitionDuration: animation.duration,
    transitionTimingFunction: animation.easing,
  };
}

/**
 * Helper function to apply dense mode styles consistently
 * @param dense Whether dense mode is active
 * @returns Style object with appropriate density values
 */
export function getDensityStyles(dense: boolean, density: keyof typeof densityScale = 'desktop') {
  const preset = densityScale[density];
  return {
    padding: dense ? preset.padding.dense : preset.padding.normal,
    fontSize: dense ? preset.fontSize.dense : preset.fontSize.normal,
    height: dense ? preset.rowHeight.dense : preset.rowHeight.normal,
  };
}

// =============================================================================
// TYPE EXPORTS
// =============================================================================

export type ColorToken = typeof colors;
export type TypographyToken = typeof typography;
export type SpacingToken = typeof spacing;
export type SemanticTokens = typeof semanticTokens;
export type DensityScale = typeof densityScale;
export type SeverityLevel = keyof typeof severity;
export type TradeSide = keyof typeof tradeSide;
export type IntervalType = keyof typeof INTERVALS;
export type StaleThreshold = keyof typeof STALE_THRESHOLDS;
