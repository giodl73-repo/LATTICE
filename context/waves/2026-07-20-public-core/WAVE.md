# Public Core Wave

## Goal

Publish the product-neutral LATTICE semantic core without private deployment,
customer, funding, or approval material.

## Pulses

| Pulse | Status | Deliverable |
|---|---|---|
| 01 | complete | Extract buildable core crates and scrub organization-specific policy. |
| 02 | complete | Add public README, product plan, license, roles, and CI. |
| 03 | complete | Validate formatting, linting, tests, and publication scan. |

## Validation

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings -A clippy::too-many-arguments
cargo test --workspace --locked
git diff --check
```
