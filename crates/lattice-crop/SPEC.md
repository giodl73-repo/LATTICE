# lattice-crop Spec

## Purpose

`lattice-crop` owns cut and partition helpers for LATTICE. It is the only crate
allowed to depend on METIS-CORE.

This is an edge helper, not part of the final-context spine. Its outputs are
candidates that must flow through candidate cuts, closure, budget checks,
frontier recording, and closed-cut verification before any AI harness can use
them.

## Phase

Hasse and Complete Lattice for large-graph partition experiments; Bottom includes
only the adapter smoke test.

## Owns

- METIS-CORE adapter boundary.
- Candidate partition helpers for large context graphs.
- Future cut-frontier diagnostics that need graph partitioning.
- Candidate-only partition diagnostics that must be closed by `lattice-order`
  before AI use.

## Does not own

- Lattice order semantics.
- Closure policy.
- Rights or source-custody policy.
- Default context selection.
- Any external dependency other than METIS-CORE.

## Dependency rule

`lattice-crop` may depend on:

```text
https://github.com/giodl73-repo/METIS-CORE
```

It must not depend on RLINE, CROP, SLICE, FLETCH, PEBBLE, PROOF, or source-corpus
repos.

## Invariants

- Partitioning output is candidate structure, not proof of semantic relevance.
- METIS-CORE must not own LATTICE context semantics.
- Calls to METIS-CORE stay isolated in this crate.
- Context cuts must still be closed by `lattice-order` before they are treated as
  LATTICE context.

## Extensibility contract

Allowed v1 growth:

- additional METIS-backed candidate partition experiments,
- Hasse/debug projection helpers over closed cuts,
- frontier-pressure diagnostics that explain candidate structure.
- candidate-only metadata that explicitly rejects final-context interpretation.

Rejected for v1:

- direct prompt/pack output,
- closure or rights-policy behavior,
- dependency expansion beyond METIS-CORE,
- treating graph partitions as lattice order or final context.

## Validation

```powershell
cargo test -p lattice-crop --quiet
```
