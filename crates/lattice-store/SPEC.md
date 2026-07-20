# lattice-store Spec

## Purpose

`lattice-store` owns the local-first storage boundary for LATTICE. The Bottom
phase starts with an in-memory store; later phases may add SQLite-backed
materializations without changing the core model semantics.

## Phase

Bottom for the memory store; Complete Lattice for durable materializations.

## Owns

- Repository-local storage traits and implementations.
- Cut lookup and persistence boundaries.
- Rebuildable pack materialization from closed cuts.
- Future schema/migration contracts.
- Rebuildable materialized views and receipts.

## Does not own

- Source content custody.
- Registry pointer semantics.
- Meet/join algorithms.
- Proof or pack rendering.
- External database services.

## Store posture

The store is a materialization boundary, not the source of truth. LATTICE source
truth remains in registered source pointers, proof-like receipts, and closed-cut
records that can be rebuilt.

Storage is an edge extension point. Durable backends may improve persistence and
query speed, but they must not change IR semantics or become a hidden source of
truth.

## Invariants

- Store APIs must preserve cut ids exactly.
- Pack materializations must remain derivable from closed cuts and profile ids.
- Materialized rows must be rebuildable from model/registry/receipt inputs.
- The crate must not introduce hosted-service requirements.
- SQLite, when added, must remain local-first and inspectable.

## Extensibility contract

Allowed v1 growth:

- in-memory fixtures and report stores,
- in-memory pack materialization for Tiny demos,
- local SQLite materializations after L2 semantic gates,
- rebuildable indexes over source pointers, grains, bonds, cuts, receipts, and
  packs.

Rejected for v1:

- hosted database requirements,
- store-specific closure or order semantics,
- irreversible materializations that cannot be rebuilt from registry/model/order
  inputs and receipts,
- storage APIs that emit final AI context without closed-cut verification.

## Validation

```powershell
cargo test -p lattice-store --quiet
```
