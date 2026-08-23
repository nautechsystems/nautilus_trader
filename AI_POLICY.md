# AI Policy

**AI tools may assist with project work, but active human thinking and judgment remain essential.**

NautilusTrader holds AI-assisted work to the same quality standard as any other contribution.

## Human responsibility

**The human contributor remains responsible for every issue, change, review, and comment they
submit.**

Before submitting AI-assisted work, review it in full, verify it, and make sure you understand and
can explain every part. You must understand not only how the change works, but why the change fits
the project's architecture and maintained scope. Do not submit speculative, untested, or
unnecessarily complex output for maintainers to validate.

**A human must choose the work, direct the implementation, and verify the result. They must
authorize every public interaction with GitHub and remain accountable for each one.** Agents acting
for contributors must not take action in the NautilusTrader project on GitHub without explicit
human direction and authorization. This includes opening or editing issues or pull requests,
requesting review, tagging or pinging maintainers, posting comments, or responding to maintainers.
Fully autonomous contributions, where an agent acts without meaningful human direction and review,
are not accepted.

Do not use AI to copy, rewrite, or disguise third-party code that you could not otherwise submit
under the [Contributor License Agreement](CLA.md).

AI tools and agents must not accept the Contributor License Agreement, certify authorship or
licensing rights, or make equivalent legal representations on a contributor's behalf.

## Quality

**Maintainers have seen higher-quality pull requests when AI workflows strengthen quality assurance
and thoroughness across design, implementation, and testing instead of stopping at an unrefined
first pass.** For AI-assisted work, independent critical review by another person or a separate
agent session under your direction can help find gaps before submission. This review process is
optional, but a human must make the final judgment on its findings, address valid issues, and verify
the updated work before submission.

These practices do not guarantee acceptance, but they can reduce avoidable review cycles.

## Maintainer effort

AI-assisted workflows should reduce the total engineering effort needed to produce a review-ready
contribution. They must not shift design, implementation, debugging, or validation work onto
maintainers.

## Communication

AI may help draft issues, pull request descriptions, review comments, and responses to maintainers.
You do not need to draft every sentence without AI. You remain responsible for the final text:
understand every claim, verify its accuracy, and present the information clearly and readably.

Review AI-assisted prose before submitting it. Unrefined output is often generic or bloated, which
makes it tedious to review and can hide important information. Remove filler and lead with the
important details.

**We encourage you to preserve your authorship in AI-assisted writing**: decide how to present the
ideas, and use AI to help express and refine them. Aim to retain your judgment and voice in the
final text.

AI can also help with editing, translation, accessibility, or structuring technical prose. Review
the result and make sure it preserves your intended meaning.

## Disclosure and attribution

NautilusTrader does not require disclosure of AI assistance. This policy does not override any
legal, contractual, or license obligation that applies to the contributor or submitted material.
If you choose to disclose AI assistance in a commit message, keep the wording general, such as
`Developed with assistance from AI.`

NautilusTrader is neutral among AI labs, vendors, models, and tools. To keep the public
contribution record neutral, do not name or promote one as part of attribution in commit messages,
pull request titles, or pull request descriptions, including summaries. Do not add branded footers
such as `Generated with ...`.

Maintainers may generalize or remove specific attribution when merging, or ask the contributor to
amend the affected commit messages or pull request descriptions.

Do not list an AI tool or model as an author, co-author, or contributor. This includes authorship or
contributor trailers, such as `Co-authored-by:`, that name an AI tool or model.

## Enforcement

Maintainers may ask contributors to correct minor attribution or presentation issues. They may close
a contribution without detailed review when substantive problems remain, such as inaccurate or
unverified content, inadequate testing, repeated failures in pre-commit checks or tests, or
a reasonable concern that the contribution lacks meaningful human direction and review. Comments
containing substantial unverified content may be hidden.

This policy was informed by
[Astral's AI Policy](https://github.com/astral-sh/.github/blob/c5187e200db51bfe11d56e13053d29bd3793fdd8/AI_POLICY.md)
and the [LLVM AI Tool Use Policy](https://llvm.org/docs/AIToolPolicy.html).
