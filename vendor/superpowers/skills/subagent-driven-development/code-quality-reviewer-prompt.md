# Code Quality Reviewer Prompt Template

Use this template when dispatching a code quality reviewer subagent.

**Purpose:** Verify implementation is well-built (clean, tested, maintainable)

**Only dispatch after spec compliance review passes.**

```
Task tool (superpowers:code-reviewer):
  Use template at requesting-code-review/code-reviewer.md

  WHAT_WAS_IMPLEMENTED: [from implementer's report]
  PLAN_OR_REQUIREMENTS: Task N from [plan-file]
  BASE_SHA: [commit before task]
  HEAD_SHA: [implementer commit or pinned diff head]
  DESCRIPTION: [task summary]
  OWNED_PATHS: [exact files/modules for this task]
  TRACKER_ROW: [current row from the Progress Tracker]
```

**Code reviewer returns:** Recommended status transition, review target commit/diff, Strengths, Issues (Critical/Important/Minor), Assessment
