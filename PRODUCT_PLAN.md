# LATTICE Product Plan

## Thesis

AI systems need a context layer that is more precise and auditable than an
unexplained top-k retrieval result. LATTICE provides ordered, proof-aware
context with explicit closure, meet, join, budget, frontier, and receipt
contracts.

## Public scope

The public repository owns product-neutral semantic types and deterministic
operations. Provider adapters, customer deployments, approval custody, and
organization-specific integration plans remain outside this repository.

## Initial consumers

- AI harnesses that need replayable context decisions.
- Corpus tools that need provenance-aware context packs.
- Evaluation systems that compare context selection and closure behavior.
- Local applications that need deterministic context materialization.

## Non-goals

- No vector database or document renderer.
- No model-provider orchestration.
- No customer data or deployment approval records.
- No opaque truncation of closure-required material.
- No product-to-product dependency shortcuts.

## Near-term work

1. Stabilize the public model and operator APIs.
2. Add concise executable examples for closure, meet, join, and budget failure.
3. Publish versioned JSON contracts for packs, frontiers, and receipts.
4. Benchmark deterministic fixture tiers and document interpretation limits.
5. Evaluate crates.io publication after API review.
