# Browser context algebra fixture

`lattice-web` creates a public synthetic eight-grain universe: three required
source/policy/receipt records followed by five optional items. All records have
public demonstration custody metadata. Optional items have source Contains
bonds. Candidate order places required records first so budgeted closure cannot
spend capacity before retaining required custody material.

Native `close_candidate_cut` closes A and B under the grain budget; native
`meet_closed` or `join_closed` combines those retained inputs under the same
budget. Deferred inputs are not resurrected when combining. The UI reports all
three stage frontiers with stage labels, while JSON retains separate frontiers.
Budgets below three return a failure and clear previous output. Grain limits
are record counts, not token limits. This bounded example does not demonstrate
arbitrary semantic dependency expansion or fetch source bytes.

Native stable cut and receipt hashes are inspectable and downloadable. URLs
contain bounded selection masks, operation, and budget. Native tests check set
counts, deterministic hashes, required-item preservation, explicit budget
failure, and input bounds. Actual-WASM browser tests check meet/join, failure
recovery, shared reload, export, mobile width, and failed engine load. Actions
runs workspace tests, lint, release WASM, a 5 MB budget and main Pages deployment.
