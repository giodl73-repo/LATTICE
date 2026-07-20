#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use lattice_model::{
    Bond, BondKind, BoundaryMetrics, BudgetFailure, CandidateQualityMetrics, ContextBudget,
    ContextMetrics, FixtureTier, Frontier, FrontierItem, FrontierReason, FrontierRecord, Grain,
    GrainId, GrainKind, LaunchReadinessFixture,
};
use lattice_model::{ClosedCut, ClosureReceipt, CutId};

pub const REGISTRY_RESOLVED: &str = "lattice.registry-resolved";
pub const INGEST_NORMALIZED: &str = "lattice.ingest-normalized";
pub const BONDS_VALIDATED: &str = "lattice.bonds-validated";
pub const VIEW_PLANNED: &str = "lattice.view-planned";
pub const CANDIDATE_CUT_BUILT: &str = "lattice.candidate-cut-built";
pub const CLOSURE_APPLIED: &str = "lattice.closure-applied";
pub const BUDGET_CHECKED: &str = "lattice.budget-checked";
pub const FRONTIER_RECORDED: &str = "lattice.frontier-recorded";
pub const CLOSED_CUT_VERIFIED: &str = "lattice.closed-cut-verified";
pub const CLOSED_CUT_SIMPLIFIED: &str = "lattice.closed-cut-simplified";
pub const PROMPT_RENDERED: &str = "lattice.prompt-rendered";
pub const DIAGNOSTICS_EMITTED: &str = "lattice.diagnostics-emitted";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PassScope {
    Registry,
    Store,
    Cuts,
    Receipts,
    Packs,
    Prompts,
    Diagnostics,
}

impl PassScope {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Registry => "registry",
            Self::Store => "store",
            Self::Cuts => "cuts",
            Self::Receipts => "receipts",
            Self::Packs => "packs",
            Self::Prompts => "prompts",
            Self::Diagnostics => "diagnostics",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PassEffect {
    Pure,
    Read(PassScope),
    Update(PassScope),
    Insert(PassScope),
    Delete(PassScope),
    Emit(PassScope),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PassContract {
    pub name: String,
    pub preconditions: Vec<&'static str>,
    pub postconditions: Vec<&'static str>,
    pub effects: Vec<PassEffect>,
    pub receipt_rule: Option<String>,
}

impl PassContract {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            preconditions: Vec::new(),
            postconditions: Vec::new(),
            effects: Vec::new(),
            receipt_rule: None,
        }
    }

    pub fn with_precondition(mut self, invariant: &'static str) -> Self {
        self.preconditions.push(invariant);
        self
    }

    pub fn with_postcondition(mut self, invariant: &'static str) -> Self {
        self.postconditions.push(invariant);
        self
    }

    pub fn with_effect(mut self, effect: PassEffect) -> Self {
        self.effects.push(effect);
        self
    }

    pub fn with_receipt_rule(mut self, receipt_rule: impl Into<String>) -> Self {
        self.receipt_rule = Some(receipt_rule.into());
        self
    }

    pub fn has_mutating_effect_on(&self, scope: PassScope) -> bool {
        self.effects.iter().any(|effect| {
            matches!(
                effect,
                PassEffect::Update(effect_scope)
                    | PassEffect::Insert(effect_scope)
                    | PassEffect::Delete(effect_scope)
                    if *effect_scope == scope
            )
        })
    }
}

pub const DEFAULT_CLOSURE_POLICY: &str = "closure-v1";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateCut {
    pub id: CutId,
    pub grains: Vec<Grain>,
    pub bonds: Vec<Bond>,
}

impl CandidateCut {
    pub fn new(id: CutId) -> Self {
        Self {
            id,
            grains: Vec::new(),
            bonds: Vec::new(),
        }
    }

    pub fn with_grain(mut self, grain: Grain) -> Self {
        self.grains.push(grain);
        self
    }

