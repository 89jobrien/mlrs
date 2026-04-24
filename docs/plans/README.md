# mlrs Feature Plans

Ordered by priority. Plans 01–03 are correctness gaps; 04–08 are feature gaps; 09–12 are
architectural improvements.

| #   | Plan                                                           | Priority | Status |
| --- | -------------------------------------------------------------- | -------- | ------ |
| 01  | [Context Window Management](01-context-window-management.md)   | high     | open   |
| 02  | [Streaming Output](02-streaming-output.md)                     | high     | open   |
| 03  | [Cancellation](03-cancellation.md)                             | high     | open   |
| 04  | [Structured Step Events](04-structured-step-events.md)         | medium   | open   |
| 05  | [Final Answer Validation](05-final-answer-validation.md)       | medium   | open   |
| 06  | [Notebook Persistence](06-notebook-persistence.md)             | medium   | open   |
| 07  | [Per-Cell Output Truncation](07-per-cell-output-truncation.md) | medium   | open   |
| 08  | [Register rlm_call](08-rlm-call-registration.md)               | medium   | open   |
| 09  | [Provider Retry / Backoff](09-provider-retry-backoff.md)       | low      | open   |
| 10  | [Reflection Step](10-reflection-step.md)                       | low      | open   |
| 11  | [Planning Phase](11-planning-phase.md)                         | low      | open   |
| 12  | [Parallel Tool Execution](12-parallel-tool-execution.md)       | low      | open   |

## Suggested implementation order

```text
03 (cancellation) → 07 (truncation) → 01 (compaction) → 04 (events)
→ 02 (streaming) → 08 (rlm_call) → 05 (validation) → 06 (persistence)
→ 09 (retry) → 10 (reflection) → 11 (planning) → 12 (parallel)
```

Start with 03 — it's low-effort, high-safety, and unblocks clean testing of everything
else. Then 07 (prevents context blowup during development). Then 01 (full compaction).
