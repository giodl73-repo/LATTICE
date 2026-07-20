# lattice-core Spec

## Purpose

`lattice-core` is the stable public API facade for the LATTICE workspace. It
re-exports core model types and owns the enterprise dependency policy that every
crate must obey.

It is also the compatibility boundary: downstream callers should see a stable
semantic spine, while source adapters, closure rules, budget profiles, and
prompt/pack profiles evolve behind reviewed contracts.

## Phase

Bottom, then carried through every later phase as the compatibility boundary.

## Owns

- Workspace-level dependency policy.
- Stable re-exports for ids, grains, bonds, cuts, and closure receipts.
- Cross-crate constants that must be shared without creating dependency cycles.

## Does not own

- Lattice semantics such as meet, join, closure, or Hasse projections.
- Store implementation.
- Registry persistence.
- METIS-CORE calls.
- CLI parsing or command execution.

## Public contract

| Item | Contract |
|---|---|
| `ALLOWED_EXTERNAL_UPSTREAM` | The only external upstream allowed by the public core policy: `https://github.com/giodl73-repo/METIS-CORE`. |
| `DependencyPolicy` | Declares `product_neutral` and the allowed external upstream list. |
| `dependency_policy()` | Returns the default policy used by validation and CLI status. |
| Re-exported model types | Preserve source compatibility for common LATTICE consumers. |

## Invariants

- `product_neutral` defaults to `true`.
- METIS-CORE is the only external upstream in the default policy.
- No crate-specific business logic belongs here.
- No external dependency may be added here unless the enterprise boundary is
  deliberately changed.
- Safe Rust is the baseline: public crates should preserve `unsafe_code =
  "forbid"` coverage through workspace lints or crate attributes.
- Re-exports should favor the stable v1 IR spine and avoid exposing experimental
  extension internals before they have validation gates.

## Extensibility contract

`lattice-core` may expose stable identifiers, policy facts, and mature model
types. It should not expose plugin registries, source-adapter internals, or
experimental pass machinery until those surfaces have L1/L2 validation and a
phase review.

## Safety contract

`lattice-core` exposes policy facts, not policy guesses. Callers should be able
to ask the crate whether an upstream is allowed and receive a deterministic
answer. This is dependency safety, not semantic proof; semantic proof belongs in
model/order/store receipts and validation.

## Validation

```powershell
cargo test -p lattice-core --quiet
```
