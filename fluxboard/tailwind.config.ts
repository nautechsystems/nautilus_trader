import type { Config } from 'tailwindcss';
import { getTailwindTheme, colors as tokenColors } from './lib/tokens';

const tokenTheme = getTailwindTheme();

export default {
  content: [
    "./index.html",
    "./*.{ts,tsx}",
    "./components/**/*.{ts,tsx}",
    "./utils/**/*.{ts,tsx}",
    "./__tests__/**/*.{ts,tsx}",
    "./fluxboard/**/*.{ts,tsx}"
  ],
  theme: {
    extend: {
      // Integrate design token system
      ...tokenTheme,
      colors: {
      ...tokenTheme.colors,
      zinc: tokenTheme.colors.neutral,
      emerald: {
        400: tokenColors.semantic.success.light,
        500: tokenColors.semantic.success.DEFAULT,
          600: tokenColors.semantic.success.dark,
          700: tokenColors.semantic.success.darker,
        },
        amber: {
          400: tokenColors.semantic.warning.light,
          500: tokenColors.semantic.warning.DEFAULT,
          600: tokenColors.semantic.warning.dark,
        },
        red: {
          400: tokenColors.semantic.danger.light,
          500: tokenColors.semantic.danger.DEFAULT,
          600: tokenColors.semantic.danger.dark,
          700: tokenColors.semantic.danger.darker,
        },
        blue: {
          400: tokenColors.semantic.info.light,
          500: tokenColors.semantic.info.DEFAULT,
          600: tokenColors.semantic.info.dark,
        },
      },

      // Add surface colors for Tailwind utility classes
      backgroundColor: {
        'surface-1': 'rgba(13, 14, 16, 1)',
        'surface-2': 'rgba(20, 21, 23, 1)',
        'surface-3': 'rgba(28, 29, 32, 1)',
      },

      // Keep existing custom keyframes and animations
      keyframes: {
        flash: {
          '0%': { backgroundColor: 'rgb(6 78 59 / 0.3)' },  // emerald-900/30
          '50%': { backgroundColor: 'rgb(6 78 59 / 0.5)' }, // emerald-900/50
          '100%': { backgroundColor: 'transparent' }
        },
        'flash-green': {
          '0%': { backgroundColor: 'rgba(34, 197, 94, 0.2)' },
          '100%': { backgroundColor: 'transparent' }
        },
        'flash-red': {
          '0%': { backgroundColor: 'rgba(239, 68, 68, 0.2)' },
          '100%': { backgroundColor: 'transparent' }
        }
      },
      animation: {
        flash: 'flash 500ms ease-out',
        'flash-green': 'flash-green 500ms ease-out',
        'flash-red': 'flash-red 500ms ease-out'
      }
    }
  },
  plugins: []
} satisfies Config;
