# Markdown Style

Standard revision: 1

This document defines the shared Markdown baseline for AgentSkills and repositories that adopt a
local copy. The maintained source is `references/markdown-style.md` in the AgentSkills repository.
Keep repository copies byte-for-byte identical and put repository-specific additions in a separate
local guide.

## Language and extensions

- Use [CommonMark](https://spec.commonmark.org/current/) as the base specification.
- Use [GitHub Flavored Markdown](https://github.github.com/gfm/) for tables, task lists,
  strikethrough, and autolinks.
- Use additional front matter, Markdown extensions, or renderer components only when the
  repository documents and supports them.
- Do not introduce syntax that depends on an undocumented renderer extension.

The linked specifications are references, not routine prerequisites. Open them only to resolve a
concrete parser or renderer ambiguity that the local guide, surrounding content, and markdownlint
do not answer.

## Enforcement

- Follow the repository's `.markdownlint.jsonc` or equivalent local configuration.
- Treat this standard as the intended style and markdownlint as automated enforcement of its
  mechanical subset.
- Treat an undocumented conflict between this standard and the local configuration as drift to
  resolve, not an implicit exception.
- Ensure all authored Markdown in the repository's lint scope passes its configured Markdown check.
- Do not hand-edit generated Markdown. Change its source and run the generator.

Generated files, imported third-party documents, and renderer fixtures may use documented
repository exclusions.

## Headings

- Use ATX headings (`#`, `##`, `###`, and so on).
- Use one H1 as the first Markdown heading.
- Use title case for H1.
- Use sentence case for H2 and below.
- Maintain a logical hierarchy without skipping heading levels.
- Leave one blank line above and below each heading.

## Paragraphs and wrapping

- Separate paragraphs with one blank line.
- Do not add consecutive blank lines.
- Treat 100-120 characters as a soft line-length target.
- Break prose at natural boundaries and allow a longer line rather than leaving one to three words
  on the next line.
- Allow code blocks, tables, and long link destinations to exceed the target when needed.

## Lists

- Use `-` for unordered list items.
- Use ordered lists only when sequence matters.
- Use `1.` for every source item in an ordered list so the renderer supplies the displayed numbers.
- Keep list indentation and spacing consistent with the local markdownlint configuration.
- Leave one blank line before and after a list.

Example:

```markdown
- First item
- Second item

1. First step
1. Second step
```

## Tables

- Use GFM pipe tables for tabular content.
- Include leading and trailing pipes.
- Align pipe characters vertically.
- Pad each column to its widest cell plus one space either side, which matches Prettier's
  default table output. MD060 checks that pipes line up but ignores cell content, so it
  accepts a column padded far wider than anything in it.
- Pad delimiter cells with spaces (`| ----- |`, not `|-----|`); MD060 does not check this
  either.
- The `normalize markdown table padding` pre-commit hook rewrites tables to this form, so
  there is no need to count characters by hand.
- Left-align text columns and right-align numeric columns where appropriate.
- Keep the delimiter row consistent with the intended rendered alignment.
- Avoid HTML tables unless Markdown cannot express the required structure or the repository
  documents the need.

Example:

```markdown
| Name  | Value |
| ----- | ----: |
| Alpha |    42 |
| Beta  |    17 |
```

## Code

- Use backtick-fenced code blocks instead of indented code blocks.
- Specify a language for every opening fence. Use `text` for plain text or output without a more
  specific grammar.
- Use a longer outer fence when documenting fenced Markdown.
- Use inline code for commands, file names, functions, types, environment variables, configuration
  keys, and identifiers.

Example:

````markdown
```rust
fn main() {
    println!("Hello");
}
```
````

Run `cargo test` and edit `.markdownlint.jsonc`.

## Emphasis

- Use `*italic*` for emphasis.
- Use `**bold**` for strong emphasis.
- Avoid emphasis that does not add meaning.

## Thematic breaks

Use three hyphens:

```markdown
---
```

## Links and images

- Use descriptive link text.
- Use inline links by default.
- Use reference-style links when the same destination appears more than once in a document.
- Avoid bare URLs.
- Keep internal links relative when the repository's renderer supports them.
- Give images useful alternative text. Use empty alternative text only for deliberately decorative
  images.

## HTML

- Prefer portable Markdown syntax over raw HTML.
- Use raw HTML only when Markdown cannot express the required result or the repository documents
  the element or component.
- Preserve supported front matter, MDX components, fence attributes, and other repository
  extensions when editing surrounding content.

## Files

- Use UTF-8 encoding.
- Use LF line endings.
- End each file with one newline.
- Do not leave trailing whitespace.

## Agent guidance

When generating or modifying Markdown:

- For narrow edits, preserve surrounding style and use markdownlint without loading the full guide
  unless both leave a concrete question unanswered.
- Before creating or substantially restructuring documentation, consult only the relevant sections
  of the repository-local Markdown guide.
- Preserve the existing document structure unless the task requires a change.
- Keep formatting consistent with surrounding content and supported renderer extensions.
- Do not reformat unrelated sections.
- Run the repository's focused Markdown check when available.

## Adoption state

ATX headings are enforced through `MD003`. Repeated `1.` ordered-list markers and languages on
opening code fences apply to new or substantially edited content, but `MD029` and `MD040` remain
disabled until existing documents receive separate mechanical migrations. Do not broaden a narrow
documentation change solely to migrate those existing constructs.
