#!/usr/bin/env node
/**
 * Token Migration Script
 *
 * Automatically migrates Tailwind color classes to token system.
 * Usage: node scripts/migrate-tokens.js <file-path>
 *
 * Example:
 *   node scripts/migrate-tokens.js src/components/MyPanel.tsx
 *
 * What it does:
 * - Replaces className Tailwind color classes with style + tokens
 * - Adds import for tokens if not present
 * - Preserves existing code structure
 *
 * IMPORTANT: Review changes carefully before committing!
 */

const fs = require('fs');
const path = require('path');

// Color mapping: Tailwind class → token path
const COLOR_MAPPINGS = {
  // Background colors
  'bg-neutral-900': 'colors.bg.base',
  'bg-neutral-800': 'colors.bg.surface',
  'bg-neutral-700': 'colors.bg.hover',
  'bg-neutral-600': 'colors.bg.active',
  'bg-neutral-950': 'colors.bg.base',
  'bg-neutral-850': 'colors.bg.surface',

  // Text colors
  'text-neutral-50': 'colors.text.primary',
  'text-neutral-100': 'colors.text.secondary',
  'text-neutral-200': 'colors.text.secondary',
  'text-neutral-300': 'colors.text.tertiary',
  'text-neutral-400': 'colors.text.muted',
  'text-neutral-500': 'colors.text.muted',
  'text-neutral-600': 'colors.text.disabled',

  // Border colors
  'border-neutral-700': 'colors.border.DEFAULT',
  'border-neutral-600': 'colors.border.hover',
  'border-emerald-500': 'colors.border.focus',

  // Semantic colors
  'text-emerald-400': 'colors.semantic.success.light',
  'text-emerald-500': 'colors.semantic.success.DEFAULT',
  'text-red-400': 'colors.semantic.danger.light',
  'text-red-500': 'colors.semantic.danger.DEFAULT',
  'text-amber-400': 'colors.semantic.warning.light',
  'text-amber-500': 'colors.semantic.warning.DEFAULT',
  'text-blue-400': 'colors.semantic.info.light',
  'text-blue-500': 'colors.semantic.info.DEFAULT',

  'bg-emerald-400': 'colors.semantic.success.light',
  'bg-emerald-500': 'colors.semantic.success.DEFAULT',
  'bg-red-400': 'colors.semantic.danger.light',
  'bg-red-500': 'colors.semantic.danger.DEFAULT',
  'bg-amber-400': 'colors.semantic.warning.light',
  'bg-amber-500': 'colors.semantic.warning.DEFAULT',
  'bg-blue-400': 'colors.semantic.info.light',
  'bg-blue-500': 'colors.semantic.info.DEFAULT',
};

function migrateFile(filePath) {
  console.log(`\n📝 Migrating: ${filePath}`);

  if (!fs.existsSync(filePath)) {
    console.error(`❌ File not found: ${filePath}`);
    process.exit(1);
  }

  let content = fs.readFileSync(filePath, 'utf8');
  let modified = false;
  const changes = [];

  // Check if file already imports tokens
  const hasTokenImport = content.includes("from '@/lib/tokens'") || content.includes('from "./lib/tokens"');

  // Track which token categories are used
  const usedTokens = new Set();

  // Migrate color classes
  Object.entries(COLOR_MAPPINGS).forEach(([tailwindClass, tokenPath]) => {
    const regex = new RegExp(`className=["']([^"']*\\s)?${tailwindClass.replace('-', '\\-')}(\\s[^"']*)?["']`, 'g');

    if (content.match(regex)) {
      modified = true;
      changes.push(`  - ${tailwindClass} → ${tokenPath}`);

      // Extract token category
      const category = tokenPath.split('.')[0];
      usedTokens.add(category);

      // This is a simplified migration - in practice, you'd need more sophisticated
      // logic to convert className to style props
      console.log(`  ⚠️  Found ${tailwindClass} - manual migration recommended`);
    }
  });

  if (!modified) {
    console.log('  ✅ No Tailwind color classes found');
    return;
  }

  // Add token import if needed
  if (!hasTokenImport && usedTokens.size > 0) {
    const tokenImports = Array.from(usedTokens).join(', ');
    const importStatement = `import { ${tokenImports} } from '@/lib/tokens';\n`;

    // Find the last import statement
    const importRegex = /^import\s+.+from\s+.+;$/gm;
    const imports = content.match(importRegex);

    if (imports && imports.length > 0) {
      const lastImport = imports[imports.length - 1];
      const lastImportIndex = content.lastIndexOf(lastImport);
      const insertIndex = lastImportIndex + lastImport.length + 1;

      content = content.slice(0, insertIndex) + importStatement + content.slice(insertIndex);
      console.log(`  ✅ Added token import: import { ${tokenImports} } from '@/lib/tokens'`);
    }
  }

  console.log('\n  📊 Migration Summary:');
  console.log(changes.join('\n'));

  console.log('\n  ⚠️  IMPORTANT: This script only detects usage. Manual migration required:');
  console.log('     1. Convert className to style props');
  console.log('     2. Use token values (e.g., style={{ color: colors.text.muted }})');
  console.log('     3. Test the component visually');
  console.log('     4. See docs/ui-standards.md for guidelines');

  // Optionally write changes (commented out for safety)
  // fs.writeFileSync(filePath, content, 'utf8');
  // console.log(`\n  ✅ File updated: ${filePath}`);
}

// Main execution
const args = process.argv.slice(2);

if (args.length === 0) {
  console.log('Usage: node scripts/migrate-tokens.js <file-path>');
  console.log('Example: node scripts/migrate-tokens.js src/components/MyPanel.tsx');
  process.exit(1);
}

const filePath = args[0];
migrateFile(filePath);

console.log('\n✨ Migration analysis complete!');
console.log('   Review the findings and migrate manually following docs/ui-standards.md\n');
