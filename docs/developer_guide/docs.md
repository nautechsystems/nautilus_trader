# Documentation Style Guide

This guide outlines the style conventions and best practices for writing documentation for NautilusTrader.

## General Principles

- We favor simplicity over complexity, less is more.
- We favor concise yet readable prose and documentation.
- We value standardization in conventions, style, patterns, etc.
- Documentation should be accessible to users of varying technical backgrounds.

## Markdown Tables

### Column Alignment and Spacing

- Use symmetrical column widths based on the space necessary dictated by the widest content in each column.
- Align column separators (`|`) vertically for better readability.
- Use consistent spacing around cell content.

### Notes and Descriptions

- All notes and descriptions should have terminating periods.
- Keep notes concise but informative.
- Use sentence case (capitalize only the first letter and proper nouns).

### Example

```markdown
| Order Type             | Spot | Margin | USDT Futures | Coin Futures | Notes                   |
|------------------------|------|--------|--------------|--------------|-------------------------|
| `MARKET`               | ✓    | ✓      | ✓            | ✓            |                         |
| `STOP_MARKET`          | -    | ✓      | ✓            | ✓            | Not supported for Spot. |
| `MARKET_IF_TOUCHED`    | -    | -      | ✓            | ✓            | Futures only.           |
```

### Support Indicators

- Use `✓` for supported features.
- Use `-` for unsupported features (not `✗` or other symbols).
- When adding notes for unsupported features, emphasize with italics: `*Not supported*`.
- Leave cells empty when no content is needed.

## Code References

- Use backticks for inline code, method names, class names, and configuration options.
- Use code blocks for multi-line examples.
- When referencing functions or code locations, include the pattern `file_path:line_number` to allow easy navigation.

## Headings

- Use title case for main headings (## Level 2).
- Use sentence case for subheadings (### Level 3 and below).
- Ensure proper heading hierarchy (don't skip levels).

## Lists

- Use hyphens (`-`) for unordered-list bullets; avoid `*` or `+` to keep the Markdown style
   consistent across the project.
- Use numbered lists only when order matters.
- Maintain consistent indentation for nested lists.
- End list items with periods when they are complete sentences.

## Links and References

- Use descriptive link text (avoid "click here" or "this link").
- Reference external documentation when appropriate.
- Ensure all internal links are relative and accurate.

## Technical Terminology

- Base capability matrices on the Nautilus domain model, not exchange-specific terminology.
- Mention exchange-specific terms in parentheses or notes when necessary for clarity.
- Use consistent terminology throughout the documentation.

## Examples and Code Samples

- Provide practical, working examples.
- Include necessary imports and context.
- Use realistic variable names and values.
- Add comments to explain non-obvious parts of examples.

## Warnings and Notes

- Use appropriate admonition blocks for important information:
  - `:::note` for general information.
  - `:::warning` for important caveats.
  - `:::tip` for helpful suggestions.

## API Documentation

- Document parameters and return types clearly.
- Include usage examples for complex APIs.
- Explain any side effects or important behavior.
- Keep parameter descriptions concise but complete.
