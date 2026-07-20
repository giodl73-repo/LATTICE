# lattice-order Spec

## Purpose

`lattice-order` owns order and closure semantics over closed cuts. It is the
crate that makes LATTICE more than a graph: it defines how selected context can
be compared, intersected, joined, and explained.

This crate is the guarded middle of the architecture. It should be extensible by
named closure/order rules and pass builders, not by source-specific shortcuts or
new unreviewed IR layers.

Budget-aware operator behavior is specified in `docs/budgeting.md`.
Pass-pipeline invariants and effects are specified in `docs/pass-pipeline.md`.
R3 requires pass invariant/effect substrate types before broad operator
implementation.

## Phase

Closure, Meet, Join, and Hasse.

## Owns

- Closure-rule execution and receipts.
- Candidate-cut closure boundaries and budget-aware closure outcomes.
- `meet` over closed cuts.
- `join` over closed cuts.
- `simplify` over closed cuts.
- Future `leq` and Hasse/debug projections.
- Canonicalization rules for ordered context.
- Closed-cut relationship diagnostics for explaining candidate Hasse edges.

## Does not own

- Raw graph traversal.
- METIS-CORE partitioning.
- Store persistence.
- CLI command parsing.
- Source registry ownership.

## Semantic contracts

| Operator | Contract |
|---|---|
| `leq(A, B)` | Planned: A is no broader than B after both cuts are closed under the same policy. |
| `meet(A, B, budget)` | Shared specific context: intersect selected grains/bonds, close the result, then return a bounded closed cut, bounded closed cut with frontier, or budget failure receipt. |
| `join(A, B, budget)` | Least broader context: union selected grains/bonds, close the result, then return a bounded closed cut, bounded closed cut with frontier, or budget failure receipt. |
| `simplify(A, budget)` | Minimal sufficient closed context: remove redundant optional material, re-close, and frontier any removed context with reasons. |
| Hasse projection | Planned: explain the immediate order edges between cuts after transitive reduction. |

Closure starts with `CandidateCut`, validates required custody/policy/receipt
material, removes invalid bonds, records frontier entries for deferred optional
material, and returns a closed cut, frontiered closed cut, or budget failure.
Meet, join, and simplify construct candidates from already closed cuts, then
reuse the same closure/budget outcome path before returning operator receipts.

## Invariants

- Meet and join always return closed cuts with closure receipts.
- Simplify always consumes and returns closed cuts with closure receipts.
- Simplify must not remove source pointers, policy grains, receipt grains, or
  required bonds.
- Simplify should preserve a bounded semantic decision spine, including required,
  contradiction, derivation, and representative citation/entity bonds; removed
  optional material is frontiered as redundant context.
- Meet and join must not silently exceed context budgets.
- Required closure material cannot be dropped to fit a prompt; if it does not fit,
  the operator returns a budget failure receipt.
- Optional material excluded by budget pressure must be recorded in a frontier.
- Retained bonds must have endpoints in the returned cut.
- Graph edges are not order edges unless explicitly projected.
- Closure must be monotone: adding input context must not remove required closure
  context except by an explicit policy change.

## Correctness and safety contract

Rust ensures memory safety for the implementation, not mathematical correctness.
`lattice-order` must prove its semantics with tests and receipts:

- meet is an intersection followed by closure,
- join is a union followed by closure,
- simplify is a semantics-preserving reduction followed by closure,
- budget pressure is checked after candidate construction and after closure,
- bounded results include budget counters and frontier counts,
- every result carries a closure receipt,
- retained bonds have valid endpoints,
- future `leq` enables property tests for idempotence, commutativity,
  associativity where applicable, and absorption,
- commands must not export unclosed candidates as final AI context.

## Extensibility contract

Allowed v1 growth:

- named closure rules with receipts,
- budget-aware operator outcome types,
- candidate-cut closure outcome types for Tiny fixtures,
- closed-cut-only meet/join helpers for Tiny fixtures,
- closed-cut-only simplify helpers for Tiny fixtures,
- Hasse/debug projections over already closed cuts,
- deterministic token/count estimators for fixtures,
- pass builders that establish the invariant catalog.

Rejected for v1:

- source-family-specific closure behavior after ingest,
- graph partition output treated as final context,
- meet/join implementations that skip closure or budget checks,
- arbitrary plugin passes that can emit prompt/pack output,
- hidden order semantics attached directly to `BondKind`.

## Validation

```powershell
cargo test -p lattice-order --quiet
```
