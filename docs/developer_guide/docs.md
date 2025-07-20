# Docs Style Guide

This guide outlines the style conventions and best practices for writing documentation for NautilusTrader.

## General principles

- We favor simplicity over complexity, less is more.
- We favor concise yet readable prose and documentation.
- We value standardization in conventions, style, patterns, etc.
- Documentation should be accessible to users of varying technical backgrounds.

## Language and tone

- Use active voice when possible ("Configure the adapter" vs "The adapter should be configured").
- Write in present tense for describing current functionality.
- Use future tense only for planned features.
- Avoid unnecessary jargon; define technical terms on first use.
- Be direct and concise; avoid filler words like "basically", "simply", "just".

## Markdown tables

### Column alignment and spacing

- Use symmetrical column widths based on the space dictated by the widest content in each column.
- Align column separators (`|`) vertically for better readability.
- Use consistent spacing around cell content.

### Notes and descriptions

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

### Support indicators

- Use `✓` for supported features.
- Use `-` for unsupported features (not `✗` or other symbols).
- When adding notes for unsupported features, emphasize with italics: `*Not supported*`.
- Leave cells empty when no content is needed.

## Code references

- Use backticks for inline code, method names, class names, and configuration options.
- Use code blocks for multi-line examples.
- When referencing functions or code locations, include the pattern `file_path:line_number` to allow easy navigation.

## Headings

We follow modern documentation conventions that prioritize readability and accessibility:

- Use title case for the main page heading (# Level 1 only).
- Use sentence case for all subheadings (## Level 2 and below).
- Always capitalize proper nouns regardless of heading level (product names, technologies, companies, acronyms).
- Ensure proper heading hierarchy (don't skip levels).

This convention aligns with industry standards used by major technology companies including Google Developer Documentation, Microsoft Docs, and Anthropic's documentation.
It improves readability, reduces cognitive load, and is more accessible for international users and screen readers.

### Examples

```markdown
# NautilusTrader Developer Guide

## Getting started with Python
## Using the Binance adapter
## REST API implementation
## WebSocket data streaming
## Testing with pytest
```

## Lists

- Use hyphens (`-`) for unordered list bullets; avoid `*` or `+` to keep the Markdown style consistent across the project.
- Use numbered lists only when order matters.
- Maintain consistent indentation for nested lists.
- End list items with periods when they are complete sentences.

## Links and references

- Use descriptive link text (avoid "click here" or "this link").
- Reference external documentation when appropriate.
- Ensure all internal links are relative and accurate.

## Technical terminology

- Base capability matrices on the Nautilus domain model, not exchange-specific terminology.
- Mention exchange-specific terms in parentheses or notes when necessary for clarity.
- Use consistent terminology throughout the documentation.

## Examples and code samples

- Provide practical, working examples.
- Include necessary imports and context.
- Use realistic variable names and values.
- Add comments to explain non-obvious parts of examples.

## Warnings and notes

- Use appropriate admonition blocks for important information:
  - `:::note` for general information.
  - `:::warning` for important caveats.
  - `:::tip` for helpful suggestions.

## Line length and wrapping

- Wrap lines at no more than ~100-120 characters for better readability and diff reviews.
- Break long sentences at natural points (after commas, conjunctions, or phrases).
- Avoid orphaned words on new lines when possible.
- Code blocks and URLs can exceed the line limit when necessary.

## API documentation

- Document parameters and return types clearly.
- Include usage examples for complex APIs.
- Explain any side effects or important behavior.
- Keep parameter descriptions concise but complete.
