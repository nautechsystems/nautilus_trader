---
name: executing-plans
description: Use when you have a written implementation plan to execute in a separate session with review checkpoints
---

# Executing Plans

## Overview

Load plan, review critically, execute tasks in batches, report for review between batches.

**Core principle:** Batch execution with checkpoints for architect review.

**Announce at start:** "I'm using the executing-plans skill to implement this plan."

## The Process

### Step 1: Load and Review Plan
1. Read plan file
2. Verify the plan contains a `## Progress Tracker` and treat it as the source of truth
3. Review critically - identify any questions or concerns about the plan
4. If concerns: Raise them with your human partner before starting
5. If no concerns: Create TodoWrite and proceed

If the tracker is missing or incomplete:
- Stop immediately
- Ask for the plan to be updated before execution begins
- Do not invent a side tracker in chat or TodoWrite

### Step 2: Execute Batch
**Default: First 3 tasks**

For each task:
1. Update the Progress Tracker row to `in_progress` with current owner and a short note
2. Mirror that state in TodoWrite
3. Follow each step exactly (plan has bite-sized steps)
4. Run verifications as specified
5. Update the Progress Tracker with commit/diff and verification evidence as work progresses
6. Mark task as `completed` in the tracker, then sync TodoWrite

Use the plan's status vocabulary. If the plan does not define one, use:
- `not_started`
- `in_progress`
- `blocked`
- `completed`

### Step 3: Report
When batch complete:
- Show what was implemented
- Show verification output
- Reference the updated tracker state
- Say: "Ready for feedback."

### Step 4: Continue
Based on feedback:
- Apply changes if needed
- Update the tracker before and after any rework
- Execute next batch
- Repeat until complete

### Step 5: Complete Development

After all tasks complete and verified:
- Announce: "I'm using the finishing-a-development-branch skill to complete this work."
- **REQUIRED SUB-SKILL:** Use superpowers:finishing-a-development-branch
- Follow that skill to verify tests, present options, execute choice

## When to Stop and Ask for Help

**STOP executing immediately when:**
- Hit a blocker mid-batch (missing dependency, test fails, instruction unclear)
- Plan has critical gaps preventing starting
- You don't understand an instruction
- Verification fails repeatedly

**Ask for clarification rather than guessing.**

When blocked:
- Update the current task row to `blocked`
- Record the blocker in `Notes / Last Update` and preserve the latest `Commit / Diff` and `Verification` state
- Report the blocker to your human partner

## When to Revisit Earlier Steps

**Return to Review (Step 1) when:**
- Partner updates the plan based on your feedback
- Fundamental approach needs rethinking

**Don't force through blockers** - stop and ask.

## Remember
- Review plan critically first
- Use the plan doc's Progress Tracker as the canonical state record
- Keep `Commit / Diff` and `Verification` columns current, not just status and notes
- Follow plan steps exactly
- Don't skip verifications
- Reference skills when plan says to
- Between batches: just report and wait
- Stop when blocked, don't guess
- Update the tracker on every state change, not just at batch boundaries
- Keep TodoWrite aligned with the tracker, but resolve conflicts in favor of the tracker
- Never start implementation on main/master branch without explicit user consent

## Integration

**Required workflow skills:**
- **superpowers:using-git-worktrees** - REQUIRED: Set up isolated workspace before starting
- **superpowers:writing-plans** - Creates the plan this skill executes
- **superpowers:finishing-a-development-branch** - Complete development after all tasks
