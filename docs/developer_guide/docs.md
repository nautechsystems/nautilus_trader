# Docs Style

This guide outlines the style conventions and best practices for writing documentation for NautilusTrader.

## General principles

- We favor simplicity over complexity, less is more.
- We favor concise yet readable prose and documentation.
- We value standardization in conventions, style, patterns, etc.
- Documentation should be accessible to users of varying technical backgrounds.

## Documentation types

Most pages should fit one of four types
([Divio documentation system](https://docs.divio.com/documentation-system/)).
Mixing types in a single page makes it harder to read and harder to maintain.

| Type             | Purpose                          | Section          |
|------------------|----------------------------------|------------------|
| **Tutorial**     | Teach by walking through a task  | `tutorials/`     |
| **How‑to guide** | Solve a specific problem         | `how_to/`        |
| **Explanation**  | Clarify design and architecture  | `concepts/`      |
| **Reference**    | Describe the machinery           | `api_reference/` |

Two sections are exceptions: `getting_started/` is an onboarding path that
combines tutorial-style walkthroughs with setup instructions, and
`integrations/` pages mix reference (capabilities, symbology) with how-to
content (setup, configuration) so each venue page is self-contained.
Standalone how-to content that is not venue-specific belongs in `how_to/`.

### Choosing the right type

- **Does your page walk a newcomer through a learning experience?** Tutorial.
- **Does it answer "How do I...?" for someone who already knows the system?** How-to guide.
- **Does it explain why something works the way it does?** Explanation.
- **Does it list classes, config fields, enums, or capabilities?** Reference.

A tutorial says "do this, then this, then this." The author picks the path.
A how-to guide says "here is how to achieve X." The reader already knows
they want X. Keep these distinct:

- Tutorials should not assume prior knowledge.
- How-to guides should not teach background concepts.

When one type needs to reference another, link to it instead of inlining. For
example, a how-to guide that configures `TradingNodeConfig` should link to the
API reference for field definitions rather than listing them again.

## Language and tone

- Use active voice when possible ("Configure the adapter" vs "The adapter should be configured").
- Write in present tense for describing current functionality.
- Use future tense only for planned features.
- Avoid unnecessary jargon; define technical terms on first use.
- Be direct and concise; avoid filler words like "basically", "simply", "just".
- Use parallel structure in lists; keep grammatical patterns consistent across items.

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
- When referencing code locations, use `file_path::function_name` or `file_path::ClassName` rather than line numbers, which become stale as code changes.

## Headings

We follow modern documentation conventions that prioritize readability and accessibility:

- Use title case for the main page heading (# Level 1 only).
- Use sentence case for all subheadings (## Level 2 and below).
- Always capitalize proper nouns regardless of heading level (product names, technologies, companies, acronyms).
- Use proper heading hierarchy (don't skip levels).

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
- Keep all internal links relative and accurate.

## Technical terminology

- Base capability matrices on the Nautilus domain model, not exchange-specific terminology.
- Mention exchange-specific terms in parentheses or notes when necessary for clarity.
- Use consistent terminology throughout the documentation.

## Examples and code samples

- Provide practical, working examples.
- Include necessary imports and context.
- Use realistic variable names and values.
- Add comments to explain non-obvious parts of examples.

## Admonitions

Use admonition blocks to highlight important information:

| Admonition   | Purpose                                                       |
|--------------|---------------------------------------------------------------|
| `:::note`    | Supplementary context that clarifies but isn't essential.     |
| `:::info`    | Important information the reader should be aware of.          |
| `:::tip`     | Helpful suggestions or best practices.                        |
| `:::warning` | Potential pitfalls or important caveats.                      |
| `:::danger`  | Critical issues that could cause data loss or system failure. |

Avoid overusing admonitions; too many diminish their impact.

## MDX components

The docs site (fumadocs) provides built-in MDX components available in all `.md` files.
No imports are needed.

### Tabs

Use `Tabs` and `Tab` for language-specific or variant code examples.

```markdown
<Tabs items={['Python', 'Rust']}>
<Tab value="Python">
\`\`\`python
strategy.submit_order(order, params={"close_position": True})
\`\`\`
</Tab>
<Tab value="Rust">
\`\`\`rust
let params = Params::from([("close_position", true.into())]);
\`\`\`
</Tab>
</Tabs>
```

### Steps

Use `Steps` and `Step` for sequential procedures.

```markdown
<Steps>
<Step>
Configure the adapter.
</Step>
<Step>
Start the trading node.
</Step>
</Steps>
```

### Accordions

Use `Accordions` and `Accordion` for collapsible content.

```markdown
<Accordions>
<Accordion title="Advanced configuration">
Content here.
</Accordion>
</Accordions>
```

### Files

Use `Files`, `Folder`, and `File` for directory tree visualizations.

```markdown
<Files>
<Folder name="src" defaultOpen>
<File name="main.rs" />
<File name="lib.rs" />
</Folder>
</Files>
```

### Cards

Use `Cards` and `Card` for linked content grids.

```markdown
<Cards>
<Card title="Getting started" href="/latest/getting_started" />
<Card title="Concepts" href="/latest/concepts" />
</Cards>
```

### TypeTable

Use `TypeTable` for parameter or type documentation tables.

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
