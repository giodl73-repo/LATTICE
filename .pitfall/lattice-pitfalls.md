# LATTICE Pitfalls

## LAT-PUB-PF-01: Closed Cut Becomes Retrieval Quality Claim

**Status:** MITIGATED

**Pattern:** Deterministic closure is described as answer quality, model
performance, or retrieval superiority.

**Domain:** Public docs, examples, benchmark copy, and consumer-facing claims.

**Detection difficulty:** Closed context can make an answer look grounded even
when the underlying selection quality is outside LATTICE's scope.

**Structural solution:** Keep LATTICE framed as ordered, proof-aware context
with receipts and explicit public-scope limits.

**Evidence:** `PRODUCT_PLAN.md`, `README.md`, and
`.roles/stakeholders/api-stability-reviewer.md`.

## LAT-PUB-PF-02: Budget Failure Is Softened Into Success

**Status:** MITIGATED

**Pattern:** Over-budget required context still emits a success receipt or a
closed-cut label.

**Domain:** Budgets, frontiers, receipts, closure tests, and diagnostics.

**Detection difficulty:** Missing required context is often invisible unless the
receipt, frontier, or failure reason is inspected.

**Structural solution:** Preserve structured budget failure and block success
receipts for unclosed cuts.

**Evidence:** README retained closure proof and
`.roles/parliament/budget-safety-reviewer.md`.

## LAT-PUB-PF-03: Source Pointers Become Source Ownership

**Status:** MITIGATED

**Pattern:** Registry or store convenience turns pointers into copied source
bytes, private evidence, or a second system of record.

**Domain:** Registry, store, packs, receipts, promotion review, and fixtures.

**Detection difficulty:** Copied fixture content can look harmless until public
promotion, rights, or refresh review exposes custody drift.

**Structural solution:** Keep source content owner-held and represent public
derivations as deterministic pointers, grains, bonds, packs, and receipts.

**Evidence:** `docs/MAINTENANCE.md` and
`.roles/parliament/evidence-custody-reviewer.md`.

## LAT-PUB-PF-04: Public Core Imports Private Deployment Promises

**Status:** MITIGATED

**Pattern:** Public docs, schemas, fixtures, or examples imply customer,
provider, approval, funding, or organization-specific readiness.

**Domain:** Public-core promotion, README/product docs, schemas, examples, and
release notes.

**Detection difficulty:** Private-incubation language can look like ordinary
product maturity language unless promotion boundaries are checked.

**Structural solution:** Promote only reviewed, product-neutral changes with
public tests and documentation.

**Evidence:** `docs/MAINTENANCE.md` and `PRODUCT_PLAN.md`.

## LAT-PUB-PF-05: Family Compatibility Is Claimed Before Rehearsal

**Status:** MITIGATED

**Pattern:** LATTICE claims FLETCH, MDCROP, WITNESS, or future-consumer
compatibility without fixtures, migration evidence, or boundary review.

**Domain:** Public API, schemas, crate responsibilities, dependency updates, and
family documentation.

**Detection difficulty:** The family diagram is easy to overread as a stable
contract rather than a responsibility map.

**Structural solution:** API Stability review must accompany public API,
schema, dependency, or compatibility changes.

**Evidence:** README family table and
`.roles/stakeholders/api-stability-reviewer.md`.
