# LATTICE Invariants

## LAT-PUB-I-01: Required Bonds Close Deterministically

**Status:** VERIFIED

**Invariant:** Required bonds close deterministically, and order-dependent
results are review blockers.

**Why it matters:** Context replay and consumer compatibility break if required
semantic closure depends on incidental ordering.

**Test:** Workspace tests and closure proof tests cover accepted and rejected
algebra cases.

**Evidence:** `.roles/parliament/semantic-algebra-reviewer.md` and
`cargo test --workspace --locked`.

## LAT-PUB-I-02: Rejected Closure Is Not Silent Truncation

**Status:** VERIFIED

**Invariant:** If required closure exceeds a budget, the result is structured
failure or frontier evidence rather than a success receipt.

**Why it matters:** Silent truncation lets downstream systems trust context that
was never actually closed.

**Test:** `cargo test -p lattice-order --test closure_proof` retains accepted
and over-budget evidence.

**Evidence:** README retained closure proof and
`.roles/parliament/budget-safety-reviewer.md`.

## LAT-PUB-I-03: Registry Stores Pointers, Not Source Custody Transfers

**Status:** VERIFIED

**Invariant:** Registry and store records may preserve deterministic pointers
and receipts, but they must not become source-content custody.

**Why it matters:** Public-source, private-source, and customer-source
boundaries collapse if LATTICE becomes a second system of record.

**Test:** Registry/store changes require Evidence Custody review and public
promotion boundary checks.

**Evidence:** `docs/MAINTENANCE.md` and
`.roles/parliament/evidence-custody-reviewer.md`.

## LAT-PUB-I-04: METIS-CORE Is A Partition Boundary

**Status:** VERIFIED

**Invariant:** METIS-CORE may support optional graph partitioning through
`lattice-crop`, but it is not semantic authority for closure.

**Why it matters:** Partitioning is useful optimization evidence, but closure
truth must remain in LATTICE's algebra and receipts.

**Test:** Dependency and crop changes require Budget Safety and API Stability
review.

**Evidence:** README design principles and
`.roles/parliament/budget-safety-reviewer.md`.

## LAT-PUB-I-05: Public Compatibility Claims Need Migration Evidence

**Status:** VERIFIED

**Invariant:** FLETCH, MDCROP, WITNESS, or future-consumer compatibility claims
need fixtures and migration evidence.

**Why it matters:** Family diagrams become unsafe public API promises when they
are not backed by rehearsed compatibility evidence.

**Test:** Public API, schema, dependency, or compatibility changes invoke API
Stability review.

**Evidence:** `.roles/stakeholders/api-stability-reviewer.md` and
`context/waves/2026-07-20-public-core/WAVE.md`.
