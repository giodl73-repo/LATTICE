# lattice-model Spec

## Purpose

`lattice-model` defines the product-neutral data types that generate LATTICE
elements: grains, bonds, cuts, and closure receipts.

The model follows the IR ladder in `docs/ir-ladder.md`.
That ladder is the v1 semantic spine; model work should encode these layers as
structured types before introducing additional IR concepts.

## Phase

Generators first; Closure, Meet, Join, Hasse, and Complete Lattice build on these
types.

## Owns

- Stable identifiers for grains and cuts.
- Grain records and labels.
- Typed bonds between grains.
- Planned context budgets and frontiers.
- Closed-cut containers.
- Closure receipt records.
- Dependency-free pack, prompt-frame, and PRESS-frame contract shapes derived
  from closed cuts.
- Built-in measurement records for boundary shape, candidate quality, and
  closed-cut counters.

## Does not own

- Order comparison.
- Meet/join algorithms.
- Store persistence.
- Source registry rows.
- Partitioning or graph algorithms.

## Core concepts

| Concept | Meaning |
|---|---|
| `GrainId` | Stable id for the smallest addressable context element. |
| `CutId` | Stable id for a named cut/view/materialized context selection. |
| `BondKind` | Typed relationship vocabulary, not automatically an order relation. |
| `Bond` | Directed relationship between two grains. |
| `GrainKind` | Generator-stage classification for source pointer, context, evidence, policy, and receipt grains. |
| `ClosedCut` | A cut after closure rules have added required grains, bonds, and receipts. |
| `ClosureReceipt` | Explanation of a closure rule or validation step. |
| `ContextBudget` | Planned limits for tokens, grains, bonds, closure expansion, receipts, and output bytes. |
| `BudgetStatus` | Planned outcome status: within budget, frontier deferred, or budget failure. |
| `Frontier` | Planned excluded or deferred grains/bonds with reason codes when a budget or policy boundary is reached. |
| `FrontierReason` | Planned reason code for why context was excluded, deferred, or rejected. |
| `ContextPack` | Portable closed-cut summary with profile id, policy metadata, and receipt counts. |
| `PromptFrame` | Prompt-facing frame metadata over a closed cut; not rendered prompt prose. |
| `PressPublicationFrame` | File/contract handoff shape for future PRESS rendering, without a PRESS dependency. |
| `BoundaryMetrics` | CROP-style boundary/conductance counters over grains and typed bonds. |
| `CandidateQualityMetrics` | Precision, recall, distractor, closure-rescue, and frontier false-negative counters. |
| `ContextMetrics` | Elapsed-time, count, receipt, output, and budget-use counters for closed cuts. |

## Invariants

- Bond endpoints must be present in a closed cut before the bond is retained.
- Bond kind does not imply lattice order unless an order projection explicitly
  interprets it.
- Closure receipts explain why a cut is valid; they are not decorative logs.
- Model types must remain free of external crate dependencies unless explicitly
  approved by the dependency gate.

## Correctness and safety contract

Rust type safety prevents many representation mistakes, but the model crate must
still encode semantic invariants explicitly:

- ids are stable handles, not display labels,
- bonds require evidence before trusted use,
- closed cuts must validate bond endpoints,
- receipts are required for closure or policy-affecting transformations,
- pack/prompt/PRESS frames must be derived from closed cuts and carry receipt
  counts,
- context budgets are semantic inputs and must be recorded in receipts when they
  affect selection,
- frontiers preserve evidence about what was excluded or deferred instead of
  silently dropping context,
- budget/frontier statuses should be enums or structured values, not free-form
  display strings,
- rights/custody fields must not be discarded by later crates.
- metrics must not turn graph shape into proof of final AI context; closure still
  gates final use.

## Extensibility contract

`lattice-model` should be conservative. Add fields and enums that make the
existing spine more explicit before adding new layers.

Allowed v1 growth:

- structured budget/frontier/outcome types,
- generator-stage grain metadata and deterministic Tiny fixture helpers,
- controlled bond-kind additions,
- receipt fields for rule ids, policy ids, hashes, and custody pointers,
- prompt/pack profile ids that reference closed cuts without rendering them.
- PRESS publication frame contracts that preserve rights and closure metadata
  without rendering documents.
- CROP-inspired metrics implemented locally over LATTICE grains, bonds, closed
  cuts, budgets, and frontiers.

Deferred until phase review:

- user-defined IR layers,
- source-family-specific grain or cut structs,
- trait-object op hierarchies,
- order semantics hidden inside bond kinds,
- prompt text as a model-layer representation.
- direct PRESS, PROOF, DOCX, PPTX, PDF, or site rendering.
- direct dependencies on CROP/RGRAPH/ROPT/RSTAT metrics crates.

## Validation

```powershell
cargo test -p lattice-model --quiet
```
