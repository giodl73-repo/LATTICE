# LATTICE Review Panel

Public changes use four active lenses.

| Role | Protects | Invoke when |
|---|---|---|
| [Semantic Algebra Reviewer](parliament/semantic-algebra-reviewer.md) | Closure and algebra laws | Changing grains, bonds, cuts, meet, join, or order |
| [Evidence Custody Reviewer](parliament/evidence-custody-reviewer.md) | Provenance, pointers, and receipts | Changing registries, stores, packs, or source handling |
| [Budget Safety Reviewer](parliament/budget-safety-reviewer.md) | Explicit failure instead of silent context loss | Changing budgets, frontiers, crop, or diagnostics |
| [API Stability Reviewer](stakeholders/api-stability-reviewer.md) | Neutral family contracts | Changing public crates, schemas, dependencies, or compatibility |

## Core Tensions

| Pulls | Against | Because |
|---|---|---|
| Semantic Algebra Reviewer | Budget Safety Reviewer | A mathematically closed cut may exceed the operator's declared budget. |
| Evidence Custody Reviewer | API Stability Reviewer | Stronger receipts can enlarge persisted and public contracts. |
| Budget Safety Reviewer | API Stability Reviewer | Safer explicit failures can break consumers that relied on permissive truncation. |
| Semantic Algebra Reviewer | API Stability Reviewer | Cleaner algebra can require incompatible model changes. |

## Review Order

1. Evidence Custody Reviewer establishes source ownership.
2. Semantic Algebra Reviewer proves closure laws.
3. Budget Safety Reviewer proves bounded success or structured failure.
4. API Stability Reviewer evaluates migration and family compatibility.
