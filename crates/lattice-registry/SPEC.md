# lattice-registry Spec

## Purpose

`lattice-registry` owns source/corpus pointers without vendoring content. It lets
LATTICE know where content lives, how it was acquired or referenced, where
proof-like backfill records live, and what rights policy applies.

Registry is an edge extension point. New source families may register new
pointer dialects, but they must lower into the same `SourcePointer` contract and
must not create source-specific closure or order semantics.

## Phase

Generators and Closure.

## Owns

- Source pointer records.
- Owner-repo identity.
- Owner work ids.
- Fetch-like registry paths and fletch ids.
- Proof-like ledger and record references.
- Rights policy fields.
- Rights boundary notes from the owning source repo.
- Future refresh/check status metadata.
- Generator-stage `IngestRecord` rows that summarize read, skipped, normalized,
  or rejected source observations before model/order semantics.
- Generator pass reports for the canonical `ResolveRegistry -> NormalizeIngest
  -> ValidateBonds -> EmitIngestReceipt` pipeline.

## Does not own

- Source content.
- Fetch execution.
- Proof execution.
- Pack rendering.
- Store persistence.

## Registry row contract

| Field | Meaning |
|---|---|
| `source_id` | Stable source/corpus id. Prefer the owning repo's work id when it is already stable. |
| `owner_repo` | Repo that owns source custody. |
| `work_id` | Owning repo's work-level custody id. |
| `fletch_registry_path` | Path to the owning repo's FLETCH registry file. This is a pointer, not a FLETCH dependency. |
| `fletch_id` | Fletch id inside that registry file. |
| `proof_ledger_path` | Path to the owning repo's proof/source ledger. This is a pointer, not a PROOF dependency. |
| `proof_record_path` | Path to the owning repo's proof source record. |
| `rights_policy` | Policy label that closure/export logic must preserve. |
| `rights_boundary` | Human-readable boundary note that closure and prompt output must preserve or cite. |

Generator pulses may lower valid source pointers into `IngestRecord` values with
typed outcomes, ingest receipts, generated grains/bonds, and summary counters.
Rejected or skipped records must keep audit receipts and must not look like
successful normalized ingest.

The first executable generators are deterministic and synthetic: they validate a
`SourcePointer`, lower it into Tiny, Small, launch-readiness, or launch-scale
grains/bonds when custody fields are present, validate generated bond endpoints,
and emit an ingest receipt. They do not fetch, parse, or vendor source content.
Medium remains under the stress gate; Large is represented by an explicit
pre-materialization stress contract until full materialization is a Complete
Lattice gate.

Concrete FONTES example:

| Field | Value |
|---|---|
| `source_id` | `fontes:apache-calcite:query-planning` |
| `owner_repo` | `giodl73-repo/FONTES` |
| `work_id` | `fontes:apache-calcite:query-planning` |
| `fletch_registry_path` | `.fletch\registries\fontes-apache-calcite-query-planning-surfaces.json` |
| `fletch_id` | `fontes.apache-calcite.lattice` |
| `proof_ledger_path` | `sources\tables\proof-source-ledger.json` |
| `proof_record_path` | `.proof\sources\fontes-course-source-ledger.source.md` |
| `rights_policy` | `derived_text_allowed` |
| `rights_boundary` | Apache Calcite documentation text is mapped for derived text; source files, releases, generated docs, examples, tests, dependencies, logos, and binary artifacts remain boundary-checked. |

## Invariants

- Registry entries point to content; they do not copy content.
- Rights policy must survive cuts, meet, join, store, and export surfaces.
- Pointer vocabulary may resemble public portfolio systems, but this crate must
  not depend on those systems.

## Extensibility contract

Allowed v1 growth:

- source adapter metadata fields that preserve owner custody,
- stricter rights-policy labels,
- refresh/check status fields,
- local validation helpers for pointer shape and required custody fields.
- deterministic Tiny, Small, launch-readiness, and launch-scale fixture lowering
  into ingest records, grains, bonds, and receipts without executing external
  source tools.
- typed generator pass reports that make pre-closure ingest effects auditable.

Rejected for v1:

- executing FLETCH or PROOF from registry code,
- vendoring source content,
- importing source-corpus crates,
- registry entries that bypass `IngestRecord`, grain, bond, receipt, and policy
  lowering.

## Validation

```powershell
cargo test -p lattice-registry --quiet
```