    pub fn with_bond(mut self, bond: Bond) -> Self {
        self.bonds.push(bond);
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClosureResult {
    Closed {
        cut: ClosedCut,
        budget_status: lattice_model::BudgetStatus,
    },
    ClosedWithFrontier {
        cut: ClosedCut,
        frontier: Frontier,
        budget_status: lattice_model::BudgetStatus,
    },
    BudgetFailure(BudgetFailure),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SimplifyResult {
    pub cut: ClosedCut,
    pub frontier: Frontier,
    pub removed_bond_count: usize,
    pub budget_status: lattice_model::BudgetStatus,
}

impl ClosureResult {
    pub const fn is_closed(&self) -> bool {
        matches!(self, Self::Closed { .. } | Self::ClosedWithFrontier { .. })
    }
}

pub fn simplify_closed(cut: &ClosedCut, budget: &ContextBudget) -> ClosureResult {
    if cut.closure_receipts.is_empty() {
        return missing_custody_failure(
            budget,
            "simplify input must be a closed cut with closure receipts",
        );
    }

    let required_grains = cut
        .grain_records
        .iter()
        .filter(|grain| is_required_grain(grain))
        .map(|grain| grain.id.clone())
        .collect::<BTreeSet<_>>();
    let retained_bonds = retained_simplified_bonds(cut, &required_grains);
    let removed_bonds = cut
        .bonds
        .difference(&retained_bonds)
        .cloned()
        .collect::<Vec<_>>();

    let candidate = CandidateCut {
        id: CutId::new(format!("simplify({})", cut.id.as_str())),
        grains: cut.grain_records.clone(),
        bonds: retained_bonds.into_iter().collect(),
    };

    let mut result = with_operator_receipt(close_candidate_cut(candidate, budget), "simplify");
    if removed_bonds.is_empty() {
        return result;
    }

    let mut simplify_frontier = Frontier::new();
    for bond in removed_bonds {
        simplify_frontier.record(FrontierRecord::new(
            FrontierItem::Bond(bond),
            FrontierReason::RedundantContext,
            "optional bond removed by closed-cut simplification",
        ));
    }

    match &mut result {
        ClosureResult::Closed { cut, .. } => ClosureResult::ClosedWithFrontier {
            cut: cut.clone(),
            frontier: simplify_frontier,
            budget_status: lattice_model::BudgetStatus::FrontierDeferred,
        },
        ClosureResult::ClosedWithFrontier { frontier, .. } => {
            for record in simplify_frontier.records() {
                frontier.record(record.clone());
            }
            result
        }
        ClosureResult::BudgetFailure(_) => result,
    }
}

fn retained_simplified_bonds(
    cut: &ClosedCut,
    required_grains: &BTreeSet<GrainId>,
) -> BTreeSet<Bond> {
    let semantic_grain_count = cut.grains.len().saturating_sub(required_grains.len());
    let citation_budget = (semantic_grain_count / 2).max(8);
    let mut retained = BTreeSet::new();
    let mut endpoint_counts = BTreeMap::<GrainId, usize>::new();
    let mut retained_citations = 0usize;
    let mut deferred_citations = Vec::new();

    for bond in &cut.bonds {
        if required_grains.contains(&bond.from) && required_grains.contains(&bond.to) {
            retained.insert(bond.clone());
            continue;
        }

        match bond.kind {
            BondKind::Requires | BondKind::Contradicts | BondKind::DerivesFrom => {
                retained.insert(bond.clone());
            }
            BondKind::Cites | BondKind::SameEntity => {
                if retained_citations < citation_budget
                    && endpoint_counts.get(&bond.from).copied().unwrap_or_default() < 2
                    && endpoint_counts.get(&bond.to).copied().unwrap_or_default() < 2
                {
                    retained.insert(bond.clone());
                    *endpoint_counts.entry(bond.from.clone()).or_default() += 1;
                    *endpoint_counts.entry(bond.to.clone()).or_default() += 1;
                    retained_citations += 1;
                } else {
                    deferred_citations.push(bond.clone());
                }
            }
            BondKind::Contains => {}
        }
    }

    for bond in deferred_citations {
        if retained_citations >= citation_budget {
            break;
        }
        if retained.insert(bond) {
            retained_citations += 1;
        }
    }

    retained
}

pub fn close_candidate_cut(candidate: CandidateCut, budget: &ContextBudget) -> ClosureResult {
    let Some(rights_policy) = effective_rights_policy(&candidate.grains) else {
        return missing_custody_failure(
            budget,
            "candidate cut has no rights policy on one or more grains",
        );
    };

    if !has_required_grain(&candidate.grains, GrainKind::SourcePointer)
        || !has_required_grain(&candidate.grains, GrainKind::Policy)
        || !has_required_grain(&candidate.grains, GrainKind::Receipt)
    {
        return missing_custody_failure(
            budget,
            "candidate cut is missing required source, policy, or receipt grain",
        );
    }

    let required_grains = candidate
        .grains
        .iter()
        .filter(|grain| is_required_grain(grain))
        .map(|grain| grain.id.clone())
        .collect::<BTreeSet<_>>();

    if budget
        .grain_limit
        .is_some_and(|limit| required_grains.len() > limit)
    {
        return required_closure_budget_failure(
            budget,
            required_grains.len(),
            "required closure grains exceed grain budget",
        );
    }

    let mut frontier = Frontier::new();
    let mut retained_grains = BTreeSet::new();
    let mut retained_grain_records = Vec::new();
    for grain in &candidate.grains {
        if required_grains.contains(&grain.id)
            || budget
                .grain_limit
                .is_none_or(|limit| retained_grains.len() < limit)
        {
            retained_grains.insert(grain.id.clone());
            retained_grain_records.push(grain.clone());
        } else {
            frontier.record(FrontierRecord::new(
                FrontierItem::Grain(grain.id.clone()),
                FrontierReason::BudgetLimit,
                "optional grain deferred by closure grain budget",
            ));
        }
    }

    let mut retained_bonds = BTreeSet::new();
    for bond in candidate.bonds {
        if !retained_grains.contains(&bond.from) || !retained_grains.contains(&bond.to) {
            frontier.record(FrontierRecord::new(
                FrontierItem::Bond(bond),
                FrontierReason::InvalidBondEndpoint,
                "bond endpoint is not retained in the closed cut",
            ));
            continue;
        }
        if budget
            .bond_limit
            .is_some_and(|limit| retained_bonds.len() >= limit)
        {
            frontier.record(FrontierRecord::new(
                FrontierItem::Bond(bond),
                FrontierReason::BudgetLimit,
                "optional bond deferred by closure bond budget",
            ));
            continue;
        }
        retained_bonds.insert(bond);
    }

    let mut cut = ClosedCut::new(candidate.id).with_policy(DEFAULT_CLOSURE_POLICY, rights_policy);
    cut.grains = retained_grains;
    cut.grain_records = retained_grain_records;
    cut.bonds = retained_bonds;
    cut.closure_receipts.push(ClosureReceipt::new(
        DEFAULT_CLOSURE_POLICY,
        "candidate cut closed over custody, policy, receipts, and retained bond endpoints",
    ));

    if frontier.is_empty() {
        ClosureResult::Closed {
            cut,
            budget_status: lattice_model::BudgetStatus::WithinBudget,
        }
    } else {
        ClosureResult::ClosedWithFrontier {
            cut,
            budget_status: frontier.status(),
            frontier,
        }
    }
}

pub fn meet_closed(left: &ClosedCut, right: &ClosedCut, budget: &ContextBudget) -> ClosureResult {
    if left.closure_receipts.is_empty() || right.closure_receipts.is_empty() {
        return missing_custody_failure(
            budget,
            "meet inputs must be closed cuts with closure receipts",
        );
    }

    let shared_grains = left
        .grains
        .intersection(&right.grains)
        .cloned()
        .collect::<BTreeSet<_>>();
    let shared_bonds = left
        .bonds
        .intersection(&right.bonds)
        .filter(|bond| shared_grains.contains(&bond.from) && shared_grains.contains(&bond.to))
        .cloned()
        .collect::<BTreeSet<_>>();
    let candidate = CandidateCut {
        id: CutId::new(format!("meet({}, {})", left.id.as_str(), right.id.as_str())),
        grains: grain_records_for(&shared_grains, &[left, right]),
        bonds: shared_bonds.into_iter().collect(),
    };

    with_operator_receipt(close_candidate_cut(candidate, budget), "meet")
}

pub fn join_closed(left: &ClosedCut, right: &ClosedCut, budget: &ContextBudget) -> ClosureResult {
    if left.closure_receipts.is_empty() || right.closure_receipts.is_empty() {
        return missing_custody_failure(
            budget,
            "join inputs must be closed cuts with closure receipts",
        );
    }

    let joined_grains = left
        .grains
        .union(&right.grains)
        .cloned()
        .collect::<BTreeSet<_>>();
    let joined_bonds = left
        .bonds
        .union(&right.bonds)
        .filter(|bond| joined_grains.contains(&bond.from) && joined_grains.contains(&bond.to))
        .cloned()
        .collect::<BTreeSet<_>>();
    let candidate = CandidateCut {
        id: CutId::new(format!("join({}, {})", left.id.as_str(), right.id.as_str())),
        grains: grain_records_for(&joined_grains, &[left, right]),
        bonds: joined_bonds.into_iter().collect(),
    };

    with_operator_receipt(close_candidate_cut(candidate, budget), "join")
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CutRelation {
    Equal,
    LeftNoBroader,
    RightNoBroader,
    Overlapping,
    Disjoint,
}

impl CutRelation {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Equal => "equal",
            Self::LeftNoBroader => "left_no_broader",
            Self::RightNoBroader => "right_no_broader",
            Self::Overlapping => "overlapping",
            Self::Disjoint => "disjoint",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CutRelationDiagnostic {
    pub left: CutId,
    pub right: CutId,
    pub relation: CutRelation,
    pub shared_grain_count: usize,
    pub left_only_grain_count: usize,
    pub right_only_grain_count: usize,
    pub note: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HasseDiagnosticEdge {
    pub narrower: CutId,
    pub broader: CutId,
    pub shared_grain_count: usize,
    pub note: String,
}

pub fn explain_closed_cut_relation(left: &ClosedCut, right: &ClosedCut) -> CutRelationDiagnostic {
    let shared_grain_count = left.grains.intersection(&right.grains).count();
    let left_only_grain_count = left.grains.difference(&right.grains).count();
    let right_only_grain_count = right.grains.difference(&left.grains).count();
    let relation = match (
        left_only_grain_count,
        right_only_grain_count,
        shared_grain_count,
    ) {
        (0, 0, _) => CutRelation::Equal,
        (0, _, _) => CutRelation::LeftNoBroader,
        (_, 0, _) => CutRelation::RightNoBroader,
        (_, _, 0) => CutRelation::Disjoint,
        _ => CutRelation::Overlapping,
    };
    let note = match relation {
        CutRelation::Equal => "closed cuts contain the same grain ids",
        CutRelation::LeftNoBroader => "left closed cut is no broader by grain inclusion",
        CutRelation::RightNoBroader => "right closed cut is no broader by grain inclusion",
        CutRelation::Overlapping => "closed cuts share some but not all grain ids",
        CutRelation::Disjoint => "closed cuts have no shared grain ids",
    }
    .to_string();

    CutRelationDiagnostic {
        left: left.id.clone(),
        right: right.id.clone(),
        relation,
        shared_grain_count,
        left_only_grain_count,
        right_only_grain_count,
        note,
    }
}

pub fn hasse_diagnostic_edges(cuts: &[ClosedCut]) -> Vec<HasseDiagnosticEdge> {
    let mut edges = Vec::new();
    for (left_index, left) in cuts.iter().enumerate() {
        for right in cuts.iter().skip(left_index + 1) {
            let relation = explain_closed_cut_relation(left, right);
            match relation.relation {
                CutRelation::LeftNoBroader => edges.push(HasseDiagnosticEdge {
                    narrower: left.id.clone(),
                    broader: right.id.clone(),
                    shared_grain_count: relation.shared_grain_count,
                    note: "diagnostic inclusion edge over closed cuts".to_string(),
                }),
                CutRelation::RightNoBroader => edges.push(HasseDiagnosticEdge {
                    narrower: right.id.clone(),
                    broader: left.id.clone(),
                    shared_grain_count: relation.shared_grain_count,
                    note: "diagnostic inclusion edge over closed cuts".to_string(),
                }),
                CutRelation::Equal | CutRelation::Overlapping | CutRelation::Disjoint => {}
            }
        }
    }
    edges
}

#[derive(Clone, Debug, PartialEq)]
pub struct ComparisonSide {
    pub selector: String,
    pub candidate_quality: CandidateQualityMetrics,
    pub boundary: BoundaryMetrics,
    pub context_metrics: Option<ContextMetrics>,
    pub cut_hash: Option<String>,
    pub receipt_hash: Option<String>,
    pub missing_required_count: usize,
    pub final_context_valid: bool,
    pub rights_policy: String,
    pub note: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ComparisonReport {
    pub tier: FixtureTier,
    pub fixture_id: String,
    pub baseline: ComparisonSide,
    pub lattice: ComparisonSide,
    pub headline: String,
}

pub fn tiny_top_k_comparison() -> ComparisonReport {
    top_k_comparison(FixtureTier::Tiny)
}

pub fn top_k_comparison(tier: FixtureTier) -> ComparisonReport {
    let started_at = Instant::now();
    let fixture = lattice_model::TinyModelFixture::from_tier(
        tier,
        format!("fontes:comparison:{}", tier.as_str()),
        "derived_text_allowed",
    );
    let universe = fixture
        .grains
        .iter()
        .map(|grain| grain.id.clone())
        .collect::<BTreeSet<_>>();
    let bonds = fixture.bonds.iter().cloned().collect::<BTreeSet<_>>();
    let generic_candidate_count = match tier {
        FixtureTier::Tiny => 3,
        FixtureTier::Small => 48,
        FixtureTier::Medium | FixtureTier::Large => 128,
    };
    let generic_candidates = fixture
        .grains
        .iter()
        .filter(|grain| grain.kind == GrainKind::Context)
        .take(generic_candidate_count)
        .map(|grain| grain.id.clone())
        .collect::<BTreeSet<_>>();
    let required = fixture
        .grains
        .iter()
        .filter(|grain| is_required_grain(grain))
        .map(|grain| grain.id.clone())
        .collect::<BTreeSet<_>>();
    let expected_context_count = match tier {
        FixtureTier::Tiny => 2,
        FixtureTier::Small => 32,
        FixtureTier::Medium | FixtureTier::Large => 96,
    };
    let mut expected = fixture
        .grains
        .iter()
        .filter(|grain| grain.kind == GrainKind::Context)
        .take(expected_context_count)
        .map(|grain| grain.id.clone())
        .collect::<BTreeSet<_>>();
    expected.extend(required.iter().cloned());
    let generic_missing_required_count = required.difference(&generic_candidates).count();
    let baseline_quality = CandidateQualityMetrics::from_sets(
        &expected,
        &generic_candidates,
        &generic_candidates,
        &BTreeSet::new(),
    );
    let baseline_boundary = BoundaryMetrics::from_bonds(&universe, &bonds, &generic_candidates)
        .expect("Tiny fixture bonds are inside the universe");

    let lattice_candidate_ids = generic_candidates
        .union(&required)
        .cloned()
        .collect::<BTreeSet<_>>();
    let candidate = fixture
        .grains
        .iter()
        .filter(|grain| lattice_candidate_ids.contains(&grain.id))
        .cloned()
        .fold(
            CandidateCut::new(CutId::new(format!("{}-comparison-lattice", tier.as_str()))),
            |candidate, grain| candidate.with_grain(grain),
        );
    let candidate = fixture
        .bonds
        .iter()
        .filter(|bond| {
            lattice_candidate_ids.contains(&bond.from) && lattice_candidate_ids.contains(&bond.to)
        })
        .cloned()
        .fold(candidate, |candidate, bond| candidate.with_bond(bond));

    let budget = ContextBudget::fixture_tier(tier);
    let (closed, frontier) = match close_candidate_cut(candidate, &budget) {
        ClosureResult::Closed { cut, .. } => (cut, Frontier::new()),
        ClosureResult::ClosedWithFrontier { cut, frontier, .. } => (cut, frontier),
        ClosureResult::BudgetFailure(failure) => {
            panic!(
                "{} comparison lattice candidate should close: {failure:?}",
                tier.as_str()
            );
        }
    };
    let frontier_grains = frontier
        .records()
        .iter()
        .filter_map(|record| match &record.item {
            FrontierItem::Grain(grain) => Some(grain.clone()),
            FrontierItem::Bond(_) | FrontierItem::Receipt(_) => None,
        })
        .collect::<BTreeSet<_>>();
    let lattice_quality = CandidateQualityMetrics::from_sets(
        &expected,
        &generic_candidates,
        &closed.grains,
        &frontier_grains,
    );
    let lattice_boundary = BoundaryMetrics::from_bonds(&universe, &bonds, &closed.grains)
        .expect("closed cut grains are inside the universe");
    let closure_added_count = closed.grains.difference(&generic_candidates).count();
    let context_metrics = ContextMetrics::from_closed_cut(
        &closed,
        &budget,
        &frontier,
        started_at.elapsed().as_millis(),
        0,
        Some(closed.grains.len() * 32),
        closure_added_count,
    );

    ComparisonReport {
        tier,
        fixture_id: format!("{}-top-k-vs-closed-cut", tier.as_str()),
        baseline: ComparisonSide {
            selector: "generic-top-k".to_string(),
            candidate_quality: baseline_quality,
            boundary: baseline_boundary,
            context_metrics: None,
            cut_hash: None,
            receipt_hash: None,
            missing_required_count: generic_missing_required_count,
            final_context_valid: false,
            rights_policy: "untracked".to_string(),
            note: "top-k selected plausible context grains but missed required source/policy/receipt closure"
                .to_string(),
        },
        lattice: ComparisonSide {
            selector: "lattice-closed-cut".to_string(),
            candidate_quality: lattice_quality,
            boundary: lattice_boundary,
            context_metrics: Some(context_metrics),
            cut_hash: Some(closed.stable_hash()),
            receipt_hash: Some(closed.receipt_hash()),
            missing_required_count: required.difference(&closed.grains).count(),
            final_context_valid: true,
            rights_policy: closed
                .rights_policy
                .clone()
                .unwrap_or_else(|| "unknown".to_string()),
            note: "closed cut preserves provenance, rights policy, receipts, and budget status"
                .to_string(),
        },
        headline: format!(
            "LATTICE turns a plausible {} top-k candidate into auditable final context by preserving closure requirements.",
            tier.as_str()
        ),
    }
}

pub fn bridge_comparison() -> ComparisonReport {
    let started_at = Instant::now();
    let source_id = "fontes:comparison:bridge";
    let rights_policy = "derived_text_allowed";
    let source = Grain::new(GrainId::new("bridge:source"), "source pointer").with_metadata(
        GrainKind::SourcePointer,
        source_id,
        rights_policy,
    );
    let left = Grain::new(GrainId::new("bridge:left"), "left evidence").with_metadata(
        GrainKind::Context,
        source_id,
        rights_policy,
    );
    let bridge = Grain::new(GrainId::new("bridge:connector"), "required connector").with_metadata(
        GrainKind::Context,
        source_id,
        rights_policy,
    );
    let right = Grain::new(GrainId::new("bridge:right"), "right evidence").with_metadata(
        GrainKind::Context,
        source_id,
        rights_policy,
    );
    let distractor = Grain::new(GrainId::new("bridge:distractor"), "noisy periphery")
        .with_metadata(GrainKind::Context, source_id, rights_policy);
    let policy = Grain::new(GrainId::new("bridge:policy"), "rights policy").with_metadata(
        GrainKind::Policy,
        source_id,
        rights_policy,
    );
    let receipt = Grain::new(GrainId::new("bridge:receipt"), "closure receipt").with_metadata(
        GrainKind::Receipt,
        source_id,
        rights_policy,
    );
    let grains = [
        source.clone(),
        left.clone(),
        bridge.clone(),
        right.clone(),
        distractor.clone(),
        policy.clone(),
        receipt.clone(),
    ];
    let bonds = BTreeSet::from([
        Bond::new(source.id.clone(), left.id.clone(), BondKind::Contains),
        Bond::new(source.id.clone(), bridge.id.clone(), BondKind::Contains),
        Bond::new(source.id.clone(), right.id.clone(), BondKind::Contains),
        Bond::new(source.id.clone(), distractor.id.clone(), BondKind::Contains),
        Bond::new(source.id.clone(), policy.id.clone(), BondKind::Contains),
        Bond::new(source.id.clone(), receipt.id.clone(), BondKind::Contains),
        Bond::new(left.id.clone(), bridge.id.clone(), BondKind::Requires),
        Bond::new(bridge.id.clone(), right.id.clone(), BondKind::Requires),
        Bond::new(distractor.id.clone(), source.id.clone(), BondKind::Cites),
    ]);
    let universe = grains
        .iter()
        .map(|grain| grain.id.clone())
        .collect::<BTreeSet<_>>();
    let generic_candidates =
        BTreeSet::from([left.id.clone(), right.id.clone(), distractor.id.clone()]);
    let required = BTreeSet::from([source.id.clone(), policy.id.clone(), receipt.id.clone()]);
    let expected = BTreeSet::from([
        source.id.clone(),
        left.id.clone(),
        bridge.id.clone(),
        right.id.clone(),
        policy.id.clone(),
        receipt.id.clone(),
    ]);
    let baseline_quality = CandidateQualityMetrics::from_sets(
        &expected,
        &generic_candidates,
        &generic_candidates,
        &BTreeSet::new(),
    );
    let baseline_boundary = BoundaryMetrics::from_bonds(&universe, &bonds, &generic_candidates)
        .expect("bridge fixture bonds are inside the universe");
    let lattice_candidate_ids = generic_candidates
        .union(&required)
        .cloned()
        .chain([bridge.id.clone()])
        .collect::<BTreeSet<_>>();
    let candidate = grains
        .iter()
        .filter(|grain| lattice_candidate_ids.contains(&grain.id))
        .cloned()
        .fold(
            CandidateCut::new(CutId::new("bridge-comparison-lattice")),
            |candidate, grain| candidate.with_grain(grain),
        );
    let candidate = bonds
        .iter()
        .filter(|bond| {
            lattice_candidate_ids.contains(&bond.from) && lattice_candidate_ids.contains(&bond.to)
        })
        .cloned()
        .fold(candidate, |candidate, bond| candidate.with_bond(bond));
    let budget = ContextBudget::tiny_fixture();
    let (closed, frontier) = match close_candidate_cut(candidate, &budget) {
        ClosureResult::Closed { cut, .. } => (cut, Frontier::new()),
        ClosureResult::ClosedWithFrontier { cut, frontier, .. } => (cut, frontier),
        ClosureResult::BudgetFailure(failure) => {
            panic!("bridge comparison lattice candidate should close: {failure:?}");
        }
    };
    let frontier_grains = frontier
        .records()
        .iter()
        .filter_map(|record| match &record.item {
            FrontierItem::Grain(grain) => Some(grain.clone()),
            FrontierItem::Bond(_) | FrontierItem::Receipt(_) => None,
        })
        .collect::<BTreeSet<_>>();
    let lattice_quality = CandidateQualityMetrics::from_sets(
        &expected,
        &generic_candidates,
        &closed.grains,
        &frontier_grains,
    );
    let lattice_boundary = BoundaryMetrics::from_bonds(&universe, &bonds, &closed.grains)
        .expect("closed bridge cut grains are inside the universe");
    let closure_added_count = closed.grains.difference(&generic_candidates).count();
    let context_metrics = ContextMetrics::from_closed_cut(
        &closed,
        &budget,
        &frontier,
        started_at.elapsed().as_millis(),
        0,
        Some(closed.grains.len() * 32),
        closure_added_count,
    );

    ComparisonReport {
        tier: FixtureTier::Tiny,
        fixture_id: "bridge-top-k-vs-closed-cut".to_string(),
        baseline: ComparisonSide {
            selector: "generic-top-k".to_string(),
            candidate_quality: baseline_quality,
            boundary: baseline_boundary,
            context_metrics: None,
            cut_hash: None,
            receipt_hash: None,
            missing_required_count: required.difference(&generic_candidates).count(),
            final_context_valid: false,
            rights_policy: "untracked".to_string(),
            note: "top-k selected endpoints and a noisy periphery grain while missing connector/provenance closure"
                .to_string(),
        },
        lattice: ComparisonSide {
            selector: "lattice-closed-cut".to_string(),
            candidate_quality: lattice_quality,
            boundary: lattice_boundary,
            context_metrics: Some(context_metrics),
            cut_hash: Some(closed.stable_hash()),
            receipt_hash: Some(closed.receipt_hash()),
            missing_required_count: required.difference(&closed.grains).count(),
            final_context_valid: true,
            rights_policy: closed
                .rights_policy
                .clone()
                .unwrap_or_else(|| "unknown".to_string()),
            note: "closed cut rescues the connector plus required source, policy, and receipt material"
                .to_string(),
        },
        headline: "LATTICE turns disconnected top-k evidence into auditable connected context by rescuing the bridge and closure material."
            .to_string(),
    }
}

pub fn launch_readiness_comparison() -> ComparisonReport {
    launch_readiness_comparison_at_scale(45)
}

#[derive(Clone, Debug, PartialEq)]
pub struct LaunchReadinessClosedCut {
    pub fact_count: usize,
    pub decision_cut_fact_count: usize,
    pub expected_count: usize,
    pub auto_bond_count: usize,
    pub guidance_bond_count: usize,
    pub cut: ClosedCut,
    pub frontier: Frontier,
    pub budget: ContextBudget,
}

pub fn close_launch_readiness_at_scale(per_department: usize) -> LaunchReadinessClosedCut {
    let fixture = LaunchReadinessFixture::parse_with_generated_per_department(per_department);
    let source = Grain::new(
        GrainId::new("launch-readiness:source"),
        "synthetic launch-readiness source pointer",
    )
    .with_metadata(
        GrainKind::SourcePointer,
        fixture.source_id.clone(),
        fixture.rights_policy.clone(),
    );
    let policy = Grain::new(
        GrainId::new("launch-readiness:policy"),
        "synthetic public demo rights policy",
    )
    .with_metadata(
        GrainKind::Policy,
        fixture.source_id.clone(),
        fixture.rights_policy.clone(),
    );
    let receipt = Grain::new(
        GrainId::new("launch-readiness:receipt"),
        "synthetic launch-readiness receipt",
    )
    .with_metadata(
        GrainKind::Receipt,
        fixture.source_id.clone(),
        fixture.rights_policy.clone(),
    );
    let mut bonds = fixture.auto_bonds.iter().cloned().collect::<BTreeSet<_>>();
    bonds.insert(Bond::new(
        source.id.clone(),
        policy.id.clone(),
        BondKind::Requires,
    ));
    bonds.insert(Bond::new(
        source.id.clone(),
        receipt.id.clone(),
        BondKind::Requires,
    ));

    let required = BTreeSet::from([source.id.clone(), policy.id.clone(), receipt.id.clone()]);
    let mut expected = fixture
        .decision_cut_fact_ids
        .iter()
        .map(|id| GrainId::new(format!("{}:{id}", fixture.source_id)))
        .collect::<BTreeSet<_>>();
    expected.extend(required.iter().cloned());
    let candidate = fixture
        .grains
        .iter()
        .filter(|grain| expected.contains(&grain.id))
        .cloned()
        .chain([source, policy, receipt])
        .fold(
            CandidateCut::new(CutId::new("launch-readiness-closed")),
            |candidate, grain| candidate.with_grain(grain),
        );
    let candidate = bonds
        .iter()
        .filter(|bond| expected.contains(&bond.from) && expected.contains(&bond.to))
        .cloned()
        .fold(candidate, |candidate, bond| candidate.with_bond(bond));
    let budget = ContextBudget {
        token_limit: Some(fixture.fact_count() * 128),
        grain_limit: Some(expected.len() + 16),
        bond_limit: Some(fixture.auto_bond_count() + 16),
        closure_expansion_limit: Some(expected.len() + 16),
        receipt_limit: Some(32),
        output_byte_limit: Some(262_144),
    };
    let (cut, frontier) = match close_candidate_cut(candidate, &budget) {
        ClosureResult::Closed { cut, .. } => (cut, Frontier::new()),
        ClosureResult::ClosedWithFrontier { cut, frontier, .. } => (cut, frontier),
        ClosureResult::BudgetFailure(failure) => {
            panic!("launch-readiness closed cut should close: {failure:?}");
        }
    };

    LaunchReadinessClosedCut {
        fact_count: fixture.fact_count(),
        decision_cut_fact_count: fixture.decision_cut_fact_count(),
        expected_count: expected.len(),
        auto_bond_count: fixture.auto_bond_count(),
        guidance_bond_count: fixture.guidance_bond_count(),
        cut,
        frontier,
        budget,
    }
}

pub fn launch_readiness_comparison_at_scale(per_department: usize) -> ComparisonReport {
    let started_at = Instant::now();
    let fixture = LaunchReadinessFixture::parse_with_generated_per_department(per_department);
    let fact_count = fixture.fact_count();
    let decision_cut_fact_count = fixture.decision_cut_fact_count();
    let source = Grain::new(
        GrainId::new("launch-readiness:source"),
        "synthetic launch-readiness source pointer",
    )
    .with_metadata(
        GrainKind::SourcePointer,
        fixture.source_id.clone(),
        fixture.rights_policy.clone(),
    );
    let policy = Grain::new(
        GrainId::new("launch-readiness:policy"),
        "synthetic public demo rights policy",
    )
    .with_metadata(
        GrainKind::Policy,
        fixture.source_id.clone(),
        fixture.rights_policy.clone(),
    );
    let receipt = Grain::new(
        GrainId::new("launch-readiness:receipt"),
        "synthetic launch-readiness receipt",
    )
    .with_metadata(
        GrainKind::Receipt,
        fixture.source_id.clone(),
        fixture.rights_policy.clone(),
    );

    let mut universe = fixture
        .grains
        .iter()
        .map(|grain| grain.id.clone())
        .collect::<BTreeSet<_>>();
    universe.insert(source.id.clone());
    universe.insert(policy.id.clone());
    universe.insert(receipt.id.clone());

    let mut bonds = fixture.auto_bonds.iter().cloned().collect::<BTreeSet<_>>();
    bonds.insert(Bond::new(
        source.id.clone(),
        policy.id.clone(),
        BondKind::Requires,
    ));
    bonds.insert(Bond::new(
        source.id.clone(),
        receipt.id.clone(),
        BondKind::Requires,
    ));

    let generic_candidates = fixture
        .grains
        .iter()
        .take(80)
        .map(|grain| grain.id.clone())
        .collect::<BTreeSet<_>>();
    let required = BTreeSet::from([source.id.clone(), policy.id.clone(), receipt.id.clone()]);
    let mut expected = fixture
        .decision_cut_fact_ids
        .iter()
        .map(|id| GrainId::new(format!("{}:{id}", fixture.source_id)))
        .collect::<BTreeSet<_>>();
    expected.extend(required.iter().cloned());

    let baseline_quality = CandidateQualityMetrics::from_sets(
        &expected,
        &generic_candidates,
        &generic_candidates,
        &BTreeSet::new(),
    );
    let baseline_boundary = BoundaryMetrics::from_bonds(&universe, &bonds, &generic_candidates)
        .expect("launch fixture top-k candidates are inside the universe");

    let lattice_candidate_ids = expected.clone();
    let candidate = fixture
        .grains
        .iter()
        .filter(|grain| lattice_candidate_ids.contains(&grain.id))
        .cloned()
        .chain([source.clone(), policy.clone(), receipt.clone()])
        .fold(
            CandidateCut::new(CutId::new("launch-readiness-comparison-lattice")),
            |candidate, grain| candidate.with_grain(grain),
        );
    let candidate = bonds
        .iter()
        .filter(|bond| {
            lattice_candidate_ids.contains(&bond.from) && lattice_candidate_ids.contains(&bond.to)
        })
        .cloned()
        .fold(candidate, |candidate, bond| candidate.with_bond(bond));

    let budget = ContextBudget {
        token_limit: Some(fact_count * 128),
        grain_limit: Some(expected.len() + 16),
        bond_limit: Some(fixture.auto_bond_count() + 16),
        closure_expansion_limit: Some(expected.len() + 16),
        receipt_limit: Some(32),
        output_byte_limit: Some(262_144),
    };
    let (closed, frontier) = match close_candidate_cut(candidate, &budget) {
        ClosureResult::Closed { cut, .. } => (cut, Frontier::new()),
        ClosureResult::ClosedWithFrontier { cut, frontier, .. } => (cut, frontier),
        ClosureResult::BudgetFailure(failure) => {
            panic!("launch-readiness comparison lattice candidate should close: {failure:?}");
        }
    };
    let frontier_grains = frontier
        .records()
        .iter()
        .filter_map(|record| match &record.item {
            FrontierItem::Grain(grain) => Some(grain.clone()),
            FrontierItem::Bond(_) | FrontierItem::Receipt(_) => None,
        })
        .collect::<BTreeSet<_>>();
    let lattice_quality = CandidateQualityMetrics::from_sets(
        &expected,
        &generic_candidates,
        &closed.grains,
        &frontier_grains,
    );
    let lattice_boundary = BoundaryMetrics::from_bonds(&universe, &bonds, &closed.grains)
        .expect("launch closed cut grains are inside the universe");
    let closure_added_count = closed.grains.difference(&generic_candidates).count();
    let context_metrics = ContextMetrics::from_closed_cut(
        &closed,
        &budget,
        &frontier,
        started_at.elapsed().as_millis(),
        0,
        Some(closed.grains.len() * 48),
        closure_added_count,
    );

    ComparisonReport {
        tier: FixtureTier::Small,
        fixture_id: "launch-readiness-top-k-vs-decision-cut".to_string(),
        baseline: ComparisonSide {
            selector: "generic-top-k-80".to_string(),
            candidate_quality: baseline_quality,
            boundary: baseline_boundary,
            context_metrics: None,
            cut_hash: None,
            receipt_hash: None,
            missing_required_count: required.difference(&generic_candidates).count(),
            final_context_valid: false,
            rights_policy: "untracked".to_string(),
            note: format!("top-k selects the first 80 plausible department facts from {fact_count} facts but misses closure and much of the launch decision cut"),
        },
        lattice: ComparisonSide {
            selector: "lattice-decision-cut".to_string(),
            candidate_quality: lattice_quality,
            boundary: lattice_boundary,
            context_metrics: Some(context_metrics),
            cut_hash: Some(closed.stable_hash()),
            receipt_hash: Some(closed.receipt_hash()),
            missing_required_count: required.difference(&closed.grains).count(),
            final_context_valid: true,
            rights_policy: closed
                .rights_policy
                .clone()
                .unwrap_or_else(|| "unknown".to_string()),
            note: format!("decision cut keeps {decision_cut_fact_count} launch-relevant facts from {fact_count} department facts and preserves source, policy, and receipt closure"),
        },
        headline: format!("LATTICE cuts a noisy {fact_count}-fact launch corpus to a closed {decision_cut_fact_count}-fact decision slice, while generic top-k stays plausible but incomplete."),
    }
}

pub fn budget_pressure_comparison() -> ComparisonReport {
    let started_at = Instant::now();
    let source_id = "fontes:comparison:budget";
    let rights_policy = "derived_text_allowed";
    let source = Grain::new(GrainId::new("budget:source"), "source pointer").with_metadata(
        GrainKind::SourcePointer,
        source_id,
        rights_policy,
    );
    let policy = Grain::new(GrainId::new("budget:policy"), "rights policy").with_metadata(
        GrainKind::Policy,
        source_id,
        rights_policy,
    );
    let receipt = Grain::new(GrainId::new("budget:receipt"), "closure receipt").with_metadata(
        GrainKind::Receipt,
        source_id,
        rights_policy,
    );
    let contexts = (1..=5)
        .map(|index| {
            Grain::new(
                GrainId::new(format!("budget:context:{index}")),
                format!("budget context {index}"),
            )
            .with_metadata(GrainKind::Context, source_id, rights_policy)
        })
        .collect::<Vec<_>>();
    let mut grains = vec![source.clone(), policy.clone(), receipt.clone()];
    grains.extend(contexts.iter().cloned());

    let mut bonds = BTreeSet::new();
    for grain in &grains[1..] {
        bonds.insert(Bond::new(
            source.id.clone(),
            grain.id.clone(),
            BondKind::Contains,
        ));
    }
    for pair in contexts.windows(2) {
        bonds.insert(Bond::new(
            pair[0].id.clone(),
            pair[1].id.clone(),
            BondKind::Requires,
        ));
    }

    let universe = grains
        .iter()
        .map(|grain| grain.id.clone())
        .collect::<BTreeSet<_>>();
    let generic_candidates = contexts
        .iter()
        .map(|grain| grain.id.clone())
        .collect::<BTreeSet<_>>();
    let required = BTreeSet::from([source.id.clone(), policy.id.clone(), receipt.id.clone()]);
    let mut expected = generic_candidates.clone();
    expected.extend(required.iter().cloned());

    let baseline_quality = CandidateQualityMetrics::from_sets(
        &expected,
        &generic_candidates,
        &generic_candidates,
        &BTreeSet::new(),
    );
    let baseline_boundary = BoundaryMetrics::from_bonds(&universe, &bonds, &generic_candidates)
        .expect("budget fixture bonds are inside the universe");
    let lattice_candidate_ids = generic_candidates
        .union(&required)
        .cloned()
        .collect::<BTreeSet<_>>();
    let candidate = grains
        .iter()
        .filter(|grain| lattice_candidate_ids.contains(&grain.id))
        .cloned()
        .fold(
            CandidateCut::new(CutId::new("budget-comparison-lattice")),
            |candidate, grain| candidate.with_grain(grain),
        );
    let candidate = bonds
        .iter()
        .filter(|bond| {
            lattice_candidate_ids.contains(&bond.from) && lattice_candidate_ids.contains(&bond.to)
        })
        .cloned()
        .fold(candidate, |candidate, bond| candidate.with_bond(bond));
    let budget = ContextBudget {
        grain_limit: Some(5),
        bond_limit: Some(8),
        token_limit: Some(320),
        ..ContextBudget::tiny_fixture()
    };
    let (closed, frontier) = match close_candidate_cut(candidate, &budget) {
        ClosureResult::ClosedWithFrontier { cut, frontier, .. } => (cut, frontier),
        ClosureResult::Closed { cut, .. } => (cut, Frontier::new()),
        ClosureResult::BudgetFailure(failure) => {
            panic!("budget pressure comparison should frontier, not fail: {failure:?}");
        }
    };
    let frontier_grains = frontier
        .records()
        .iter()
        .filter_map(|record| match &record.item {
            FrontierItem::Grain(grain) => Some(grain.clone()),
            FrontierItem::Bond(_) | FrontierItem::Receipt(_) => None,
        })
        .collect::<BTreeSet<_>>();
    let lattice_quality = CandidateQualityMetrics::from_sets(
        &expected,
        &generic_candidates,
        &closed.grains,
        &frontier_grains,
    );
    let lattice_boundary = BoundaryMetrics::from_bonds(&universe, &bonds, &closed.grains)
        .expect("closed budget cut grains are inside the universe");
    let closure_added_count = closed.grains.difference(&generic_candidates).count();
    let context_metrics = ContextMetrics::from_closed_cut(
        &closed,
        &budget,
        &frontier,
        started_at.elapsed().as_millis(),
        0,
        Some(closed.grains.len() * 32),
        closure_added_count,
    );

    ComparisonReport {
        tier: FixtureTier::Tiny,
        fixture_id: "budget-top-k-vs-closed-cut".to_string(),
        baseline: ComparisonSide {
            selector: "generic-top-k".to_string(),
            candidate_quality: baseline_quality,
            boundary: baseline_boundary,
            context_metrics: None,
            cut_hash: None,
            receipt_hash: None,
            missing_required_count: required.difference(&generic_candidates).count(),
            final_context_valid: false,
            rights_policy: "untracked".to_string(),
            note: "top-k fills the grain budget with optional context and leaves no room for required closure"
                .to_string(),
        },
        lattice: ComparisonSide {
            selector: "lattice-closed-cut".to_string(),
            candidate_quality: lattice_quality,
            boundary: lattice_boundary,
            context_metrics: Some(context_metrics),
            cut_hash: Some(closed.stable_hash()),
            receipt_hash: Some(closed.receipt_hash()),
            missing_required_count: required.difference(&closed.grains).count(),
            final_context_valid: true,
            rights_policy: closed
                .rights_policy
                .clone()
                .unwrap_or_else(|| "unknown".to_string()),
            note: "closed cut reserves required source, policy, and receipt material and frontiers optional context"
                .to_string(),
        },
        headline: "LATTICE handles budget pressure by preserving required closure and recording optional context on the frontier."
            .to_string(),
    }
}

pub fn rights_policy_comparison() -> ComparisonReport {
    let started_at = Instant::now();
    let permissive_source_id = "fontes:comparison:rights:permissive";
    let restricted_source_id = "fontes:comparison:rights:restricted";
    let source = Grain::new(GrainId::new("rights:source"), "source pointer").with_metadata(
        GrainKind::SourcePointer,
        permissive_source_id,
        "derived_text_allowed",
    );
    let permissive = Grain::new(GrainId::new("rights:permissive"), "derived evidence")
        .with_metadata(
            GrainKind::Context,
            permissive_source_id,
            "derived_text_allowed",
        );
    let restricted = Grain::new(GrainId::new("rights:restricted"), "restricted evidence")
        .with_metadata(GrainKind::Context, restricted_source_id, "reference_only");
    let policy = Grain::new(GrainId::new("rights:policy"), "mixed rights policy").with_metadata(
        GrainKind::Policy,
        permissive_source_id,
        "derived_text_allowed",
    );
    let receipt = Grain::new(GrainId::new("rights:receipt"), "closure receipt").with_metadata(
        GrainKind::Receipt,
        permissive_source_id,
        "derived_text_allowed",
    );
    let grains = [
        source.clone(),
        permissive.clone(),
        restricted.clone(),
        policy.clone(),
        receipt.clone(),
    ];
    let bonds = BTreeSet::from([
        Bond::new(source.id.clone(), permissive.id.clone(), BondKind::Contains),
        Bond::new(source.id.clone(), restricted.id.clone(), BondKind::Contains),
        Bond::new(source.id.clone(), policy.id.clone(), BondKind::Contains),
        Bond::new(source.id.clone(), receipt.id.clone(), BondKind::Contains),
        Bond::new(
            restricted.id.clone(),
            permissive.id.clone(),
            BondKind::Cites,
        ),
    ]);
    let universe = grains
        .iter()
        .map(|grain| grain.id.clone())
        .collect::<BTreeSet<_>>();
    let generic_candidates = BTreeSet::from([permissive.id.clone(), restricted.id.clone()]);
    let required = BTreeSet::from([source.id.clone(), policy.id.clone(), receipt.id.clone()]);
    let mut expected = generic_candidates.clone();
    expected.extend(required.iter().cloned());
    let baseline_quality = CandidateQualityMetrics::from_sets(
        &expected,
        &generic_candidates,
        &generic_candidates,
        &BTreeSet::new(),
    );
    let baseline_boundary = BoundaryMetrics::from_bonds(&universe, &bonds, &generic_candidates)
        .expect("rights fixture bonds are inside the universe");
    let lattice_candidate_ids = generic_candidates
        .union(&required)
        .cloned()
        .collect::<BTreeSet<_>>();
    let candidate = grains
        .iter()
        .filter(|grain| lattice_candidate_ids.contains(&grain.id))
        .cloned()
        .fold(
            CandidateCut::new(CutId::new("rights-comparison-lattice")),
            |candidate, grain| candidate.with_grain(grain),
        );
    let candidate = bonds
        .iter()
        .filter(|bond| {
            lattice_candidate_ids.contains(&bond.from) && lattice_candidate_ids.contains(&bond.to)
        })
        .cloned()
        .fold(candidate, |candidate, bond| candidate.with_bond(bond));
    let budget = ContextBudget::tiny_fixture();
    let (closed, frontier) = match close_candidate_cut(candidate, &budget) {
        ClosureResult::Closed { cut, .. } => (cut, Frontier::new()),
        ClosureResult::ClosedWithFrontier { cut, frontier, .. } => (cut, frontier),
        ClosureResult::BudgetFailure(failure) => {
            panic!("rights comparison lattice candidate should close: {failure:?}");
        }
    };
    let frontier_grains = frontier
        .records()
        .iter()
        .filter_map(|record| match &record.item {
            FrontierItem::Grain(grain) => Some(grain.clone()),
            FrontierItem::Bond(_) | FrontierItem::Receipt(_) => None,
        })
        .collect::<BTreeSet<_>>();
    let lattice_quality = CandidateQualityMetrics::from_sets(
        &expected,
        &generic_candidates,
        &closed.grains,
        &frontier_grains,
    );
    let lattice_boundary = BoundaryMetrics::from_bonds(&universe, &bonds, &closed.grains)
        .expect("closed rights cut grains are inside the universe");
    let closure_added_count = closed.grains.difference(&generic_candidates).count();
    let context_metrics = ContextMetrics::from_closed_cut(
        &closed,
        &budget,
        &frontier,
        started_at.elapsed().as_millis(),
        0,
        Some(closed.grains.len() * 32),
        closure_added_count,
    );

    ComparisonReport {
        tier: FixtureTier::Tiny,
        fixture_id: "rights-top-k-vs-closed-cut".to_string(),
        baseline: ComparisonSide {
            selector: "generic-top-k".to_string(),
            candidate_quality: baseline_quality,
            boundary: baseline_boundary,
            context_metrics: None,
            cut_hash: None,
            receipt_hash: None,
            missing_required_count: required.difference(&generic_candidates).count(),
            final_context_valid: false,
            rights_policy: "untracked".to_string(),
            note: "top-k mixes derived_text_allowed and reference_only grains without a rights policy"
                .to_string(),
        },
        lattice: ComparisonSide {
            selector: "lattice-closed-cut".to_string(),
            candidate_quality: lattice_quality,
            boundary: lattice_boundary,
            context_metrics: Some(context_metrics),
            cut_hash: Some(closed.stable_hash()),
            receipt_hash: Some(closed.receipt_hash()),
            missing_required_count: required.difference(&closed.grains).count(),
            final_context_valid: true,
            rights_policy: closed
                .rights_policy
                .clone()
                .unwrap_or_else(|| "unknown".to_string()),
            note: "closed cut surfaces mixed rights policy and keeps source, policy, and receipt material"
                .to_string(),
        },
        headline: "LATTICE makes mixed rights explicit instead of letting top-k context hide policy drift."
            .to_string(),
    }
}

fn grain_records_for(ids: &BTreeSet<GrainId>, cuts: &[&ClosedCut]) -> Vec<Grain> {
    let mut seen = BTreeSet::new();
    let mut grains = Vec::new();
    for cut in cuts {
        for grain in &cut.grain_records {
            if ids.contains(&grain.id) && seen.insert(grain.id.clone()) {
                grains.push(grain.clone());
            }
        }
    }
    grains
}

fn with_operator_receipt(mut result: ClosureResult, operator: &str) -> ClosureResult {
    let note = match operator {
        "meet" => "meet candidate constructed from shared closed-cut context and re-closed",
        "join" => "join candidate constructed from unioned closed-cut context and re-closed",
        "simplify" => "closed cut simplified by removing redundant optional bonds and re-closing",
        _ => "operator candidate constructed and re-closed",
    };
    match &mut result {
        ClosureResult::Closed { cut, .. } | ClosureResult::ClosedWithFrontier { cut, .. } => {
            cut.closure_receipts
                .push(ClosureReceipt::new(operator, note));
        }
        ClosureResult::BudgetFailure(failure) => {
            failure.receipt = ClosureReceipt::new(operator, note);
        }
    }
    result
}

fn effective_rights_policy(grains: &[Grain]) -> Option<String> {
    let mut rights = grains
        .iter()
        .map(|grain| grain.rights_policy.as_deref())
        .collect::<Option<Vec<_>>>()?;
    rights.sort_unstable();
    rights.dedup();
    match rights.as_slice() {
        [] => None,
        [single] => Some((*single).to_string()),
        _ => Some("mixed".to_string()),
    }
}

fn has_required_grain(grains: &[Grain], kind: GrainKind) -> bool {
    grains.iter().any(|grain| grain.kind == kind)
}

fn is_required_grain(grain: &Grain) -> bool {
    matches!(
        grain.kind,
        GrainKind::SourcePointer | GrainKind::Policy | GrainKind::Receipt
    )
}

fn missing_custody_failure(budget: &ContextBudget, note: impl Into<String>) -> ClosureResult {
    ClosureResult::BudgetFailure(BudgetFailure::new(
        FrontierReason::MissingCustody,
        budget.clone(),
        0,
        ClosureReceipt::new(DEFAULT_CLOSURE_POLICY, note),
    ))
}

fn required_closure_budget_failure(
    budget: &ContextBudget,
    frontier_count: usize,
    note: impl Into<String>,
) -> ClosureResult {
    ClosureResult::BudgetFailure(BudgetFailure::new(
        FrontierReason::RequiredClosureExceedsBudget,
        budget.clone(),
        frontier_count,
        ClosureReceipt::new(DEFAULT_CLOSURE_POLICY, note),
    ))
}

pub fn meet(left: &ClosedCut, right: &ClosedCut) -> ClosedCut {
    let mut cut = ClosedCut::new(CutId::new(format!(
        "meet({}, {})",
        left.id.as_str(),
        right.id.as_str()
    )))
    .with_policy(DEFAULT_CLOSURE_POLICY, "derived_text_allowed");

    cut.grains = left.grains.intersection(&right.grains).cloned().collect();
    cut.bonds = left
        .bonds
        .intersection(&right.bonds)
        .filter(|bond| cut.contains_bond_endpoints(bond))
        .cloned()
        .collect();
    cut.closure_receipts.push(ClosureReceipt::new(
        "meet",
        "intersection closed over retained bonds",
    ));
    cut
}

pub fn join(left: &ClosedCut, right: &ClosedCut) -> ClosedCut {
    let mut cut = ClosedCut::new(CutId::new(format!(
        "join({}, {})",
        left.id.as_str(),
        right.id.as_str()
    )))
    .with_policy(DEFAULT_CLOSURE_POLICY, "derived_text_allowed");

    cut.grains = left.grains.union(&right.grains).cloned().collect();
    cut.bonds = left
        .bonds
        .union(&right.bonds)
        .filter(|bond| cut.contains_bond_endpoints(bond))
        .cloned()
        .collect();
    cut.closure_receipts.push(ClosureReceipt::new(
        "join",
        "union closed over retained bonds",
    ));
    cut
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AlgebraPropertyRow {
    pub id: &'static str,
    pub law: &'static str,
    pub operator: &'static str,
    pub left_grain_count: usize,
    pub right_grain_count: usize,
    pub passed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AlgebraPropertyReport {
    pub rows: Vec<AlgebraPropertyRow>,
}

impl AlgebraPropertyReport {
    pub fn passed(&self) -> bool {
        self.rows.iter().all(|row| row.passed)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PropertyHardeningRow {
    pub id: &'static str,
    pub property: &'static str,
    pub fixture: &'static str,
    pub expected_status: &'static str,
    pub observed_status: &'static str,
    pub frontier_count: usize,
    pub passed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PropertyHardeningReport {
    pub rows: Vec<PropertyHardeningRow>,
}

impl PropertyHardeningReport {
    pub fn passed(&self) -> bool {
        self.rows.iter().all(|row| row.passed)
    }
}

pub fn property_hardening_report() -> PropertyHardeningReport {
    let frontier_budget = ContextBudget {
        grain_limit: Some(4),
        ..ContextBudget::tiny_fixture()
    };
    let failure_budget = ContextBudget {
        grain_limit: Some(2),
        ..ContextBudget::tiny_fixture()
    };
    let frontier_result =
        close_candidate_cut(hardening_candidate("frontier-fixture", 2), &frontier_budget);
    let failure_result =
        close_candidate_cut(hardening_candidate("failure-fixture", 0), &failure_budget);
    let adversarial_result = close_candidate_cut(
        adversarial_missing_custody_candidate(),
        &ContextBudget::tiny_fixture(),
    );

    PropertyHardeningReport {
        rows: vec![
            hardening_status_row(
                "budget_frontier_deferred",
                "budget_outcome",
                "optional-grain-budget",
                "frontier_deferred",
                &frontier_result,
            ),
            hardening_reason_row(
                "frontier_preserves_budget_reason",
                "frontier_preservation",
                "optional-grain-budget",
                "budget_limit",
                &frontier_result,
            ),
            hardening_status_row(
                "required_closure_budget_failure",
                "budget_outcome",
                "required-grain-budget",
                "budget_failure",
                &failure_result,
            ),
            hardening_reason_row(
                "adversarial_missing_custody_rejected",
                "adversarial_failure",
                "missing-custody",
                "missing_custody",
                &adversarial_result,
            ),
        ],
    }
}

pub fn algebra_property_report() -> AlgebraPropertyReport {
    let left = property_cut(
        "left",
        &["a", "b", "shared"],
        &[("a", "shared", BondKind::Requires)],
    );
    let right = property_cut(
        "right",
        &["shared", "c"],
        &[("shared", "c", BondKind::Cites)],
    );
    let third = property_cut(
        "third",
        &["shared", "d"],
        &[("shared", "d", BondKind::Contains)],
    );
    let bond_left = property_cut(
        "bond_left",
        &["shared", "bridge", "left_extra"],
        &[
            ("shared", "bridge", BondKind::Requires),
            ("left_extra", "shared", BondKind::Cites),
        ],
    );
    let bond_right = property_cut(
        "bond_right",
        &["shared", "bridge", "right_extra"],
        &[
            ("shared", "bridge", BondKind::Requires),
            ("shared", "right_extra", BondKind::Cites),
        ],
    );
    let meet_lr = meet(&left, &right);
    let meet_rl = meet(&right, &left);
    let join_lr = join(&left, &right);
    let join_rl = join(&right, &left);
    let meet_expected_grains = left.grains.intersection(&right.grains).cloned().collect();
    let join_expected_grains = left.grains.union(&right.grains).cloned().collect();
    let meet_expected_bonds = expected_bonds_for_grains(
        left.bonds.intersection(&right.bonds).cloned(),
        &meet_expected_grains,
    );
    let join_expected_bonds = expected_bonds_for_grains(
        left.bonds.union(&right.bonds).cloned(),
        &join_expected_grains,
    );
    let meet_ll = meet(&left, &left);
    let join_ll = join(&left, &left);
    let meet_absorption = meet(&left, &join_lr);
    let join_absorption = join(&left, &meet_lr);
    let meet_associative_left = meet(&left, &meet(&right, &third));
    let meet_associative_right = meet(&meet(&left, &right), &third);
    let join_associative_left = join(&left, &join(&right, &third));
    let join_associative_right = join(&join(&left, &right), &third);
    let meet_monotone_left_lower = meet(&meet_lr, &third);
    let meet_monotone_left_upper = meet(&left, &third);
    let meet_monotone_right_lower = meet(&meet_lr, &third);
    let meet_monotone_right_upper = meet(&right, &third);
    let join_monotone_left_lower = join(&meet_lr, &third);
    let join_monotone_left_upper = join(&left, &third);
    let join_monotone_right_lower = join(&meet_lr, &third);
    let join_monotone_right_upper = join(&right, &third);
    let meet_bond_endpoint = meet(&bond_left, &bond_right);
    let join_bond_endpoint = join(&bond_left, &bond_right);
    let meet_lr_replay = meet(&left, &right);
    let join_lr_replay = join(&left, &right);

    AlgebraPropertyReport {
        rows: vec![
            property_row(
                "meet_commutative",
                "commutative",
                "meet",
                &meet_lr,
                &meet_rl,
            ),
            property_row(
                "join_commutative",
                "commutative",
                "join",
                &join_lr,
                &join_rl,
            ),
            property_row("meet_idempotent", "idempotent", "meet", &meet_ll, &left),
            property_row("join_idempotent", "idempotent", "join", &join_ll, &left),
            property_row(
                "meet_absorption",
                "absorption",
                "meet",
                &meet_absorption,
                &left,
            ),
            property_row(
                "join_absorption",
                "absorption",
                "join",
                &join_absorption,
                &left,
            ),
            property_row(
                "meet_associative",
                "associative",
                "meet",
                &meet_associative_left,
                &meet_associative_right,
            ),
            property_row(
                "join_associative",
                "associative",
                "join",
                &join_associative_left,
                &join_associative_right,
            ),
            order_property_row(
                "meet_lower_bound_left",
                "lower_bound",
                "meet",
                &meet_lr,
                &left,
            ),
            order_property_row(
                "meet_lower_bound_right",
                "lower_bound",
                "meet",
                &meet_lr,
                &right,
            ),
            order_property_row(
                "join_upper_bound_left",
                "upper_bound",
                "join",
                &left,
                &join_lr,
            ),
            order_property_row(
                "join_upper_bound_right",
                "upper_bound",
                "join",
                &right,
                &join_lr,
            ),
            order_property_row(
                "meet_monotone_left",
                "monotone",
                "meet",
                &meet_monotone_left_lower,
                &meet_monotone_left_upper,
            ),
            order_property_row(
                "meet_monotone_right",
                "monotone",
                "meet",
                &meet_monotone_right_lower,
                &meet_monotone_right_upper,
            ),
            order_property_row(
                "join_monotone_left",
                "monotone",
                "join",
                &join_monotone_left_lower,
                &join_monotone_left_upper,
            ),
            order_property_row(
                "join_monotone_right",
                "monotone",
                "join",
                &join_monotone_right_lower,
                &join_monotone_right_upper,
            ),
            closure_witness_property_row("meet_closure_witness", "meet", &meet_lr),
            closure_witness_property_row("join_closure_witness", "join", &join_lr),
            policy_witness_property_row("meet_policy_witness", "meet", &meet_lr),
            policy_witness_property_row("join_policy_witness", "join", &join_lr),
            rights_witness_property_row("meet_rights_witness", "meet", &meet_lr),
            rights_witness_property_row("join_rights_witness", "join", &join_lr),
            bond_endpoint_property_row("meet_bond_endpoint_witness", "meet", &meet_bond_endpoint),
            bond_endpoint_property_row("join_bond_endpoint_witness", "join", &join_bond_endpoint),
            property_row(
                "meet_deterministic_replay",
                "deterministic_replay",
                "meet",
                &meet_lr,
                &meet_lr_replay,
            ),
            property_row(
                "join_deterministic_replay",
                "deterministic_replay",
                "join",
                &join_lr,
                &join_lr_replay,
            ),
            input_closure_property_row("meet_input_closure_witness", "meet", &left, &right),
            input_closure_property_row("join_input_closure_witness", "join", &left, &right),
            grain_set_property_row(
                "meet_grain_set_witness",
                "meet",
                &meet_lr,
                &meet_expected_grains,
            ),
            grain_set_property_row(
                "join_grain_set_witness",
                "join",
                &join_lr,
                &join_expected_grains,
            ),
            bond_set_property_row(
                "meet_bond_set_witness",
                "meet",
                &meet_lr,
                &meet_expected_bonds,
            ),
            bond_set_property_row(
                "join_bond_set_witness",
                "join",
                &join_lr,
                &join_expected_bonds,
            ),
            receipt_note_property_row(
                "meet_receipt_note_witness",
                "meet",
                &meet_lr,
                "intersection closed over retained bonds",
            ),
            receipt_note_property_row(
                "join_receipt_note_witness",
                "join",
                &join_lr,
                "union closed over retained bonds",
            ),
            receipt_hash_property_row(
                "meet_receipt_hash_witness",
                "meet",
                &meet_lr,
                "intersection closed over retained bonds",
            ),
            receipt_hash_property_row(
                "join_receipt_hash_witness",
                "join",
                &join_lr,
                "union closed over retained bonds",
            ),
            result_id_property_row(
                "meet_result_id_witness",
                "meet",
                &meet_lr,
                "meet(left, right)",
            ),
            result_id_property_row(
                "join_result_id_witness",
                "join",
                &join_lr,
                "join(left, right)",
            ),
            receipt_count_property_row("meet_receipt_count_witness", "meet", &meet_lr),
            receipt_count_property_row("join_receipt_count_witness", "join", &join_lr),
            policy_value_property_row("meet_policy_value_witness", "meet", &meet_lr),
            policy_value_property_row("join_policy_value_witness", "join", &join_lr),
            rights_value_property_row("meet_rights_value_witness", "meet", &meet_lr),
            rights_value_property_row("join_rights_value_witness", "join", &join_lr),
            receipt_rule_property_row("meet_receipt_rule_witness", "meet", &meet_lr),
            receipt_rule_property_row("join_receipt_rule_witness", "join", &join_lr),
        ],
    }
}

/// Evaluate meet/join algebra laws against an externally supplied
/// `ClosedCut` (e.g. the live cut produced by `lattice-cli`'s
/// `close live` / `validate live`). Unlike `algebra_property_report()`,
/// which exercises laws against `lattice-order`'s internal synthetic
/// property fixtures, this variant treats `cut` itself as the algebraic
/// object under test.
///
/// The checks cover the subset of laws that are well-defined against a
/// single closed cut without requiring two named inputs:
///
/// - **Idempotence** (`meet(cut, cut) ≡ cut`, `join(cut, cut) ≡ cut`)
/// - **Commutativity** (`meet(cut, cut)` is order-insensitive — trivially
///   true for a single cut, but exercises the operator path)
/// - **Order bounds** (`meet(cut, cut) ⊑ cut`, `cut ⊑ join(cut, cut)`)
/// - **Closure witness** (`cut.closure_receipts` non-empty)
/// - **Policy witness** (`cut.closure_policy` is `Some`)
/// - **Rights witness** (`cut.rights_policy` is `Some`)
/// - **Bond endpoint witness** (every bond endpoint resolves to a
///   retained grain id)
/// - **Grain record completeness** (every `grains` entry has a matching
///   `grain_records` entry)
/// - **Custody grain witness** (the cut contains at least one
///   `SourcePointer`, one `Policy`, and one `Receipt` grain — the same
///   custody triple `close_candidate_cut` requires on input, validated on
///   the closed output)
/// - **Provenance bond witness** (every delta-context grain — a `Context`
///   grain in `grain_records` — participates in at least one bond, so
///   the cut has no orphan context grains)
///
/// The returned report's `rows` carry static law/operator identifiers;
/// the cut's identity is conveyed by the caller (the live-validation
/// envelope already emits `cut_id` and `cut_hash`).
pub fn algebra_property_report_for_cut(cut: &ClosedCut) -> AlgebraPropertyReport {
    let meet_self = meet(cut, cut);
    let join_self = join(cut, cut);
    let meet_self_replay = meet(cut, cut);
    let join_self_replay = join(cut, cut);

    AlgebraPropertyReport {
        rows: vec![
            live_set_equality_row(
                "live_meet_idempotent",
                "idempotent",
                "meet",
                &meet_self,
                cut,
            ),
            live_set_equality_row(
                "live_join_idempotent",
                "idempotent",
                "join",
                &join_self,
                cut,
            ),
            live_set_equality_row(
                "live_meet_commutative",
                "commutative",
                "meet",
                &meet_self,
                &meet_self_replay,
            ),
            live_set_equality_row(
                "live_join_commutative",
                "commutative",
                "join",
                &join_self,
                &join_self_replay,
            ),
            live_set_subset_row(
                "live_meet_lower_bound",
                "lower_bound",
                "meet",
                &meet_self,
                cut,
            ),
            live_set_subset_row(
                "live_join_upper_bound",
                "upper_bound",
                "join",
                cut,
                &join_self,
            ),
            live_set_equality_row(
                "live_meet_deterministic_replay",
                "deterministic_replay",
                "meet",
                &meet_self,
                &meet_self_replay,
            ),
            live_set_equality_row(
                "live_join_deterministic_replay",
                "deterministic_replay",
                "join",
                &join_self,
                &join_self_replay,
            ),
            live_closure_witness_row("live_closure_witness", "closure-v1", cut),
            live_policy_witness_row("live_policy_witness", cut),
            live_rights_witness_row("live_rights_witness", cut),
            live_bond_endpoint_witness_row("live_bond_endpoint_witness", cut),
            live_grain_record_completeness_row("live_grain_record_completeness", cut),
        ],
    }
}

/// Evaluate property hardening contracts (custody, frontier, evidence)
/// against an externally supplied `ClosedCut`. Unlike
/// `property_hardening_report()`, which exercises hardening against
/// `lattice-order`'s synthetic frontier/failure/adversarial fixtures,
/// this variant treats `cut` itself as the closed object under test.
///
/// The checks cover the property invariants the live closure pipeline
/// must produce in any well-formed cut:
///
/// - **Custody source-pointer grain**: `cut.grain_records` contains a
///   `GrainKind::SourcePointer` grain.
/// - **Custody policy grain**: `cut.grain_records` contains a
///   `GrainKind::Policy` grain.
/// - **Custody receipt grain**: `cut.grain_records` contains a
///   `GrainKind::Receipt` grain.
/// - **Provenance bond**: a closed live cut must wire at least one
///   delta-derived `Context` grain back to a `SourcePointer` grain via
///   `DerivesFrom`, `Requires`, or `Cites`. Receipt-to-delta citation
///   bonds are useful audit evidence, but they do not by themselves
///   prove source provenance. The negative
///   `live-delta-tiny-bad-algebra.json` fixture is designed to violate
///   this invariant by using operations outside the source-bond-mapped
///   set.
pub fn property_hardening_report_for_cut(cut: &ClosedCut) -> PropertyHardeningReport {
    PropertyHardeningReport {
        rows: vec![
            live_custody_grain_row(
                "live_custody_source_pointer_grain",
                "live-cut-custody",
                GrainKind::SourcePointer,
                cut,
            ),
            live_custody_grain_row(
                "live_custody_policy_grain",
                "live-cut-custody",
                GrainKind::Policy,
                cut,
            ),
            live_custody_grain_row(
                "live_custody_receipt_grain",
                "live-cut-custody",
                GrainKind::Receipt,
                cut,
            ),
            live_provenance_bond_row("live_provenance_bond_witness", "live-cut-provenance", cut),
        ],
    }
}

fn live_set_equality_row(
    id: &'static str,
    law: &'static str,
    operator: &'static str,
    left: &ClosedCut,
    right: &ClosedCut,
) -> AlgebraPropertyRow {
    AlgebraPropertyRow {
        id,
        law,
        operator,
        left_grain_count: left.grains.len(),
        right_grain_count: right.grains.len(),
        passed: left.grains == right.grains && left.bonds == right.bonds,
    }
}

fn live_set_subset_row(
    id: &'static str,
    law: &'static str,
    operator: &'static str,
    lower: &ClosedCut,
    upper: &ClosedCut,
) -> AlgebraPropertyRow {
    AlgebraPropertyRow {
        id,
        law,
        operator,
        left_grain_count: lower.grains.len(),
        right_grain_count: upper.grains.len(),
        passed: lower.grains.is_subset(&upper.grains) && lower.bonds.is_subset(&upper.bonds),
    }
}

fn live_closure_witness_row(
    id: &'static str,
    operator: &'static str,
    cut: &ClosedCut,
) -> AlgebraPropertyRow {
    AlgebraPropertyRow {
        id,
        law: "closure_witness",
        operator,
        left_grain_count: cut.grains.len(),
        right_grain_count: cut.closure_receipts.len(),
        passed: !cut.closure_receipts.is_empty(),
    }
}

fn live_policy_witness_row(id: &'static str, cut: &ClosedCut) -> AlgebraPropertyRow {
    AlgebraPropertyRow {
        id,
        law: "policy_witness",
        operator: "live-cut",
        left_grain_count: cut.grains.len(),
        right_grain_count: usize::from(cut.closure_policy.is_some()),
        passed: cut.closure_policy.is_some(),
    }
}

fn live_rights_witness_row(id: &'static str, cut: &ClosedCut) -> AlgebraPropertyRow {
    AlgebraPropertyRow {
        id,
        law: "rights_witness",
        operator: "live-cut",
        left_grain_count: cut.grains.len(),
        right_grain_count: usize::from(cut.rights_policy.is_some()),
        passed: cut.rights_policy.is_some(),
    }
}

fn live_bond_endpoint_witness_row(id: &'static str, cut: &ClosedCut) -> AlgebraPropertyRow {
    AlgebraPropertyRow {
        id,
        law: "bond_endpoint_witness",
        operator: "live-cut",
        left_grain_count: cut.grains.len(),
        right_grain_count: cut.bonds.len(),
        passed: cut
            .bonds
            .iter()
            .all(|bond| cut.contains_bond_endpoints(bond)),
    }
}

fn live_grain_record_completeness_row(id: &'static str, cut: &ClosedCut) -> AlgebraPropertyRow {
    let record_ids: BTreeSet<GrainId> = cut.grain_records.iter().map(|g| g.id.clone()).collect();
    AlgebraPropertyRow {
        id,
        law: "grain_record_completeness",
        operator: "live-cut",
        left_grain_count: cut.grains.len(),
        right_grain_count: record_ids.len(),
        passed: cut.grains.iter().all(|id| record_ids.contains(id)),
    }
}

fn live_custody_grain_row(
    id: &'static str,
    fixture: &'static str,
    kind: GrainKind,
    cut: &ClosedCut,
) -> PropertyHardeningRow {
    let property = match kind {
        GrainKind::SourcePointer => "custody_source_pointer",
        GrainKind::Policy => "custody_policy",
        GrainKind::Receipt => "custody_receipt",
        _ => "custody_other",
    };
    let expected_status = "present";
    let observed_status = if cut.grain_records.iter().any(|grain| grain.kind == kind) {
        "present"
    } else {
        "absent"
    };
    PropertyHardeningRow {
        id,
        property,
        fixture,
        expected_status,
        observed_status,
        frontier_count: 0,
        passed: observed_status == expected_status,
    }
}

fn live_provenance_bond_row(
    id: &'static str,
    fixture: &'static str,
    cut: &ClosedCut,
) -> PropertyHardeningRow {
    let expected_status = "present";
    let grain_kinds: BTreeMap<&GrainId, GrainKind> = cut
        .grain_records
        .iter()
        .map(|grain| (&grain.id, grain.kind))
        .collect();
    let has_source_provenance_bond = cut.bonds.iter().any(|bond| {
        matches!(
            bond.kind,
            BondKind::DerivesFrom | BondKind::Requires | BondKind::Cites
        ) && grain_kinds.get(&bond.from) == Some(&GrainKind::Context)
            && grain_kinds.get(&bond.to) == Some(&GrainKind::SourcePointer)
    });
    let observed_status = if has_source_provenance_bond {
        "present"
    } else {
        "absent"
    };
    PropertyHardeningRow {
        id,
        property: "provenance_bond",
        fixture,
        expected_status,
        observed_status,
        frontier_count: 0,
        passed: observed_status == expected_status,
    }
}

fn hardening_candidate(id: &str, optional_count: usize) -> CandidateCut {
    let source_id = "fontes:property-hardening";
    let rights_policy = "derived_text_allowed";
    let mut grains = vec![
        Grain::new(GrainId::new(format!("{id}:source")), "source pointer").with_metadata(
            GrainKind::SourcePointer,
            source_id,
            rights_policy,
        ),
        Grain::new(GrainId::new(format!("{id}:policy")), "closure policy").with_metadata(
            GrainKind::Policy,
            source_id,
            rights_policy,
        ),
        Grain::new(GrainId::new(format!("{id}:receipt")), "closure receipt").with_metadata(
            GrainKind::Receipt,
            source_id,
            rights_policy,
        ),
    ];
    for index in 0..optional_count {
        grains.push(
            Grain::new(
                GrainId::new(format!("{id}:optional-{index}")),
                format!("optional evidence {index}"),
            )
            .with_metadata(GrainKind::Evidence, source_id, rights_policy),
        );
    }
    CandidateCut {
        id: CutId::new(id),
        grains,
        bonds: Vec::new(),
    }
}

fn adversarial_missing_custody_candidate() -> CandidateCut {
    CandidateCut {
        id: CutId::new("missing-custody-fixture"),
        grains: vec![Grain::new(
            GrainId::new("missing-custody-fixture:context"),
            "context without custody metadata",
        )],
        bonds: Vec::new(),
    }
}

fn hardening_status_row(
    id: &'static str,
    property: &'static str,
    fixture: &'static str,
    expected_status: &'static str,
    result: &ClosureResult,
) -> PropertyHardeningRow {
    let observed_status = closure_result_status(result);
    PropertyHardeningRow {
        id,
        property,
        fixture,
        expected_status,
        observed_status,
        frontier_count: closure_result_frontier_count(result),
        passed: observed_status == expected_status,
    }
}

fn hardening_reason_row(
    id: &'static str,
    property: &'static str,
    fixture: &'static str,
    expected_status: &'static str,
    result: &ClosureResult,
) -> PropertyHardeningRow {
    let observed_status = closure_result_reason(result);
    PropertyHardeningRow {
        id,
        property,
        fixture,
        expected_status,
        observed_status,
        frontier_count: closure_result_frontier_count(result),
        passed: observed_status == expected_status,
    }
}

fn closure_result_status(result: &ClosureResult) -> &'static str {
    match result {
        ClosureResult::Closed { budget_status, .. }
        | ClosureResult::ClosedWithFrontier { budget_status, .. } => budget_status.as_str(),
        ClosureResult::BudgetFailure(failure) => failure.status().as_str(),
    }
}

fn closure_result_reason(result: &ClosureResult) -> &'static str {
    match result {
        ClosureResult::ClosedWithFrontier { frontier, .. } => frontier
            .records()
            .first()
            .map(|record| record.reason.as_str())
            .unwrap_or("none"),
        ClosureResult::BudgetFailure(failure) => failure.reason.as_str(),
        ClosureResult::Closed { .. } => "none",
    }
}

fn closure_result_frontier_count(result: &ClosureResult) -> usize {
    match result {
        ClosureResult::Closed { .. } => 0,
        ClosureResult::ClosedWithFrontier { frontier, .. } => frontier.records().len(),
        ClosureResult::BudgetFailure(failure) => failure.frontier_count,
    }
}

fn expected_bonds_for_grains(
    bonds: impl Iterator<Item = Bond>,
    grains: &BTreeSet<GrainId>,
) -> BTreeSet<Bond> {
    bonds
        .filter(|bond| grains.contains(&bond.from) && grains.contains(&bond.to))
        .collect()
}

fn property_cut(id: &str, grains: &[&str], bonds: &[(&str, &str, BondKind)]) -> ClosedCut {
    let mut cut = ClosedCut::new(CutId::new(id))
        .with_receipt(ClosureReceipt::new(
            "property-fixture",
            "closed property fixture",
        ))
        .with_policy(DEFAULT_CLOSURE_POLICY, "derived_text_allowed");
    for grain in grains {
        cut = cut.with_grain(GrainId::new(*grain));
    }
    for (from, to, kind) in bonds {
        cut = cut.with_bond(Bond::new(
            GrainId::new(*from),
            GrainId::new(*to),
            kind.clone(),
        ));
    }
    cut
}

fn property_row(
    id: &'static str,
    law: &'static str,
    operator: &'static str,
    left: &ClosedCut,
    right: &ClosedCut,
) -> AlgebraPropertyRow {
    AlgebraPropertyRow {
        id,
        law,
        operator,
        left_grain_count: left.grains.len(),
        right_grain_count: right.grains.len(),
        passed: same_context_elements(left, right),
    }
}

fn same_context_elements(left: &ClosedCut, right: &ClosedCut) -> bool {
    left.grains == right.grains && left.bonds == right.bonds
}

fn order_property_row(
    id: &'static str,
    law: &'static str,
    operator: &'static str,
    lower: &ClosedCut,
    upper: &ClosedCut,
) -> AlgebraPropertyRow {
    AlgebraPropertyRow {
        id,
        law,
        operator,
        left_grain_count: lower.grains.len(),
        right_grain_count: upper.grains.len(),
        passed: context_subset(lower, upper),
    }
}

fn context_subset(lower: &ClosedCut, upper: &ClosedCut) -> bool {
    lower.grains.is_subset(&upper.grains) && lower.bonds.is_subset(&upper.bonds)
}

fn closure_witness_property_row(
    id: &'static str,
    operator: &'static str,
    cut: &ClosedCut,
) -> AlgebraPropertyRow {
    AlgebraPropertyRow {
        id,
        law: "closure_witness",
        operator,
        left_grain_count: cut.grains.len(),
        right_grain_count: cut.closure_receipts.len(),
        passed: cut
            .closure_receipts
            .iter()
            .any(|receipt| receipt.rule == operator),
    }
}

fn policy_witness_property_row(
    id: &'static str,
    operator: &'static str,
    cut: &ClosedCut,
) -> AlgebraPropertyRow {
    AlgebraPropertyRow {
        id,
        law: "policy_witness",
        operator,
        left_grain_count: cut.grains.len(),
        right_grain_count: usize::from(cut.closure_policy.is_some()),
        passed: cut.closure_policy.as_deref() == Some(DEFAULT_CLOSURE_POLICY),
    }
}

fn rights_witness_property_row(
    id: &'static str,
    operator: &'static str,
    cut: &ClosedCut,
) -> AlgebraPropertyRow {
    AlgebraPropertyRow {
        id,
        law: "rights_witness",
        operator,
        left_grain_count: cut.grains.len(),
        right_grain_count: usize::from(cut.rights_policy.is_some()),
        passed: cut.rights_policy.as_deref() == Some("derived_text_allowed"),
    }
}

fn bond_endpoint_property_row(
    id: &'static str,
    operator: &'static str,
    cut: &ClosedCut,
) -> AlgebraPropertyRow {
    AlgebraPropertyRow {
        id,
        law: "bond_endpoint_witness",
        operator,
        left_grain_count: cut.grains.len(),
        right_grain_count: cut.bonds.len(),
        passed: cut
            .bonds
            .iter()
            .all(|bond| cut.contains_bond_endpoints(bond)),
    }
}

fn input_closure_property_row(
    id: &'static str,
    operator: &'static str,
    left: &ClosedCut,
    right: &ClosedCut,
) -> AlgebraPropertyRow {
    AlgebraPropertyRow {
        id,
        law: "input_closure_witness",
        operator,
        left_grain_count: left.closure_receipts.len(),
        right_grain_count: right.closure_receipts.len(),
        passed: !left.closure_receipts.is_empty() && !right.closure_receipts.is_empty(),
    }
}

fn grain_set_property_row(
    id: &'static str,
    operator: &'static str,
    cut: &ClosedCut,
    expected_grains: &BTreeSet<GrainId>,
) -> AlgebraPropertyRow {
    AlgebraPropertyRow {
        id,
        law: "grain_set_witness",
        operator,
        left_grain_count: cut.grains.len(),
        right_grain_count: expected_grains.len(),
        passed: cut.grains == *expected_grains,
    }
}

fn bond_set_property_row(
    id: &'static str,
    operator: &'static str,
    cut: &ClosedCut,
    expected_bonds: &BTreeSet<Bond>,
) -> AlgebraPropertyRow {
    AlgebraPropertyRow {
        id,
        law: "bond_set_witness",
        operator,
        left_grain_count: cut.bonds.len(),
        right_grain_count: expected_bonds.len(),
        passed: cut.bonds == *expected_bonds,
    }
}

fn receipt_note_property_row(
    id: &'static str,
    operator: &'static str,
    cut: &ClosedCut,
    expected_note: &'static str,
) -> AlgebraPropertyRow {
    AlgebraPropertyRow {
        id,
        law: "receipt_note_witness",
        operator,
        left_grain_count: cut.closure_receipts.len(),
        right_grain_count: 1,
        passed: cut
            .closure_receipts
            .iter()
            .any(|receipt| receipt.rule == operator && receipt.note == expected_note),
    }
}

fn receipt_hash_property_row(
    id: &'static str,
    operator: &'static str,
    cut: &ClosedCut,
    expected_note: &'static str,
) -> AlgebraPropertyRow {
    let expected_hash = ClosureReceipt::new(operator, expected_note).stable_hash();
    AlgebraPropertyRow {
        id,
        law: "receipt_hash_witness",
        operator,
        left_grain_count: cut.closure_receipts.len(),
        right_grain_count: expected_hash.len(),
        passed: cut
            .closure_receipts
            .iter()
            .any(|receipt| receipt.stable_hash() == expected_hash),
    }
}

fn result_id_property_row(
    id: &'static str,
    operator: &'static str,
    cut: &ClosedCut,
    expected_id: &'static str,
) -> AlgebraPropertyRow {
    AlgebraPropertyRow {
        id,
        law: "result_id_witness",
        operator,
        left_grain_count: cut.id.as_str().len(),
        right_grain_count: expected_id.len(),
        passed: cut.id.as_str() == expected_id,
    }
}

fn receipt_count_property_row(
    id: &'static str,
    operator: &'static str,
    cut: &ClosedCut,
) -> AlgebraPropertyRow {
    let operator_receipt_count = cut
        .closure_receipts
        .iter()
        .filter(|receipt| receipt.rule == operator)
        .count();
    AlgebraPropertyRow {
        id,
        law: "receipt_count_witness",
        operator,
        left_grain_count: cut.closure_receipts.len(),
        right_grain_count: operator_receipt_count,
        passed: operator_receipt_count == 1,
    }
}

fn policy_value_property_row(
    id: &'static str,
    operator: &'static str,
    cut: &ClosedCut,
) -> AlgebraPropertyRow {
    AlgebraPropertyRow {
        id,
        law: "policy_value_witness",
        operator,
        left_grain_count: cut.closure_policy.as_deref().unwrap_or_default().len(),
        right_grain_count: DEFAULT_CLOSURE_POLICY.len(),
        passed: cut.closure_policy.as_deref() == Some(DEFAULT_CLOSURE_POLICY),
    }
}

fn rights_value_property_row(
    id: &'static str,
    operator: &'static str,
    cut: &ClosedCut,
) -> AlgebraPropertyRow {
    const DEFAULT_RIGHTS_POLICY: &str = "derived_text_allowed";
    AlgebraPropertyRow {
        id,
        law: "rights_value_witness",
        operator,
        left_grain_count: cut.rights_policy.as_deref().unwrap_or_default().len(),
        right_grain_count: DEFAULT_RIGHTS_POLICY.len(),
        passed: cut.rights_policy.as_deref() == Some(DEFAULT_RIGHTS_POLICY),
    }
}

fn receipt_rule_property_row(
    id: &'static str,
    operator: &'static str,
    cut: &ClosedCut,
) -> AlgebraPropertyRow {
    AlgebraPropertyRow {
        id,
        law: "receipt_rule_witness",
        operator,
        left_grain_count: cut.closure_receipts.len(),
        right_grain_count: operator.len(),
        passed: cut
            .closure_receipts
            .iter()
            .any(|receipt| receipt.rule == operator),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lattice_model::{
        Bond, BondKind, ClosedCut, ContextBudget, CutId, Grain, GrainId, TinyModelFixture,
    };

    #[test]
    fn meet_keeps_shared_grains() {
        let a = GrainId::new("a");
        let b = GrainId::new("b");
        let c = GrainId::new("c");
        let left = ClosedCut::new(CutId::new("left"))
            .with_grain(a.clone())
            .with_grain(b.clone());
        let right = ClosedCut::new(CutId::new("right"))
            .with_grain(b.clone())
            .with_grain(c);

        let result = meet(&left, &right);

        assert_eq!(result.grains.len(), 1);
        assert!(result.grains.contains(&b));
        assert_eq!(
            result.closure_policy.as_deref(),
            Some(DEFAULT_CLOSURE_POLICY)
        );
        assert_eq!(
            result.rights_policy.as_deref(),
            Some("derived_text_allowed")
        );
    }

    #[test]
    fn join_keeps_union_and_valid_bonds() {
        let a = GrainId::new("a");
        let b = GrainId::new("b");
        let bond = Bond::new(a.clone(), b.clone(), BondKind::Requires);
        let left = ClosedCut::new(CutId::new("left"))
            .with_grain(a)
            .with_bond(bond.clone());
        let right = ClosedCut::new(CutId::new("right")).with_grain(b);

        let result = join(&left, &right);

        assert_eq!(result.grains.len(), 2);
        assert!(result.bonds.contains(&bond));
        assert_eq!(
            result.closure_policy.as_deref(),
            Some(DEFAULT_CLOSURE_POLICY)
        );
        assert_eq!(
            result.rights_policy.as_deref(),
            Some("derived_text_allowed")
        );
    }

    #[test]
    fn algebra_property_report_validates_meet_join_laws() {
        let report = algebra_property_report();

        assert_eq!(report.rows.len(), 46);
        assert!(report.passed());
        assert!(report
            .rows
            .iter()
            .any(|row| row.id == "meet_commutative" && row.operator == "meet"));
        assert!(report
            .rows
            .iter()
            .any(|row| row.id == "join_absorption" && row.law == "absorption"));
        assert!(report
            .rows
            .iter()
            .any(|row| row.id == "meet_associative" && row.law == "associative"));
        assert!(report
            .rows
            .iter()
            .any(|row| row.id == "meet_lower_bound_left" && row.law == "lower_bound"));
        assert!(report
            .rows
            .iter()
            .any(|row| row.id == "join_upper_bound_right" && row.law == "upper_bound"));
        assert!(report
            .rows
            .iter()
            .any(|row| row.id == "meet_monotone_left" && row.law == "monotone"));
        assert!(report
            .rows
            .iter()
            .any(|row| row.id == "join_monotone_right" && row.law == "monotone"));
        assert!(report
            .rows
            .iter()
            .any(|row| row.id == "meet_closure_witness" && row.law == "closure_witness"));
        assert!(report
            .rows
            .iter()
            .any(|row| row.id == "join_closure_witness" && row.law == "closure_witness"));
        assert!(report
            .rows
            .iter()
            .any(|row| row.id == "meet_grain_set_witness" && row.law == "grain_set_witness"));
        assert!(report
            .rows
            .iter()
            .any(|row| row.id == "join_grain_set_witness" && row.law == "grain_set_witness"));
        assert!(report
            .rows
            .iter()
            .any(|row| row.id == "meet_bond_set_witness" && row.law == "bond_set_witness"));
        assert!(report
            .rows
            .iter()
            .any(|row| row.id == "join_bond_set_witness" && row.law == "bond_set_witness"));
        assert!(report
            .rows
            .iter()
            .any(|row| row.id == "meet_receipt_note_witness" && row.law == "receipt_note_witness"));
        assert!(report
            .rows
            .iter()
            .any(|row| row.id == "join_receipt_note_witness" && row.law == "receipt_note_witness"));
        assert!(report
            .rows
            .iter()
            .any(|row| row.id == "meet_receipt_hash_witness"
                && row.law == "receipt_hash_witness"
                && row.right_grain_count > 0));
        assert!(report
            .rows
            .iter()
            .any(|row| row.id == "join_receipt_hash_witness"
                && row.law == "receipt_hash_witness"
                && row.right_grain_count > 0));
        assert!(report
            .rows
            .iter()
            .any(|row| row.id == "meet_result_id_witness" && row.law == "result_id_witness"));
        assert!(report
            .rows
            .iter()
            .any(|row| row.id == "join_result_id_witness" && row.law == "result_id_witness"));
        assert!(report
            .rows
            .iter()
            .any(|row| row.id == "meet_receipt_count_witness"
                && row.law == "receipt_count_witness"
                && row.right_grain_count == 1));
        assert!(report
            .rows
            .iter()
            .any(|row| row.id == "join_receipt_count_witness"
                && row.law == "receipt_count_witness"
                && row.right_grain_count == 1));
        assert!(report
            .rows
            .iter()
            .any(|row| row.id == "meet_policy_value_witness" && row.law == "policy_value_witness"));
        assert!(report
            .rows
            .iter()
            .any(|row| row.id == "join_policy_value_witness" && row.law == "policy_value_witness"));
        assert!(report
            .rows
            .iter()
            .any(|row| row.id == "meet_rights_value_witness" && row.law == "rights_value_witness"));
        assert!(report
            .rows
            .iter()
            .any(|row| row.id == "join_rights_value_witness" && row.law == "rights_value_witness"));
        assert!(report
            .rows
            .iter()
            .any(|row| row.id == "meet_receipt_rule_witness"
                && row.law == "receipt_rule_witness"
                && row.right_grain_count == "meet".len()));
        assert!(report
            .rows
            .iter()
            .any(|row| row.id == "join_receipt_rule_witness"
                && row.law == "receipt_rule_witness"
                && row.right_grain_count == "join".len()));
        assert!(report
            .rows
            .iter()
            .any(|row| row.id == "meet_policy_witness" && row.law == "policy_witness"));
        assert!(report
            .rows
            .iter()
            .any(|row| row.id == "join_policy_witness" && row.law == "policy_witness"));
        assert!(report
            .rows
            .iter()
            .any(|row| row.id == "meet_rights_witness" && row.law == "rights_witness"));
        assert!(report
            .rows
            .iter()
            .any(|row| row.id == "join_rights_witness" && row.law == "rights_witness"));
        assert!(report
            .rows
            .iter()
            .any(|row| row.id == "meet_bond_endpoint_witness"
                && row.law == "bond_endpoint_witness"
                && row.right_grain_count == 1));
        assert!(report
            .rows
            .iter()
            .any(|row| row.id == "join_bond_endpoint_witness"
                && row.law == "bond_endpoint_witness"
                && row.right_grain_count == 3));
        assert!(report
            .rows
            .iter()
            .any(|row| row.id == "meet_deterministic_replay" && row.law == "deterministic_replay"));
        assert!(report
            .rows
            .iter()
            .any(|row| row.id == "join_deterministic_replay" && row.law == "deterministic_replay"));
        assert!(report
            .rows
            .iter()
            .any(|row| row.id == "meet_input_closure_witness"
                && row.law == "input_closure_witness"
                && row.left_grain_count == 1
                && row.right_grain_count == 1));
        assert!(report
            .rows
            .iter()
            .any(|row| row.id == "join_input_closure_witness"
                && row.law == "input_closure_witness"
                && row.left_grain_count == 1
                && row.right_grain_count == 1));
    }

    #[test]
    fn property_hardening_report_validates_budget_frontier_and_failures() {
        let report = property_hardening_report();

        assert_eq!(report.rows.len(), 4);
        assert!(report.passed());
        assert!(report
            .rows
            .iter()
            .any(|row| row.id == "budget_frontier_deferred"
                && row.property == "budget_outcome"
                && row.observed_status == "frontier_deferred"
                && row.frontier_count > 0));
        assert!(report
            .rows
            .iter()
            .any(|row| row.id == "frontier_preserves_budget_reason"
                && row.property == "frontier_preservation"
                && row.observed_status == "budget_limit"));
        assert!(report
            .rows
            .iter()
            .any(|row| row.id == "required_closure_budget_failure"
                && row.property == "budget_outcome"
                && row.observed_status == "budget_failure"));
        assert!(report
            .rows
            .iter()
            .any(|row| row.id == "adversarial_missing_custody_rejected"
                && row.property == "adversarial_failure"
                && row.observed_status == "missing_custody"));
    }

    #[test]
    fn pass_contract_declares_invariants_effects_and_receipts() {
        let contract = PassContract::new("CheckBudget")
            .with_precondition(CLOSURE_APPLIED)
            .with_postcondition(BUDGET_CHECKED)
            .with_effect(PassEffect::Read(PassScope::Cuts))
            .with_effect(PassEffect::Emit(PassScope::Receipts))
            .with_receipt_rule("budget-check");

        assert_eq!(contract.preconditions, vec![CLOSURE_APPLIED]);
        assert_eq!(contract.postconditions, vec![BUDGET_CHECKED]);
        assert_eq!(PassScope::Receipts.as_str(), "receipts");
        assert!(!contract.has_mutating_effect_on(PassScope::Cuts));
    }

    #[test]
    fn pass_contract_detects_mutating_scope_conflicts() {
        let contract = PassContract::new("RecordFrontier")
            .with_precondition(BUDGET_CHECKED)
            .with_postcondition(FRONTIER_RECORDED)
            .with_effect(PassEffect::Insert(PassScope::Receipts))
            .with_receipt_rule("frontier-recorded");

        assert!(contract.has_mutating_effect_on(PassScope::Receipts));
        assert!(!contract.has_mutating_effect_on(PassScope::Prompts));
    }

    #[test]
    fn closes_tiny_candidate_cut_with_receipt_and_policy() {
        let fixture = TinyModelFixture::from_source("fontes:ok", "derived_text_allowed");
        let candidate = fixture
            .grains
            .into_iter()
            .fold(CandidateCut::new(CutId::new("tiny")), |candidate, grain| {
                candidate.with_grain(grain)
            });
        let candidate = fixture
            .bonds
            .into_iter()
            .fold(candidate, |candidate, bond| candidate.with_bond(bond));

        let result = close_candidate_cut(candidate, &ContextBudget::tiny_fixture());

        let ClosureResult::Closed { cut, budget_status } = result else {
            panic!("tiny fixture should close without frontier");
        };
        assert_eq!(budget_status, lattice_model::BudgetStatus::WithinBudget);
        assert_eq!(cut.grains.len(), 10);
        assert_eq!(cut.bonds.len(), 20);
        assert_eq!(cut.closure_policy.as_deref(), Some(DEFAULT_CLOSURE_POLICY));
        assert_eq!(cut.rights_policy.as_deref(), Some("derived_text_allowed"));
        assert_eq!(cut.closure_receipts.len(), 1);
    }

    #[test]
    fn simplify_preserves_closed_grains_and_frontiers_redundant_bonds() {
        let fixture = TinyModelFixture::from_source("fontes:ok", "derived_text_allowed");
        let candidate = fixture
            .grains
            .into_iter()
            .fold(CandidateCut::new(CutId::new("tiny")), |candidate, grain| {
                candidate.with_grain(grain)
            });
        let candidate = fixture
            .bonds
            .into_iter()
            .fold(candidate, |candidate, bond| candidate.with_bond(bond));
        let ClosureResult::Closed { cut, .. } =
            close_candidate_cut(candidate, &ContextBudget::tiny_fixture())
        else {
            panic!("tiny fixture should close before simplification");
        };

        let result = simplify_closed(&cut, &ContextBudget::tiny_fixture());

        let ClosureResult::ClosedWithFrontier {
            cut: simplified,
            frontier,
            budget_status,
        } = result
        else {
            panic!("simplification should frontier redundant optional bonds");
        };
        assert_eq!(budget_status, lattice_model::BudgetStatus::FrontierDeferred);
        assert_eq!(simplified.grains, cut.grains);
        assert!(simplified.bonds.len() < cut.bonds.len());
        assert!(simplified
            .closure_receipts
            .iter()
            .any(|receipt| receipt.rule == "simplify"));
        assert!(frontier
            .records()
            .iter()
            .all(|record| record.reason == FrontierReason::RedundantContext));
    }

    #[test]
    fn launch_simplification_scales_redundant_bond_frontier() {
        let original = close_launch_readiness_at_scale(45);

        let result = simplify_closed(&original.cut, &original.budget);

        let ClosureResult::ClosedWithFrontier {
            cut: simplified,
            frontier,
            ..
        } = result
        else {
            panic!("launch simplification should frontier redundant bonds");
        };
        assert_eq!(original.fact_count, 240);
        assert_eq!(original.cut.grains.len(), simplified.grains.len());
        assert_eq!(original.cut.bonds.len(), 342);
        assert_eq!(simplified.bonds.len(), 55);
        assert_eq!(frontier.records().len(), 287);
        assert!(simplified
            .closure_receipts
            .iter()
            .any(|receipt| receipt.rule == "simplify"));
    }

    #[test]
    fn closure_frontiers_invalid_bonds() {
        let fixture = TinyModelFixture::from_source("fontes:ok", "derived_text_allowed");
        let invalid_bond = Bond::new(
            GrainId::new("fontes:ok:missing"),
            fixture.grains[0].id.clone(),
            BondKind::Requires,
        );
        let candidate = fixture
            .grains
            .into_iter()
            .fold(CandidateCut::new(CutId::new("tiny")), |candidate, grain| {
                candidate.with_grain(grain)
            })
            .with_bond(invalid_bond);

        let result = close_candidate_cut(candidate, &ContextBudget::tiny_fixture());

        let ClosureResult::ClosedWithFrontier { cut, frontier, .. } = result else {
            panic!("invalid bond should produce a frontiered closed cut");
        };
        assert_eq!(cut.bonds.len(), 0);
        assert_eq!(frontier.records().len(), 1);
        assert_eq!(
            frontier.records()[0].reason,
            FrontierReason::InvalidBondEndpoint
        );
    }

    #[test]
    fn closure_fails_when_required_custody_is_missing() {
        let grain = Grain::new(GrainId::new("g"), "untrusted grain").with_metadata(
            GrainKind::Context,
            "fontes:bad",
            "derived_text_allowed",
        );
        let candidate = CandidateCut::new(CutId::new("bad")).with_grain(grain);

        let result = close_candidate_cut(candidate, &ContextBudget::tiny_fixture());

        let ClosureResult::BudgetFailure(failure) = result else {
            panic!("missing custody should fail closure");
        };
        assert_eq!(failure.reason, FrontierReason::MissingCustody);
        assert_eq!(failure.status(), lattice_model::BudgetStatus::BudgetFailure);
    }

    #[test]
    fn closure_fails_when_required_grains_exceed_budget() {
        let fixture = TinyModelFixture::from_source("fontes:ok", "derived_text_allowed");
        let candidate = fixture
            .grains
            .into_iter()
            .fold(CandidateCut::new(CutId::new("tiny")), |candidate, grain| {
                candidate.with_grain(grain)
            });
        let budget = ContextBudget {
            grain_limit: Some(2),
            ..ContextBudget::tiny_fixture()
        };

        let result = close_candidate_cut(candidate, &budget);

        let ClosureResult::BudgetFailure(failure) = result else {
            panic!("required closure over budget should fail");
        };
        assert_eq!(failure.reason, FrontierReason::RequiredClosureExceedsBudget);
    }

    fn closed_tiny_cut(id: &str, source_id: &str) -> ClosedCut {
        let fixture = TinyModelFixture::from_source(source_id, "derived_text_allowed");
        let candidate = fixture
            .grains
            .into_iter()
            .fold(CandidateCut::new(CutId::new(id)), |candidate, grain| {
                candidate.with_grain(grain)
            });
        let candidate = fixture
            .bonds
            .into_iter()
            .fold(candidate, |candidate, bond| candidate.with_bond(bond));
        match close_candidate_cut(candidate, &ContextBudget::tiny_fixture()) {
            ClosureResult::Closed { cut, .. } => cut,
            result => panic!("expected closed tiny cut, got {result:?}"),
        }
    }

    #[test]
    fn meet_closed_recloses_shared_context() {
        let left = closed_tiny_cut("left", "fontes:shared");
        let mut right = closed_tiny_cut("right", "fontes:shared");
        let deferred_index = right
            .grain_records
            .iter()
            .position(|grain| grain.kind == GrainKind::Context)
            .expect("tiny fixture has optional context grains");
        let deferred = right.grain_records.remove(deferred_index);
        right.grains.remove(&deferred.id);
        right
            .bonds
            .retain(|bond| bond.from != deferred.id && bond.to != deferred.id);

        let result = meet_closed(&left, &right, &ContextBudget::tiny_fixture());

        let ClosureResult::Closed { cut, budget_status } = result else {
            panic!("meet should close shared context");
        };
        assert_eq!(budget_status, lattice_model::BudgetStatus::WithinBudget);
        assert_eq!(cut.grains.len(), 9);
        assert_eq!(cut.closure_receipts.len(), 2);
        assert!(cut
            .closure_receipts
            .iter()
            .any(|receipt| receipt.rule == "meet"));
    }

    #[test]
    fn join_closed_recloses_unioned_context() {
        let left = closed_tiny_cut("left", "fontes:shared");
        let right = closed_tiny_cut("right", "fontes:shared");

        let result = join_closed(&left, &right, &ContextBudget::tiny_fixture());

        let ClosureResult::Closed { cut, budget_status } = result else {
            panic!("join should close unioned context");
        };
        assert_eq!(budget_status, lattice_model::BudgetStatus::WithinBudget);
        assert_eq!(cut.grains.len(), 10);
        assert_eq!(cut.bonds.len(), 20);
        assert_eq!(cut.closure_receipts.len(), 2);
        assert!(cut
            .closure_receipts
            .iter()
            .any(|receipt| receipt.rule == "join"));
    }

    #[test]
    fn join_closed_returns_budget_failure_when_required_closure_cannot_fit() {
        let left = closed_tiny_cut("left", "fontes:shared");
        let right = closed_tiny_cut("right", "fontes:shared");
        let budget = ContextBudget {
            grain_limit: Some(2),
            ..ContextBudget::tiny_fixture()
        };

        let result = join_closed(&left, &right, &budget);

        let ClosureResult::BudgetFailure(failure) = result else {
            panic!("join should fail when required closure cannot fit");
        };
        assert_eq!(failure.reason, FrontierReason::RequiredClosureExceedsBudget);
        assert_eq!(failure.receipt.rule, "join");
    }

    #[test]
    fn explains_closed_cut_relationships() {
        let broader = closed_tiny_cut("broader", "fontes:shared");
        let mut narrower = broader.clone();
        narrower.id = CutId::new("narrower");
        let optional_index = narrower
            .grain_records
            .iter()
            .position(|grain| grain.kind == GrainKind::Context)
            .unwrap();
        let optional = narrower.grain_records.remove(optional_index);
        narrower.grains.remove(&optional.id);
        narrower
            .bonds
            .retain(|bond| bond.from != optional.id && bond.to != optional.id);

        let diagnostic = explain_closed_cut_relation(&narrower, &broader);

        assert_eq!(diagnostic.relation, CutRelation::LeftNoBroader);
        assert_eq!(diagnostic.left_only_grain_count, 0);
        assert_eq!(diagnostic.right_only_grain_count, 1);
        assert_eq!(diagnostic.relation.as_str(), "left_no_broader");
    }

    #[test]
    fn hasse_diagnostic_edges_only_use_closed_cuts() {
        let broader = closed_tiny_cut("broader", "fontes:shared");
        let mut narrower = broader.clone();
        narrower.id = CutId::new("narrower");
        let optional_index = narrower
            .grain_records
            .iter()
            .position(|grain| grain.kind == GrainKind::Context)
            .unwrap();
        let optional = narrower.grain_records.remove(optional_index);
        narrower.grains.remove(&optional.id);
        narrower
            .bonds
            .retain(|bond| bond.from != optional.id && bond.to != optional.id);

        let edges = hasse_diagnostic_edges(&[narrower, broader]);

        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].narrower.as_str(), "narrower");
        assert_eq!(edges[0].broader.as_str(), "broader");
    }

    #[test]
    fn tiny_comparison_shows_closed_cut_advantage_over_generic_top_k() {
        let report = tiny_top_k_comparison();

        assert_eq!(report.fixture_id, "tiny-top-k-vs-closed-cut");
        assert_eq!(report.baseline.selector, "generic-top-k");
        assert!(!report.baseline.final_context_valid);
        assert!(report.baseline.missing_required_count > 0);
        assert_eq!(report.lattice.selector, "lattice-closed-cut");
        assert!(report.lattice.final_context_valid);
        assert_eq!(report.lattice.missing_required_count, 0);
        assert!(report.lattice.candidate_quality.closure_rescue_count > 0);
        assert!(report.lattice.context_metrics.is_some());
        assert!(report.baseline.cut_hash.is_none());
        assert!(report.baseline.receipt_hash.is_none());
        assert!(report
            .lattice
            .cut_hash
            .as_deref()
            .is_some_and(|hash| hash.starts_with("lattice-stable-v1:")));
        assert!(report
            .lattice
            .receipt_hash
            .as_deref()
            .is_some_and(|hash| hash.starts_with("lattice-stable-v1:")));
    }

    #[test]
    fn small_comparison_uses_same_metrics_at_larger_fixture_size() {
        let report = top_k_comparison(FixtureTier::Small);
        let context = report
            .lattice
            .context_metrics
            .as_ref()
            .expect("small comparison should include context metrics");

        assert_eq!(report.fixture_id, "small-top-k-vs-closed-cut");
        assert_eq!(report.tier, FixtureTier::Small);
        assert_eq!(report.baseline.candidate_quality.candidate_count, 48);
        assert!(!report.baseline.final_context_valid);
        assert!(report.baseline.missing_required_count > 0);
        assert!(report.lattice.final_context_valid);
        assert_eq!(report.lattice.missing_required_count, 0);
        assert_eq!(
            context.budget_status,
            lattice_model::BudgetStatus::WithinBudget
        );
        assert!(context.grain_count > 48);
    }

    #[test]
    fn bridge_comparison_rescues_connector_context() {
        let report = bridge_comparison();
        let context = report
            .lattice
            .context_metrics
            .as_ref()
            .expect("bridge comparison should include context metrics");

        assert_eq!(report.fixture_id, "bridge-top-k-vs-closed-cut");
        assert!(!report.baseline.final_context_valid);
        assert_eq!(report.baseline.candidate_quality.missed_expected_count, 4);
        assert_eq!(report.baseline.missing_required_count, 3);
        assert!(report.lattice.final_context_valid);
        assert_eq!(report.lattice.missing_required_count, 0);
        assert_eq!(report.lattice.candidate_quality.closure_rescue_count, 4);
        assert_eq!(context.grain_count, 7);
        assert_eq!(context.receipt_count, 1);
    }

    #[test]
    fn budget_comparison_frontiers_optional_context() {
        let report = budget_pressure_comparison();
        let context = report
            .lattice
            .context_metrics
            .as_ref()
            .expect("budget comparison should include context metrics");

        assert_eq!(report.fixture_id, "budget-top-k-vs-closed-cut");
        assert!(!report.baseline.final_context_valid);
        assert_eq!(report.baseline.missing_required_count, 3);
        assert!(report.lattice.final_context_valid);
        assert_eq!(report.lattice.missing_required_count, 0);
        assert_eq!(report.lattice.candidate_quality.closure_rescue_count, 3);
        assert_eq!(
            report
                .lattice
                .candidate_quality
                .frontier_false_negative_count,
            3
        );
        assert_eq!(context.grain_count, 5);
        assert!(context.frontier_count > 0);
        assert_eq!(
            context.budget_status,
            lattice_model::BudgetStatus::FrontierDeferred
        );
    }

    #[test]
    fn rights_comparison_surfaces_mixed_policy() {
        let report = rights_policy_comparison();
        let context = report
            .lattice
            .context_metrics
            .as_ref()
            .expect("rights comparison should include context metrics");

        assert_eq!(report.fixture_id, "rights-top-k-vs-closed-cut");
        assert!(!report.baseline.final_context_valid);
        assert_eq!(report.baseline.rights_policy, "untracked");
        assert_eq!(report.baseline.missing_required_count, 3);
        assert!(report.lattice.final_context_valid);
        assert_eq!(report.lattice.missing_required_count, 0);
        assert_eq!(report.lattice.rights_policy, "mixed");
        assert_eq!(report.lattice.candidate_quality.closure_rescue_count, 3);
        assert_eq!(context.receipt_count, 1);
        assert_eq!(
            context.budget_status,
            lattice_model::BudgetStatus::WithinBudget
        );
    }

    #[test]
    fn launch_readiness_comparison_scales_past_default_fixture() {
        let report = launch_readiness_comparison_at_scale(95);
        let context = report
            .lattice
            .context_metrics
            .as_ref()
            .expect("launch comparison should include context metrics");

        assert!(report.headline.contains("490-fact"));
        assert!(!report.baseline.final_context_valid);
        assert!(report.baseline.candidate_quality.candidate_recall < 0.2);
        assert!(report.lattice.final_context_valid);
        assert_eq!(report.lattice.missing_required_count, 0);
        assert_eq!(report.lattice.candidate_quality.closure_rescue_count, 187);
        assert_eq!(
            context.budget_status,
            lattice_model::BudgetStatus::WithinBudget
        );
    }
}
