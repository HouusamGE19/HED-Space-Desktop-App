# Prompt Playbook

These prompt patterns are meant to make coding-agent use reproducible and reviewable rather than ad hoc.

## Prompt structure

A strong engineering prompt contains five parts:

1. **Goal** — the result required.
2. **Context** — relevant errors, screenshots, paths, or architecture.
3. **Boundaries** — what must not change and what requires approval.
4. **Verification** — the checks or acceptance scenarios that matter.
5. **Deliverable** — code, analysis, patch, review, or documentation.

## Understand an unfamiliar repository

```text
Analyze this repository without modifying files.

Explain:
1. its purpose and main entry points;
2. the major modules and how data flows between them;
3. how to build, test, and run it;
4. important configuration and generated-code boundaries;
5. the five files a new maintainer should read first.

Support the explanation with file references and flag anything you could not verify.
```

## Diagnose before fixing

```text
Diagnose the reported failure, but do not edit files.

Reproduce it if a safe local reproduction exists. Trace the responsible path,
identify the root cause with file references, and distinguish evidence from
inference. Propose the smallest safe fix and the regression test or validation
that would prove it.
```

## Implement a focused fix

```text
Fix the confirmed bug. Keep the patch focused, preserve unrelated changes and
public behavior, add or update appropriate validation, run the relevant checks,
and review the final diff.

Finish with the root cause, files changed, commands run, outcomes, and any
remaining risk.
```

## Add a feature

```text
Goal:
Implement [feature].

Context:
- Start with [relevant paths/components].
- Existing behavior that must remain: [behavior].

Boundaries:
- Keep the public behavior outside this feature unchanged.
- Do not add a production dependency without explaining the need first.
- Preserve unrelated working-tree changes.

Verification:
- Exercise [acceptance scenario].
- Run the focused source validation and any affected build/runtime checks.

Deliverable:
Implement the feature, review the final diff, and summarize files changed,
checks run, and remaining manual verification.
```

## Screenshot-driven UI iteration

```text
Use the attached screenshot as the current runtime state.

Goal:
Make [specific area] match [desired behavior/appearance].

Focus only on [component/section]. Preserve the rest of the layout and current
functionality. Inspect the relevant HTML/CSS/JS first, explain the likely cause,
then implement the smallest change. Afterward, tell me exactly what I should
verify in the running app and what screenshot would prove the result.
```

## Review generated work

```text
Review the current diff as a maintainer. Find concrete bugs or regressions.
Prioritize correctness, security/trust boundaries, broken UI behavior,
accidental scope expansion, and missing validation. Cite affected files and
explain the failure scenario. Do not modify files yet.
```

## Correct an over-broad patch

```text
Reduce the patch to the smallest change required by the acceptance criteria.
Remove unrelated formatting, renaming, cleanup, and abstraction changes.
Preserve the actual fix and re-run the affected checks.
```

## Parallel review when useful

```text
Review this change using independent perspectives:
- correctness and edge cases;
- security and trust boundaries;
- tests/validation and maintainability.

Consolidate only concrete findings with file references. Do not modify files.
```

Use parallel agents only when the work is genuinely independent; avoid having several agents edit the same files.
