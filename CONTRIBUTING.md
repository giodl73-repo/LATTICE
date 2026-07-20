# Contributing

Contributions should preserve LATTICE's product-neutral semantic spine and make
failure, frontier, rights, and receipt behavior explicit.

## Before opening a pull request

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings -A clippy::too-many-arguments
cargo test --workspace --locked
```

Do not add customer material, private deployment records, provider-specific
product logic, or organization-specific approval artifacts.
