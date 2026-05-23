# Chat SDK Goal Refinements

This is an append-only log of refinements to the chat-sdk Codex `/goal` brief
([`scripts/codex-goal-chat/port-chat-sdk.md`](../../scripts/codex-goal-chat/port-chat-sdk.md))
and condition file
([`scripts/codex-goal-chat/goal-condition.md`](../../scripts/codex-goal-chat/goal-condition.md)).

The brief mandates a refinement pass after every 5 successful merge-back
cycles. Each entry below should capture:

1. **Slices covered** — the slice numbers (or commit SHA range) reviewed.
2. **What the brief got wrong or left out** — concrete upstream facts that
   contradict, refine, or extend the current brief.
3. **Stale or misleading guidance** — sections of the brief that should be
   tightened, removed, or reordered.
4. **Edits applied** — the exact brief/condition changes landed alongside this
   entry.
5. **Open refinements deferred** — items spotted but not yet folded in, with a
   rationale for deferring.

## Entry template

```
### YYYY-MM-DD — slices N..N+5

**What the brief got wrong or left out**
- ...

**Stale or misleading guidance**
- ...

**Edits applied**
- `scripts/codex-goal-chat/port-chat-sdk.md`: ...
- `scripts/codex-goal-chat/goal-condition.md`: ...

**Open refinements deferred**
- ...
```

## Entries

_(first entry to be written after the first 5 merge-back cycles complete)_
