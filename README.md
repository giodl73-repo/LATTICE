# LATTICE

**Auditable context algebra for AI systems.**

LATTICE models context as typed grains, bonds, closed cuts, frontiers, budgets,
packs, and receipts. It provides deterministic primitives for selecting,
combining, explaining, and replaying the context supplied to an AI system.

This public edition contains the product-neutral semantic core. It does not
include private deployment plans, customer material, approval records, or
organization-specific integrations.

## Crates

| Crate | Role |
|---|---|
| `lattice-model` | Core grains, bonds, cuts, budgets, frontiers, and fixture types. |
| `lattice-core` | Public dependency policy and shared model exports. |
| `lattice-order` | Closure, meet, join, order, and diagnostic operations. |
| `lattice-registry` | Source-pointer and deterministic registry contracts. |
| `lattice-store` | Local persistence and transaction-safe materialization. |
| `lattice-crop` | Optional graph partitioning boundary through METIS-CORE. |

## Design principles

- Context decisions should be deterministic and receipt-backed.
- Required closure material must not be silently dropped to fit a budget.
- Source content remains with its owner; LATTICE stores pointers and derived
  semantic records.
- Product and provider integrations belong at the edges, not in the core model.
- The only external graph dependency is
  [METIS-CORE](https://github.com/giodl73-repo/METIS-CORE).

## Development

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings -A clippy::too-many-arguments
cargo test --workspace
```

## Status

This is an early public core extracted from a larger incubation codebase. The
semantic model and deterministic fixture suites are implemented; stable package
APIs and crates.io publication are not yet promised.

## License

MIT. See [`LICENSE`](LICENSE).
