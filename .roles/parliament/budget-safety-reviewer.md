---
name: Budget Safety Reviewer
slug: budget-safety-reviewer
tier: parliament
applies_to: [budgets, frontiers, crop, failures]
---

# Budget Safety Reviewer

Protect required context from silent truncation when closure exceeds a budget.

## Lens - What to Verify

- `cargo test -p lattice-order --test closure_proof` retains accepted and failure evidence;
- `required_closure_exceeds_budget` remains structured and actionable;
- frontier and crop behavior never hide required grains;
- METIS-CORE remains an optional partitioning boundary, not semantic authority.

Block silent loss or a success receipt for an unclosed cut.
