#![forbid(unsafe_code)]

use std::collections::BTreeSet;

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct GrainId(String);

impl GrainId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct CutId(String);

impl CutId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum BondKind {
    Contains,
    DerivesFrom,
    Cites,
    Contradicts,
    SameEntity,
    Requires,
}

impl BondKind {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Contains => "contains",
            Self::DerivesFrom => "derives_from",
            Self::Cites => "cites",
            Self::Contradicts => "contradicts",
            Self::SameEntity => "same_entity",
            Self::Requires => "requires",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Bond {
    pub from: GrainId,
    pub to: GrainId,
    pub kind: BondKind,
}

impl Bond {
    pub fn new(from: GrainId, to: GrainId, kind: BondKind) -> Self {
        Self { from, to, kind }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Grain {
    pub id: GrainId,
    pub label: String,
    pub kind: GrainKind,
    pub source_id: Option<String>,
    pub rights_policy: Option<String>,
}

impl Grain {
    pub fn new(id: GrainId, label: impl Into<String>) -> Self {
        Self {
            id,
            label: label.into(),
            kind: GrainKind::Context,
            source_id: None,
            rights_policy: None,
        }
    }

    pub fn with_metadata(
        mut self,
        kind: GrainKind,
        source_id: impl Into<String>,
        rights_policy: impl Into<String>,
    ) -> Self {
        self.kind = kind;
        self.source_id = Some(source_id.into());
        self.rights_policy = Some(rights_policy.into());
        self
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GrainKind {
    SourcePointer,
    Context,
    Evidence,
    Policy,
    Receipt,
}

impl GrainKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SourcePointer => "source_pointer",
            Self::Context => "context",
            Self::Evidence => "evidence",
            Self::Policy => "policy",
            Self::Receipt => "receipt",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClosureReceipt {
    pub rule: String,
    pub note: String,
}

impl ClosureReceipt {
    pub fn new(rule: impl Into<String>, note: impl Into<String>) -> Self {
        Self {
            rule: rule.into(),
            note: note.into(),
        }
    }

    pub fn stable_hash(&self) -> String {
        let mut hasher = StableHasher::new("closure_receipt");
        hasher.write_token(&self.rule);
        hasher.write_token(&self.note);
        hasher.finish()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextBudget {
    pub token_limit: Option<usize>,
    pub grain_limit: Option<usize>,
    pub bond_limit: Option<usize>,
    pub closure_expansion_limit: Option<usize>,
    pub receipt_limit: Option<usize>,
    pub output_byte_limit: Option<usize>,
}

impl ContextBudget {
    pub const fn unbounded() -> Self {
        Self {
            token_limit: None,
            grain_limit: None,
            bond_limit: None,
            closure_expansion_limit: None,
            receipt_limit: None,
            output_byte_limit: None,
        }
    }

    pub const fn tiny_fixture() -> Self {
        Self {
            token_limit: Some(1_000),
            grain_limit: Some(10),
            bond_limit: Some(20),
            closure_expansion_limit: Some(5),
            receipt_limit: Some(10),
            output_byte_limit: Some(16_384),
        }
    }

    pub const fn fixture_tier(tier: FixtureTier) -> Self {
        match tier {
            FixtureTier::Tiny => Self::tiny_fixture(),
            FixtureTier::Small => Self {
                token_limit: Some(8_000),
                grain_limit: Some(64),
                bond_limit: Some(256),
                closure_expansion_limit: Some(32),
                receipt_limit: Some(32),
                output_byte_limit: Some(131_072),
            },
            FixtureTier::Medium => Self {
                token_limit: Some(32_000),
                grain_limit: Some(512),
                bond_limit: Some(2_048),
                closure_expansion_limit: Some(128),
                receipt_limit: Some(128),
                output_byte_limit: Some(1_048_576),
            },
            FixtureTier::Large => Self {
                token_limit: Some(128_000),
                grain_limit: Some(2_048),
                bond_limit: Some(8_192),
                closure_expansion_limit: Some(512),
                receipt_limit: Some(512),
                output_byte_limit: Some(4_194_304),
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BudgetStatus {
    WithinBudget,
    FrontierDeferred,
    BudgetFailure,
}

impl BudgetStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WithinBudget => "within_budget",
            Self::FrontierDeferred => "frontier_deferred",
            Self::BudgetFailure => "budget_failure",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrontierReason {
    BudgetLimit,
    PolicyBoundary,
    RightsBoundary,
    MissingCustody,
    InvalidBondEndpoint,
    RequiredClosureExceedsBudget,
    RedundantContext,
}

impl FrontierReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BudgetLimit => "budget_limit",
            Self::PolicyBoundary => "policy_boundary",
            Self::RightsBoundary => "rights_boundary",
            Self::MissingCustody => "missing_custody",
            Self::InvalidBondEndpoint => "invalid_bond_endpoint",
            Self::RequiredClosureExceedsBudget => "required_closure_exceeds_budget",
            Self::RedundantContext => "redundant_context",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FrontierItem {
    Grain(GrainId),
    Bond(Bond),
    Receipt(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrontierRecord {
    pub item: FrontierItem,
    pub reason: FrontierReason,
    pub note: String,
}

impl FrontierRecord {
    pub fn new(item: FrontierItem, reason: FrontierReason, note: impl Into<String>) -> Self {
        Self {
            item,
            reason,
            note: note.into(),
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Frontier {
    records: Vec<FrontierRecord>,
}

impl Frontier {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&mut self, record: FrontierRecord) {
        self.records.push(record);
    }

    pub fn records(&self) -> &[FrontierRecord] {
        &self.records
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    pub fn status(&self) -> BudgetStatus {
        if self.records.is_empty() {
            BudgetStatus::WithinBudget
        } else {
            BudgetStatus::FrontierDeferred
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BudgetFailure {
    pub reason: FrontierReason,
    pub budget: ContextBudget,
    pub frontier_count: usize,
    pub receipt: ClosureReceipt,
}

impl BudgetFailure {
    pub fn new(
        reason: FrontierReason,
        budget: ContextBudget,
        frontier_count: usize,
        receipt: ClosureReceipt,
    ) -> Self {
        Self {
            reason,
            budget,
            frontier_count,
            receipt,
        }
    }

    pub const fn status(&self) -> BudgetStatus {
        BudgetStatus::BudgetFailure
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FixtureTier {
    Tiny,
    Small,
    Medium,
    Large,
}

impl FixtureTier {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Tiny => "tiny",
            Self::Small => "small",
            Self::Medium => "medium",
            Self::Large => "large",
        }
    }

    pub const fn target_grain_count(self) -> usize {
        match self {
            Self::Tiny => 10,
            Self::Small => 1_000,
            Self::Medium => 100_000,
            Self::Large => 1_000_000,
        }
    }

    pub const fn target_bond_count(self) -> usize {
        match self {
            Self::Tiny => 20,
            Self::Small => 5_000,
            Self::Medium => 500_000,
            Self::Large => 5_000_000,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FixtureFamily {
    HappyPath,
    BudgetPressure,
    Adversarial,
    RetrievalQuality,
    Refresh,
}

impl FixtureFamily {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::HappyPath => "happy-path",
            Self::BudgetPressure => "budget-pressure",
            Self::Adversarial => "adversarial",
            Self::RetrievalQuality => "retrieval-quality",
            Self::Refresh => "refresh",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FixtureReportSkeleton {
    pub repo_sha: String,
    pub command_line: String,
    pub tier: FixtureTier,
    pub os: String,
    pub budget: ContextBudget,
    pub included_families: Vec<FixtureFamily>,
    pub excluded_cases: Vec<String>,
    pub caveats: Vec<String>,
}

impl FixtureReportSkeleton {
    pub fn tiny(command_line: impl Into<String>) -> Self {
        Self {
            repo_sha: "unknown".to_string(),
            command_line: command_line.into(),
            tier: FixtureTier::Tiny,
            os: std::env::consts::OS.to_string(),
            budget: ContextBudget::tiny_fixture(),
            included_families: vec![FixtureFamily::HappyPath],
            excluded_cases: vec![
                "real corpus ingestion".to_string(),
                "closure receipts".to_string(),
                "meet/join operators".to_string(),
            ],
            caveats: vec!["L1 skeleton only; no context correctness claim".to_string()],
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShardStatus {
    Closed,
    FrontierDeferred,
    BudgetFailure,
}

impl ShardStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Closed => "closed",
            Self::FrontierDeferred => "frontier_deferred",
            Self::BudgetFailure => "budget_failure",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShardManifest {
    pub shard_id: String,
    pub source_scope: String,
    pub rights_policy: String,
    pub grain_count: usize,
    pub bond_count: usize,
    pub receipt_count: usize,
    pub frontier_count: usize,
    pub closed_cut_hash: String,
    pub manifest_hash: String,
    pub status: ShardStatus,
}

impl ShardManifest {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        shard_id: impl Into<String>,
        source_scope: impl Into<String>,
        rights_policy: impl Into<String>,
        grain_count: usize,
        bond_count: usize,
        receipt_count: usize,
        frontier_count: usize,
        closed_cut_hash: impl Into<String>,
        status: ShardStatus,
    ) -> Self {
        let mut shard = Self {
            shard_id: shard_id.into(),
            source_scope: source_scope.into(),
            rights_policy: rights_policy.into(),
            grain_count,
            bond_count,
            receipt_count,
            frontier_count,
            closed_cut_hash: closed_cut_hash.into(),
            manifest_hash: String::new(),
            status,
        };
        shard.manifest_hash = shard.stable_manifest_hash();
        shard
    }

    pub fn stable_manifest_hash(&self) -> String {
        let mut hasher = StableHasher::new("shard_manifest");
        hasher.write_token(&self.shard_id);
        hasher.write_token(&self.source_scope);
        hasher.write_token(&self.rights_policy);
        hasher.write_usize(self.grain_count);
        hasher.write_usize(self.bond_count);
        hasher.write_usize(self.receipt_count);
        hasher.write_usize(self.frontier_count);
        hasher.write_token(&self.closed_cut_hash);
        hasher.write_token(self.status.as_str());
        hasher.finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShardBoundaryKind {
    Bridge,
    Cites,
    SameEntity,
    Conflicts,
}

impl ShardBoundaryKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Bridge => "bridge",
            Self::Cites => "cites",
            Self::SameEntity => "same_entity",
            Self::Conflicts => "conflicts",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShardAlignmentStatus {
    Aligned,
    Frontier,
    Conflict,
}

impl ShardAlignmentStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Aligned => "aligned",
            Self::Frontier => "frontier",
            Self::Conflict => "conflict",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShardBoundaryEdge {
    pub edge_id: String,
    pub from_shard: String,
    pub to_shard: String,
    pub kind: ShardBoundaryKind,
    pub evidence_receipt: String,
    pub rights_compatible: bool,
    pub alignment_status: ShardAlignmentStatus,
}

impl ShardBoundaryEdge {
    pub fn new(
        edge_id: impl Into<String>,
        from_shard: impl Into<String>,
        to_shard: impl Into<String>,
        kind: ShardBoundaryKind,
        evidence_receipt: impl Into<String>,
        rights_compatible: bool,
        alignment_status: ShardAlignmentStatus,
    ) -> Self {
        Self {
            edge_id: edge_id.into(),
            from_shard: from_shard.into(),
            to_shard: to_shard.into(),
            kind,
            evidence_receipt: evidence_receipt.into(),
            rights_compatible,
            alignment_status,
        }
    }

    pub fn stable_hash(&self) -> String {
        let mut hasher = StableHasher::new("shard_boundary_edge");
        hasher.write_token(&self.edge_id);
        hasher.write_token(&self.from_shard);
        hasher.write_token(&self.to_shard);
        hasher.write_token(self.kind.as_str());
        hasher.write_token(&self.evidence_receipt);
        hasher.write_bool(self.rights_compatible);
        hasher.write_token(self.alignment_status.as_str());
        hasher.finish()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShardAlignmentRow {
    pub row_id: String,
    pub requirement: String,
    pub passed: bool,
    pub evidence: String,
}

impl ShardAlignmentRow {
    pub fn new(
        row_id: impl Into<String>,
        requirement: impl Into<String>,
        passed: bool,
        evidence: impl Into<String>,
    ) -> Self {
        Self {
            row_id: row_id.into(),
            requirement: requirement.into(),
            passed,
            evidence: evidence.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShardRouteStatus {
    Selected,
    Skipped,
    Frontier,
}

impl ShardRouteStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Selected => "selected",
            Self::Skipped => "skipped",
            Self::Frontier => "frontier",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShardRouteDecision {
    pub shard_id: String,
    pub status: ShardRouteStatus,
    pub score: u8,
    pub reason: String,
    pub boundary_evidence: Vec<String>,
}

impl ShardRouteDecision {
    pub fn new(
        shard_id: impl Into<String>,
        status: ShardRouteStatus,
        score: u8,
        reason: impl Into<String>,
        boundary_evidence: Vec<String>,
    ) -> Self {
        Self {
            shard_id: shard_id.into(),
            status,
            score,
            reason: reason.into(),
            boundary_evidence,
        }
    }

    pub fn stable_hash(&self) -> String {
        let mut hasher = StableHasher::new("shard_route_decision");
        hasher.write_token(&self.shard_id);
        hasher.write_token(self.status.as_str());
        hasher.write_usize(usize::from(self.score));
        hasher.write_token(&self.reason);
        for evidence in &self.boundary_evidence {
            hasher.write_token(evidence);
        }
        hasher.finish()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShardScenarioExample {
    pub scenario_id: String,
    pub title: String,
    pub query: String,
    pub selected_shards: Vec<String>,
    pub frontier_shards: Vec<String>,
    pub boundary_edges: Vec<String>,
    pub expected_outcome: String,
}

impl ShardScenarioExample {
    pub fn new(
        scenario_id: impl Into<String>,
        title: impl Into<String>,
        query: impl Into<String>,
        selected_shards: Vec<String>,
        frontier_shards: Vec<String>,
        boundary_edges: Vec<String>,
        expected_outcome: impl Into<String>,
    ) -> Self {
        Self {
            scenario_id: scenario_id.into(),
            title: title.into(),
            query: query.into(),
            selected_shards,
            frontier_shards,
            boundary_edges,
            expected_outcome: expected_outcome.into(),
        }
    }

    pub fn stable_hash(&self) -> String {
        let mut hasher = StableHasher::new("shard_scenario_example");
        hasher.write_token(&self.scenario_id);
        hasher.write_token(&self.title);
        hasher.write_token(&self.query);
        for shard in &self.selected_shards {
            hasher.write_token("selected");
            hasher.write_token(shard);
        }
        for shard in &self.frontier_shards {
            hasher.write_token("frontier");
            hasher.write_token(shard);
        }
        for edge in &self.boundary_edges {
            hasher.write_token("edge");
            hasher.write_token(edge);
        }
        hasher.write_token(&self.expected_outcome);
        hasher.finish()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TinyModelFixture {
    pub source_id: String,
    pub rights_policy: String,
    pub grains: Vec<Grain>,
    pub bonds: Vec<Bond>,
}

impl TinyModelFixture {
    pub fn from_source(source_id: impl Into<String>, rights_policy: impl Into<String>) -> Self {
        Self::from_tier(FixtureTier::Tiny, source_id, rights_policy)
    }

    pub fn from_tier(
        tier: FixtureTier,
        source_id: impl Into<String>,
        rights_policy: impl Into<String>,
    ) -> Self {
        let source_id = source_id.into();
        let rights_policy = rights_policy.into();
        let source_grain_id = GrainId::new(format!("{source_id}:grain:00"));
        let grain_count = tier.target_grain_count();
        let bond_count = tier.target_bond_count();
        let mut grains = Vec::with_capacity(grain_count);
        grains.push(
            Grain::new(source_grain_id.clone(), "source pointer").with_metadata(
                GrainKind::SourcePointer,
                source_id.clone(),
                rights_policy.clone(),
            ),
        );

        for index in 1..grain_count {
            let kind = if index == grain_count - 3 {
                GrainKind::Evidence
            } else if index == grain_count - 2 {
                GrainKind::Policy
            } else if index == grain_count - 1 {
                GrainKind::Receipt
            } else {
                match index {
                    7 if tier == FixtureTier::Tiny => GrainKind::Evidence,
                    8 if tier == FixtureTier::Tiny => GrainKind::Policy,
                    9 if tier == FixtureTier::Tiny => GrainKind::Receipt,
                    _ => GrainKind::Context,
                }
            };
            grains.push(
                Grain::new(
                    GrainId::new(format!("{source_id}:grain:{index:06}")),
                    format!("{} synthetic grain {index:06}", tier.as_str()),
                )
                .with_metadata(kind, source_id.clone(), rights_policy.clone()),
            );
        }

        let mut bonds = BTreeSet::new();
        for grain in grains.iter().take(grain_count).skip(1) {
            bonds.insert(Bond::new(
                source_grain_id.clone(),
                grain.id.clone(),
                BondKind::Contains,
            ));
        }
        for index in 1..(grain_count - 1) {
            bonds.insert(Bond::new(
                grains[index].id.clone(),
                grains[index + 1].id.clone(),
                BondKind::Requires,
            ));
        }
        let extra_kinds = [
            BondKind::Cites,
            BondKind::DerivesFrom,
            BondKind::SameEntity,
            BondKind::Contradicts,
        ];
        let mut seed = 0usize;
        while bonds.len() < bond_count {
            let from_index = 1 + (seed % (grain_count - 1));
            let to_index = (seed * 17 + 3) % grain_count;
            if from_index != to_index {
                bonds.insert(Bond::new(
                    grains[from_index].id.clone(),
                    grains[to_index].id.clone(),
                    extra_kinds[seed % extra_kinds.len()].clone(),
                ));
            }
            seed += 1;
        }

        Self {
            source_id,
            rights_policy,
            grains,
            bonds: bonds.into_iter().collect(),
        }
    }
}

pub const LAUNCH_READINESS_FACTS: &str = "\
product|prd-ux-caveat|assistant launch is feature-complete but exposes beta caveats|launch,assistant,readiness,beta|auto
product|prd-guardrail|safe answer mode is required for regulated customer questions|launch,assistant,guardrail,compliance|auto
product|prd-gap|admin analytics is deferred until post-launch|launch,assistant,analytics,frontier|guidance
customer|cust-cohort|first rollout cohort is 250 design partners|launch,customer,cohort,blast-radius|auto
customer|cust-risk|three healthcare customers require stricter response review|launch,customer,compliance,review|guidance
customer|cust-demand|pilot customers report high demand for support deflection|launch,customer,support,value|auto
compliance|comp-policy|derived text is allowed but customer data cannot enter prompts|launch,compliance,rights,privacy|auto
compliance|comp-approval|legal approval is conditional on support rollback wording|launch,compliance,legal,operations|guidance
compliance|comp-audit|launch must cite source pointers and receipt hashes|launch,compliance,evidence,receipt|auto
operations|ops-support|support team is staffed for weekday business hours only|launch,operations,support,coverage|auto
operations|ops-rollback|rollback playbook exists but paging owner is unclear|launch,operations,rollback,frontier|guidance
operations|ops-slo|incident response target is fifteen minutes for launch week|launch,operations,slo,support|auto
evidence|ev-receipts|all launch recommendation facts need receipts before prompt handoff|launch,evidence,receipt,closure|auto
evidence|ev-conflict|customer demand conflicts with support coverage limits|launch,evidence,customer,operations,conflict|auto
evidence|ev-decision|go recommendation requires product, customer, compliance, operations, and evidence context|launch,evidence,decision,join|guidance";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum LaunchDepartment {
    Product,
    Customer,
    Compliance,
    Operations,
    Evidence,
}

impl LaunchDepartment {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Product => "product",
            Self::Customer => "customer",
            Self::Compliance => "compliance",
            Self::Operations => "operations",
            Self::Evidence => "evidence",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "product" => Some(Self::Product),
            "customer" => Some(Self::Customer),
            "compliance" => Some(Self::Compliance),
            "operations" => Some(Self::Operations),
            "evidence" => Some(Self::Evidence),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BondOrigin {
    Auto,
    Guidance,
}

impl BondOrigin {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Guidance => "guidance",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LaunchFact {
    pub department: LaunchDepartment,
    pub id: String,
    pub text: String,
    pub tags: Vec<String>,
    pub bond_origin: BondOrigin,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LaunchGuidanceBond {
    pub from: GrainId,
    pub to: GrainId,
    pub reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LaunchReadinessFixture {
    pub source_id: String,
    pub rights_policy: String,
    pub facts: Vec<LaunchFact>,
    pub grains: Vec<Grain>,
    pub auto_bonds: Vec<Bond>,
    pub guidance_bonds: Vec<LaunchGuidanceBond>,
    pub decision_cut_fact_ids: Vec<String>,
}

impl LaunchReadinessFixture {
    pub fn parse_default() -> Self {
        Self::parse_with_generated_per_department(45)
    }

    pub fn parse_with_generated_per_department(per_department: usize) -> Self {
        Self::parse_with_generated_per_department_for_source(
            "synthetic:launch-readiness",
            "synthetic_public_demo",
            per_department,
        )
    }

    pub fn parse_with_generated_per_department_for_source(
        source_id: impl Into<String>,
        rights_policy: impl Into<String>,
        per_department: usize,
    ) -> Self {
        let mut facts = LAUNCH_READINESS_FACTS
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(parse_launch_fact)
            .collect::<Vec<_>>();
        facts.extend(generated_launch_facts(per_department));
        Self::from_facts(source_id, rights_policy, facts)
    }

    pub fn parse(
        source_id: impl Into<String>,
        rights_policy: impl Into<String>,
        input: &str,
    ) -> Self {
        let facts = input
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(parse_launch_fact)
            .collect::<Vec<_>>();
        Self::from_facts(source_id, rights_policy, facts)
    }

    fn from_facts(
        source_id: impl Into<String>,
        rights_policy: impl Into<String>,
        facts: Vec<LaunchFact>,
    ) -> Self {
        let source_id = source_id.into();
        let rights_policy = rights_policy.into();
        let grains = facts
            .iter()
            .map(|fact| {
                Grain::new(
                    GrainId::new(format!("{}:{}", source_id, fact.id)),
                    format!("{}: {}", fact.department.as_str(), fact.text),
                )
                .with_metadata(
                    GrainKind::Context,
                    source_id.clone(),
                    rights_policy.clone(),
                )
            })
            .collect::<Vec<_>>();
        let auto_bonds = auto_bond_launch_facts(&source_id, &facts);
        let guidance_bonds = guidance_bond_launch_facts(&source_id, &facts);
        let decision_cut_fact_ids = decision_cut_fact_ids(&facts);

        Self {
            source_id,
            rights_policy,
            facts,
            grains,
            auto_bonds,
            guidance_bonds,
            decision_cut_fact_ids,
        }
    }

    pub fn fact_count(&self) -> usize {
        self.facts.len()
    }

    pub fn department_count(&self) -> usize {
        self.facts
            .iter()
            .map(|fact| fact.department)
            .collect::<BTreeSet<_>>()
            .len()
    }

    pub fn auto_bond_count(&self) -> usize {
        self.auto_bonds.len()
    }

    pub fn guidance_bond_count(&self) -> usize {
        self.guidance_bonds.len()
    }

    pub fn auto_bond_ratio(&self) -> f64 {
        let total = self.auto_bond_count() + self.guidance_bond_count();
        if total == 0 {
            0.0
        } else {
            self.auto_bond_count() as f64 / total as f64
        }
    }

    pub fn decision_cut_fact_count(&self) -> usize {
        self.decision_cut_fact_ids.len()
    }

    pub fn decision_cut_ratio(&self) -> f64 {
        if self.facts.is_empty() {
            0.0
        } else {
            self.decision_cut_fact_count() as f64 / self.facts.len() as f64
        }
    }
}

fn parse_launch_fact(line: &str) -> LaunchFact {
    let parts = line.split('|').collect::<Vec<_>>();
    assert_eq!(parts.len(), 5, "launch fact rows must have 5 fields");
    let department =
        LaunchDepartment::parse(parts[0]).expect("launch fact department must be known");
    let bond_origin = match parts[4] {
        "auto" => BondOrigin::Auto,
        "guidance" => BondOrigin::Guidance,
        _ => panic!("launch fact bond origin must be auto or guidance"),
    };
    LaunchFact {
        department,
        id: parts[1].to_string(),
        text: parts[2].to_string(),
        tags: parts[3].split(',').map(str::to_string).collect(),
        bond_origin,
    }
}

fn generated_launch_facts(per_department: usize) -> Vec<LaunchFact> {
    let departments = [
        LaunchDepartment::Product,
        LaunchDepartment::Customer,
        LaunchDepartment::Compliance,
        LaunchDepartment::Operations,
        LaunchDepartment::Evidence,
    ];
    departments
        .into_iter()
        .flat_map(|department| {
            (0..per_department).map(move |index| generated_launch_fact(department, index))
        })
        .collect()
}

fn generated_launch_fact(department: LaunchDepartment, index: usize) -> LaunchFact {
    let (prefix, themes) = match department {
        LaunchDepartment::Product => (
            "prd",
            [
                "assistant",
                "guardrail",
                "latency",
                "onboarding",
                "analytics",
                "readiness",
            ],
        ),
        LaunchDepartment::Customer => (
            "cust",
            [
                "cohort",
                "support",
                "healthcare",
                "feedback",
                "blast-radius",
                "readiness",
            ],
        ),
        LaunchDepartment::Compliance => (
            "comp",
            [
                "privacy",
                "rights",
                "approval",
                "audit",
                "data-boundary",
                "readiness",
            ],
        ),
        LaunchDepartment::Operations => (
            "ops",
            [
                "support",
                "rollback",
                "coverage",
                "slo",
                "incident",
                "readiness",
            ],
        ),
        LaunchDepartment::Evidence => (
            "ev",
            [
                "receipt",
                "conflict",
                "source-pointer",
                "decision",
                "closure",
                "readiness",
            ],
        ),
    };
    let theme = themes[index % themes.len()];
    let shared = match index % 9 {
        0 => "compliance",
        1 => "support",
        2 => "customer",
        3 => "operations",
        4 => "evidence",
        5 => "frontier",
        6 => "budget",
        7 => "receipt",
        _ => "launch-week",
    };
    let origin = if matches!(theme, "approval" | "rollback" | "privacy")
        || matches!(shared, "frontier" | "budget")
        || index.is_multiple_of(13)
    {
        BondOrigin::Guidance
    } else {
        BondOrigin::Auto
    };

    LaunchFact {
        department,
        id: format!("{prefix}-generated-{index:03}"),
        text: format!(
            "{} launch readiness note {index:03}: {theme} signal touches {shared}",
            department.as_str()
        ),
        tags: vec![
            "launch".to_string(),
            department.as_str().to_string(),
            theme.to_string(),
            shared.to_string(),
        ],
        bond_origin: origin,
    }
}

fn decision_cut_fact_ids(facts: &[LaunchFact]) -> Vec<String> {
    let keep_tags = [
        "receipt", "rollback", "privacy", "decision", "conflict", "frontier", "budget",
    ];
    facts
        .iter()
        .filter(|fact| {
            fact.id == "ev-decision"
                || fact.id == "ev-receipts"
                || fact
                    .tags
                    .iter()
                    .any(|tag| keep_tags.iter().any(|keep| keep == tag))
        })
        .map(|fact| fact.id.clone())
        .collect()
}

fn auto_bond_launch_facts(source_id: &str, facts: &[LaunchFact]) -> Vec<Bond> {
    let mut bonds = BTreeSet::new();
    for left in facts {
        for right in facts {
            if left.id.as_str() >= right.id.as_str() || left.department == right.department {
                continue;
            }
            let shared_tags = left
                .tags
                .iter()
                .filter(|tag| tag.as_str() != "launch" && right.tags.contains(tag))
                .count();
            if shared_tags > 0
                && left.bond_origin == BondOrigin::Auto
                && right.bond_origin == BondOrigin::Auto
            {
                bonds.insert(Bond::new(
                    GrainId::new(format!("{source_id}:{}", left.id)),
                    GrainId::new(format!("{source_id}:{}", right.id)),
                    BondKind::Cites,
                ));
            }
        }
    }
    bonds.into_iter().collect()
}

fn guidance_bond_launch_facts(source_id: &str, facts: &[LaunchFact]) -> Vec<LaunchGuidanceBond> {
    let guidance = facts
        .iter()
        .filter(|fact| fact.bond_origin == BondOrigin::Guidance)
        .collect::<Vec<_>>();
    let evidence = facts
        .iter()
        .find(|fact| fact.id == "ev-decision")
        .expect("launch fixture should include decision guidance");
    let receipt_anchor = facts
        .iter()
        .find(|fact| fact.id == "ev-receipts")
        .expect("launch fixture should include receipt evidence");
    guidance
        .into_iter()
        .map(|fact| LaunchGuidanceBond {
            from: GrainId::new(format!("{source_id}:{}", fact.id)),
            to: GrainId::new(format!(
                "{source_id}:{}",
                if fact.id == evidence.id {
                    receipt_anchor.id.as_str()
                } else {
                    evidence.id.as_str()
                }
            )),
            reason: "requires human launch-readiness guidance".to_string(),
        })
        .collect()
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClosedCut {
    pub id: CutId,
    pub grains: BTreeSet<GrainId>,
    pub grain_records: Vec<Grain>,
    pub bonds: BTreeSet<Bond>,
    pub closure_receipts: Vec<ClosureReceipt>,
    pub closure_policy: Option<String>,
    pub rights_policy: Option<String>,
}

impl ClosedCut {
    pub fn new(id: CutId) -> Self {
        Self {
            id,
            grains: BTreeSet::new(),
            grain_records: Vec::new(),
            bonds: BTreeSet::new(),
            closure_receipts: Vec::new(),
            closure_policy: None,
            rights_policy: None,
        }
    }

    pub fn with_grain(mut self, grain: GrainId) -> Self {
        self.grains.insert(grain);
        self
    }

    pub fn with_bond(mut self, bond: Bond) -> Self {
        self.bonds.insert(bond);
        self
    }

    pub fn with_receipt(mut self, receipt: ClosureReceipt) -> Self {
        self.closure_receipts.push(receipt);
        self
    }

    pub fn with_policy(
        mut self,
        closure_policy: impl Into<String>,
        rights_policy: impl Into<String>,
    ) -> Self {
        self.closure_policy = Some(closure_policy.into());
        self.rights_policy = Some(rights_policy.into());
        self
    }

    pub fn contains_bond_endpoints(&self, bond: &Bond) -> bool {
        self.grains.contains(&bond.from) && self.grains.contains(&bond.to)
    }

    pub fn stable_hash(&self) -> String {
        let mut hasher = StableHasher::new("closed_cut");
        hasher.write_token(self.id.as_str());
        hasher.write_optional(self.closure_policy.as_deref());
        hasher.write_optional(self.rights_policy.as_deref());

        for grain in &self.grains {
            hasher.write_token("grain");
            hasher.write_token(grain.as_str());
        }

        let mut grain_records = self.grain_records.iter().collect::<Vec<_>>();
        grain_records.sort_by(|left, right| left.id.cmp(&right.id));
        for grain in grain_records {
            hasher.write_token("grain_record");
            hasher.write_token(grain.id.as_str());
            hasher.write_token(&grain.label);
            hasher.write_token(grain.kind.as_str());
            hasher.write_optional(grain.source_id.as_deref());
            hasher.write_optional(grain.rights_policy.as_deref());
        }

        for bond in &self.bonds {
            hasher.write_token("bond");
            hasher.write_token(bond.from.as_str());
            hasher.write_token(bond.to.as_str());
            hasher.write_token(bond.kind.as_str());
        }

        for receipt in &self.closure_receipts {
            hasher.write_token("receipt");
            hasher.write_token(&receipt.stable_hash());
        }

        hasher.finish()
    }

    pub fn receipt_hash(&self) -> String {
        let mut hasher = StableHasher::new("closed_cut_receipts");
        hasher.write_token(self.id.as_str());
        for receipt in &self.closure_receipts {
            hasher.write_token(&receipt.stable_hash());
        }
        hasher.finish()
    }
}

const STABLE_HASH_PREFIX: &str = "lattice-stable-v1";
const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct StableHasher {
    state: u64,
}

impl StableHasher {
    fn new(domain: &str) -> Self {
        let mut hasher = Self {
            state: FNV_OFFSET_BASIS,
        };
        hasher.write_token(STABLE_HASH_PREFIX);
        hasher.write_token(domain);
        hasher
    }

    fn write_optional(&mut self, value: Option<&str>) {
        match value {
            Some(value) => {
                self.write_token("some");
                self.write_token(value);
            }
            None => self.write_token("none"),
        }
    }

    fn write_bool(&mut self, value: bool) {
        self.write_token(if value { "true" } else { "false" });
    }

    fn write_usize(&mut self, value: usize) {
        self.write_token(&value.to_string());
    }

    fn write_token(&mut self, value: &str) {
        for byte in value.len().to_string().bytes().chain([b':']) {
            self.write_byte(byte);
        }
        for byte in value.bytes() {
            self.write_byte(byte);
        }
        self.write_byte(b'|');
    }

    fn write_byte(&mut self, byte: u8) {
        self.state ^= u64::from(byte);
        self.state = self.state.wrapping_mul(FNV_PRIME);
    }

    fn finish(self) -> String {
        format!("{STABLE_HASH_PREFIX}:{:016x}", self.state)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextPack {
    pub id: String,
    pub cut_id: CutId,
    pub cut_hash: String,
    pub receipt_hash: String,
    pub profile_id: String,
    pub cache_prefix: String,
    pub grain_count: usize,
    pub bond_count: usize,
    pub receipt_count: usize,
    pub closure_policy: Option<String>,
    pub rights_policy: Option<String>,
    pub caveats: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfileMetricWeight {
    pub metric: &'static str,
    pub weight: u8,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LatticeProfileMode {
    pub id: &'static str,
    pub goal: &'static str,
    pub optimizes_for: &'static [&'static str],
    pub tradeoff: &'static str,
    pub latency_class: &'static str,
    pub audit_level: &'static str,
    pub context_width: &'static str,
    pub closure_required: bool,
    pub pack_profile: &'static str,
    pub metric_weights: Vec<ProfileMetricWeight>,
}

impl LatticeProfileMode {
    pub fn total_weight(&self) -> u16 {
        self.metric_weights
            .iter()
            .map(|weight| u16::from(weight.weight))
            .sum()
    }
}

pub fn lattice_profile_modes() -> Vec<LatticeProfileMode> {
    vec![
        profile_mode(
            "fast-answer",
            "Interactive answer grounding",
            &["latency", "precision", "provenance_floor"],
            "Smaller packs and lighter audit detail",
            "fast",
            "light",
            "narrow",
            true,
            "answer-grounding",
            &[
                ("latency", 35),
                ("precision", 25),
                ("provenance_completeness", 20),
                ("recall", 10),
                ("frontier_honesty", 10),
            ],
        ),
        profile_mode(
            "balanced-synthesis",
            "Default research synthesis",
            &["recall", "precision", "trust"],
            "Moderate latency for stronger context quality",
            "balanced",
            "standard",
            "medium",
            true,
            "answer-grounding",
            &[
                ("recall", 20),
                ("precision", 20),
                ("branch_coverage", 15),
                ("provenance_completeness", 15),
                ("closure_validity", 15),
                ("frontier_honesty", 10),
                ("latency", 5),
            ],
        ),
        profile_mode(
            "audit-grade",
            "After-the-fact verification",
            &["closure", "replay", "provenance", "frontier"],
            "Slower, audit-heavy execution",
            "slow",
            "full",
            "wide",
            true,
            "coverage-audit",
            &[
                ("closure_validity", 30),
                ("provenance_completeness", 25),
                ("replayability", 20),
                ("frontier_honesty", 15),
                ("recall", 10),
            ],
        ),
        profile_mode(
            "reading-path",
            "Learn a topic step by step",
            &["prerequisite_order", "branch_coverage", "clarity"],
            "Longer packs and less direct answers",
            "balanced",
            "standard",
            "wide",
            true,
            "reading-path",
            &[
                ("prerequisite_order", 30),
                ("branch_coverage", 25),
                ("recall", 20),
                ("clarity", 15),
                ("provenance_completeness", 10),
            ],
        ),
        profile_mode(
            "common-thread",
            "Find the invariant across branches",
            &["meet_quality", "synthesis", "precision"],
            "May omit broad surrounding context",
            "balanced",
            "standard",
            "medium",
            true,
            "answer-grounding",
            &[
                ("common_thread_quality", 35),
                ("precision", 20),
                ("branch_coverage", 15),
                ("closure_validity", 15),
                ("frontier_honesty", 15),
            ],
        ),
        profile_mode(
            "red-team",
            "Challenge an answer or claim",
            &["contradiction", "missing_evidence", "frontier"],
            "Less direct answer; more caveats",
            "balanced",
            "strong",
            "wide",
            true,
            "review-triage",
            &[
                ("frontier_honesty", 25),
                ("contradiction_coverage", 20),
                ("provenance_completeness", 20),
                ("closure_validity", 15),
                ("precision", 10),
                ("replayability", 10),
            ],
        ),
        profile_mode(
            "coverage-audit",
            "Find sparse, overloaded, duplicated, or missing coverage",
            &["coverage", "gaps", "duplication"],
            "Not optimized for Q&A",
            "slow",
            "full",
            "wide",
            true,
            "coverage-audit",
            &[
                ("branch_coverage", 25),
                ("gap_detection", 20),
                ("provenance_completeness", 20),
                ("frontier_honesty", 15),
                ("replayability", 10),
                ("latency", 10),
            ],
        ),
        profile_mode(
            "incident-response",
            "High-pressure operational query",
            &["latency", "risk_triage", "provenance_floor"],
            "Narrower context and lower audit detail",
            "fast",
            "standard",
            "narrow",
            true,
            "answer-grounding",
            &[
                ("latency", 30),
                ("risk_triage", 25),
                ("precision", 20),
                ("provenance_completeness", 15),
                ("frontier_honesty", 10),
            ],
        ),
        profile_mode(
            "teaching-mode",
            "Explain for a learner",
            &["clarity", "sequence", "examples"],
            "Longer packs and slower synthesis",
            "balanced",
            "standard",
            "wide",
            true,
            "reading-path",
            &[
                ("clarity", 30),
                ("prerequisite_order", 20),
                ("branch_coverage", 15),
                ("recall", 15),
                ("provenance_completeness", 10),
                ("frontier_honesty", 10),
            ],
        ),
        profile_mode(
            "expert-brief",
            "Compact synthesis for expert readers",
            &["density", "precision", "novelty"],
            "Assumes background knowledge",
            "fast",
            "standard",
            "medium",
            true,
            "answer-grounding",
            &[
                ("precision", 25),
                ("density", 25),
                ("common_thread_quality", 20),
                ("provenance_completeness", 15),
                ("latency", 10),
                ("frontier_honesty", 5),
            ],
        ),
        profile_mode(
            "decision-support",
            "Compare options and tradeoffs",
            &["alternatives", "criteria", "exclusions"],
            "Needs explicit decision criteria",
            "balanced",
            "strong",
            "wide",
            true,
            "answer-grounding",
            &[
                ("alternative_coverage", 25),
                ("precision", 20),
                ("frontier_honesty", 20),
                ("provenance_completeness", 15),
                ("closure_validity", 10),
                ("replayability", 10),
            ],
        ),
        profile_mode(
            "panel-review",
            "Multi-role critique and review",
            &["role_coverage", "disagreement", "audit_trail"],
            "Verbose and slower",
            "slow",
            "full",
            "wide",
            true,
            "review-triage",
            &[
                ("role_coverage", 25),
                ("contradiction_coverage", 20),
                ("provenance_completeness", 15),
                ("closure_validity", 15),
                ("frontier_honesty", 15),
                ("replayability", 10),
            ],
        ),
    ]
}

#[allow(clippy::too_many_arguments)]
fn profile_mode(
    id: &'static str,
    goal: &'static str,
    optimizes_for: &'static [&'static str],
    tradeoff: &'static str,
    latency_class: &'static str,
    audit_level: &'static str,
    context_width: &'static str,
    closure_required: bool,
    pack_profile: &'static str,
    weights: &[(&'static str, u8)],
) -> LatticeProfileMode {
    LatticeProfileMode {
        id,
        goal,
        optimizes_for,
        tradeoff,
        latency_class,
        audit_level,
        context_width,
        closure_required,
        pack_profile,
        metric_weights: weights
            .iter()
            .map(|(metric, weight)| ProfileMetricWeight {
                metric,
                weight: *weight,
            })
            .collect(),
    }
}

impl ContextPack {
    pub fn from_closed_cut(cut: &ClosedCut, profile_id: impl Into<String>) -> Self {
        let profile_id = profile_id.into();
        let cut_hash = cut.stable_hash();
        let receipt_hash = cut.receipt_hash();
        let cache_prefix = format!("lattice-cache-v1:{profile_id}:{cut_hash}");
        Self {
            id: format!("pack:{}:{}", profile_id, cut.id.as_str()),
            cut_id: cut.id.clone(),
            cut_hash,
            receipt_hash,
            profile_id,
            cache_prefix,
            grain_count: cut.grains.len(),
            bond_count: cut.bonds.len(),
            receipt_count: cut.closure_receipts.len(),
            closure_policy: cut.closure_policy.clone(),
            rights_policy: cut.rights_policy.clone(),
            caveats: vec!["closed-cut-derived materialization; not source of truth".to_string()],
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PromptFrame {
    pub frame_version: String,
    pub pack: ContextPack,
    pub contract: String,
    pub grain_ids: Vec<String>,
    pub receipt_count: usize,
}

impl PromptFrame {
    pub fn from_closed_cut(cut: &ClosedCut, profile_id: impl Into<String>) -> Self {
        Self {
            frame_version: "lattice.prompt-frame.v1".to_string(),
            pack: ContextPack::from_closed_cut(cut, profile_id),
            contract: "closed-cut-only".to_string(),
            grain_ids: cut
                .grains
                .iter()
                .map(|grain_id| grain_id.as_str().to_string())
                .collect(),
            receipt_count: cut.closure_receipts.len(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PressPublicationFrame {
    pub frame_version: String,
    pub pack: ContextPack,
    pub target_family: String,
    pub handoff_contract: String,
    pub receipt_count: usize,
    pub caveats: Vec<String>,
}

impl PressPublicationFrame {
    pub fn from_closed_cut(
        cut: &ClosedCut,
        profile_id: impl Into<String>,
        target_family: impl Into<String>,
    ) -> Self {
        let pack = ContextPack::from_closed_cut(cut, profile_id);
        Self {
            frame_version: "lattice.press-frame.v1".to_string(),
            receipt_count: pack.receipt_count,
            pack,
            target_family: target_family.into(),
            handoff_contract: "file-contract-only; PRESS renders downstream".to_string(),
            caveats: vec![
                "LATTICE does not render DOCX/PPTX/PDF/site artifacts".to_string(),
                "rights policy and closure receipts must survive downstream rendering".to_string(),
            ],
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MetricError {
    SelectedGrainOutsideUniverse(GrainId),
    BondEndpointOutsideUniverse(Bond),
}

#[derive(Clone, Debug, PartialEq)]
pub struct BoundaryMetrics {
    pub selected_count: usize,
    pub complement_count: usize,
    pub selected_internal_bonds: usize,
    pub complement_internal_bonds: usize,
    pub boundary_bonds: usize,
    pub selected_degree: usize,
    pub complement_degree: usize,
    pub total_bonds: usize,
    pub conductance: f64,
}

impl BoundaryMetrics {
    pub fn from_bonds(
        universe: &BTreeSet<GrainId>,
        bonds: &BTreeSet<Bond>,
        selected: &BTreeSet<GrainId>,
    ) -> Result<Self, MetricError> {
        if let Some(grain) = selected.iter().find(|grain| !universe.contains(*grain)) {
            return Err(MetricError::SelectedGrainOutsideUniverse(grain.clone()));
        }

        let mut selected_internal_bonds = 0usize;
        let mut complement_internal_bonds = 0usize;
        let mut boundary_bonds = 0usize;

        for bond in bonds {
            if !universe.contains(&bond.from) || !universe.contains(&bond.to) {
                return Err(MetricError::BondEndpointOutsideUniverse(bond.clone()));
            }

            match (selected.contains(&bond.from), selected.contains(&bond.to)) {
                (true, true) => selected_internal_bonds += 1,
                (false, false) => complement_internal_bonds += 1,
                _ => boundary_bonds += 1,
            }
        }

        let selected_degree = selected_internal_bonds * 2 + boundary_bonds;
        let complement_degree = complement_internal_bonds * 2 + boundary_bonds;
        let denominator = selected_degree.min(complement_degree);
        let conductance = if denominator == 0 {
            0.0
        } else {
            boundary_bonds as f64 / denominator as f64
        };

        Ok(Self {
            selected_count: selected.len(),
            complement_count: universe.len() - selected.len(),
            selected_internal_bonds,
            complement_internal_bonds,
            boundary_bonds,
            selected_degree,
            complement_degree,
            total_bonds: selected_internal_bonds + complement_internal_bonds + boundary_bonds,
            conductance,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CandidateQualityMetrics {
    pub expected_count: usize,
    pub candidate_count: usize,
    pub true_positive_count: usize,
    pub missed_expected_count: usize,
    pub distractor_count: usize,
    pub closure_rescue_count: usize,
    pub frontier_false_negative_count: usize,
    pub candidate_recall: f64,
    pub candidate_precision: f64,
}

impl CandidateQualityMetrics {
    pub fn from_sets(
        expected: &BTreeSet<GrainId>,
        candidates: &BTreeSet<GrainId>,
        closed: &BTreeSet<GrainId>,
        frontier: &BTreeSet<GrainId>,
    ) -> Self {
        let true_positive_count = candidates
            .iter()
            .filter(|grain| expected.contains(*grain))
            .count();
        let missed_expected_count = expected
            .iter()
            .filter(|grain| !candidates.contains(*grain))
            .count();
        let distractor_count = candidates
            .iter()
            .filter(|grain| !expected.contains(*grain))
            .count();
        let closure_rescue_count = closed
            .iter()
            .filter(|grain| !candidates.contains(*grain))
            .count();
        let frontier_false_negative_count = expected
            .iter()
            .filter(|grain| frontier.contains(*grain))
            .count();

        Self {
            expected_count: expected.len(),
            candidate_count: candidates.len(),
            true_positive_count,
            missed_expected_count,
            distractor_count,
            closure_rescue_count,
            frontier_false_negative_count,
            candidate_recall: ratio(true_positive_count, expected.len()),
            candidate_precision: ratio(true_positive_count, candidates.len()),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ContextMetrics {
    pub elapsed_ms: u128,
    pub grain_count: usize,
    pub bond_count: usize,
    pub closure_added_count: usize,
    pub frontier_count: usize,
    pub receipt_count: usize,
    pub output_bytes: usize,
    pub token_budget: Option<usize>,
    pub estimated_tokens: Option<usize>,
    pub budget_used_ratio: Option<f64>,
    pub budget_status: BudgetStatus,
}

impl ContextMetrics {
    pub fn from_closed_cut(
        cut: &ClosedCut,
        budget: &ContextBudget,
        frontier: &Frontier,
        elapsed_ms: u128,
        output_bytes: usize,
        estimated_tokens: Option<usize>,
        closure_added_count: usize,
    ) -> Self {
        Self {
            elapsed_ms,
            grain_count: cut.grains.len(),
            bond_count: cut.bonds.len(),
            closure_added_count,
            frontier_count: frontier.records().len(),
            receipt_count: cut.closure_receipts.len(),
            output_bytes,
            token_budget: budget.token_limit,
            estimated_tokens,
            budget_used_ratio: estimated_tokens
                .zip(budget.token_limit)
                .map(|(tokens, limit)| ratio(tokens, limit)),
            budget_status: frontier.status(),
        }
    }
}

fn ratio(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closed_cut_tracks_grains_and_bonds() {
        let a = GrainId::new("a");
        let b = GrainId::new("b");
        let cut = ClosedCut::new(CutId::new("cut"))
            .with_grain(a.clone())
            .with_grain(b.clone())
            .with_bond(Bond::new(a, b, BondKind::Requires));

        assert_eq!(cut.grains.len(), 2);
        assert_eq!(cut.bonds.len(), 1);
    }

    #[test]
    fn context_budget_and_frontier_are_typed() {
        let budget = ContextBudget::tiny_fixture();
        let mut frontier = Frontier::new();

        assert_eq!(budget.grain_limit, Some(10));
        assert_eq!(frontier.status(), BudgetStatus::WithinBudget);

        frontier.record(FrontierRecord::new(
            FrontierItem::Grain(GrainId::new("deferred")),
            FrontierReason::BudgetLimit,
            "deferred by tiny fixture grain budget",
        ));

        assert_eq!(frontier.status(), BudgetStatus::FrontierDeferred);
        assert_eq!(frontier.records()[0].reason.as_str(), "budget_limit");
    }

    #[test]
    fn budget_failure_is_not_success_shaped() {
        let failure = BudgetFailure::new(
            FrontierReason::RequiredClosureExceedsBudget,
            ContextBudget::tiny_fixture(),
            1,
            ClosureReceipt::new("budget-check", "required closure exceeds budget"),
        );

        assert_eq!(failure.status(), BudgetStatus::BudgetFailure);
        assert_eq!(failure.reason.as_str(), "required_closure_exceeds_budget");
    }

    #[test]
    fn tiny_fixture_report_skeleton_records_l1_caveat() {
        let report = FixtureReportSkeleton::tiny("lattice fixture tiny");

        assert_eq!(report.tier.as_str(), "tiny");
        assert_eq!(report.tier.target_grain_count(), 10);
        assert_eq!(report.tier.target_bond_count(), 20);
        assert!(report
            .caveats
            .contains(&"L1 skeleton only; no context correctness claim".to_string()));
    }

    #[test]
    fn shard_manifest_hash_is_deterministic_and_status_typed() {
        let first = ShardManifest::new(
            "search-docs-us-2026-q2",
            "search/docs/us/2026-q2",
            "derived_text_allowed",
            24_000,
            120_000,
            4,
            0,
            "lattice-stable-v1:closed-cut:search-docs-us-2026-q2",
            ShardStatus::Closed,
        );
        let second = ShardManifest::new(
            "search-docs-us-2026-q2",
            "search/docs/us/2026-q2",
            "derived_text_allowed",
            24_000,
            120_000,
            4,
            0,
            "lattice-stable-v1:closed-cut:search-docs-us-2026-q2",
            ShardStatus::Closed,
        );

        assert_eq!(first.status.as_str(), "closed");
        assert_eq!(first.manifest_hash, second.manifest_hash);
        assert!(first.manifest_hash.starts_with("lattice-stable-v1:"));
    }

    #[test]
    fn shard_boundary_edges_preserve_frontier_and_conflict_states() {
        let frontier = ShardBoundaryEdge::new(
            "edge-engineering-release-frontier",
            "engineering-notes-2026-q2",
            "release-artifacts-2026-q2",
            ShardBoundaryKind::Cites,
            "receipt:boundary:engineering-release",
            false,
            ShardAlignmentStatus::Frontier,
        );
        let conflict = ShardBoundaryEdge::new(
            "edge-support-engineering-conflict",
            "support-kb-2026-q2",
            "engineering-notes-2026-q2",
            ShardBoundaryKind::Conflicts,
            "receipt:boundary:support-engineering",
            false,
            ShardAlignmentStatus::Conflict,
        );

        assert_eq!(frontier.kind.as_str(), "cites");
        assert_eq!(frontier.alignment_status.as_str(), "frontier");
        assert_eq!(conflict.kind.as_str(), "conflicts");
        assert_eq!(conflict.alignment_status.as_str(), "conflict");
        assert_ne!(frontier.stable_hash(), conflict.stable_hash());
    }

    #[test]
    fn shard_route_decision_hash_preserves_status_and_evidence() {
        let selected = ShardRouteDecision::new(
            "support-kb-2026-q2",
            ShardRouteStatus::Selected,
            92,
            "query mentions support readiness",
            vec!["edge-search-us-support-bridge".to_string()],
        );
        let frontier = ShardRouteDecision::new(
            "engineering-notes-2026-q2",
            ShardRouteStatus::Frontier,
            88,
            "relevant but rights frontiered",
            vec!["edge-support-engineering-conflict".to_string()],
        );

        assert_eq!(selected.status.as_str(), "selected");
        assert_eq!(frontier.status.as_str(), "frontier");
        assert_ne!(selected.stable_hash(), frontier.stable_hash());
    }

    #[test]
    fn shard_scenario_example_hash_preserves_frontier_and_edges() {
        let scenario = ShardScenarioExample::new(
            "support-conflict",
            "Support conflict route",
            "release readiness with support conflicts",
            vec!["support-kb-2026-q2".to_string()],
            vec!["engineering-notes-2026-q2".to_string()],
            vec!["edge-support-engineering-conflict".to_string()],
            "route support evidence and preserve engineering notes as frontier",
        );

        assert_eq!(scenario.frontier_shards.len(), 1);
        assert!(scenario.stable_hash().starts_with("lattice-stable-v1:"));
    }

    #[test]
    fn tiny_model_fixture_is_deterministic_and_budget_sized() {
        let fixture = TinyModelFixture::from_source(
            "fontes:apache-calcite:query-planning",
            "derived_text_allowed",
        );

        assert_eq!(fixture.grains.len(), FixtureTier::Tiny.target_grain_count());
        assert_eq!(fixture.bonds.len(), FixtureTier::Tiny.target_bond_count());
        assert_eq!(fixture.grains[0].kind, GrainKind::SourcePointer);
        assert_eq!(
            fixture.grains[0].source_id.as_deref(),
            Some("fontes:apache-calcite:query-planning")
        );
        assert!(fixture.bonds.iter().all(|bond| {
            fixture.grains.iter().any(|grain| grain.id == bond.from)
                && fixture.grains.iter().any(|grain| grain.id == bond.to)
        }));
    }

    #[test]
    fn launch_readiness_fixture_parses_departments_and_bond_origins() {
        let fixture = LaunchReadinessFixture::parse_default();

        assert_eq!(fixture.department_count(), 5);
        assert_eq!(fixture.fact_count(), 240);
        assert_eq!(fixture.grains.len(), 240);
        assert_eq!(fixture.decision_cut_fact_count(), 107);
        assert!(fixture.auto_bond_count() > fixture.guidance_bond_count());
        assert_eq!(fixture.guidance_bond_count(), 91);
        assert!(fixture.auto_bond_ratio() > 0.9);
        assert!(fixture.decision_cut_ratio() < 0.5);
        assert!(fixture
            .facts
            .iter()
            .any(|fact| fact.department == LaunchDepartment::Compliance
                && fact.tags.contains(&"rights".to_string())));
    }

    #[test]
    fn launch_readiness_fixture_scales_deterministically() {
        let fixture = LaunchReadinessFixture::parse_with_generated_per_department(95);

        assert_eq!(fixture.department_count(), 5);
        assert_eq!(fixture.fact_count(), 490);
        assert_eq!(fixture.decision_cut_fact_count(), 212);
        assert_eq!(fixture.guidance_bond_count(), 174);
        assert!(fixture.auto_bond_count() > 10_000);
        assert!(fixture.decision_cut_ratio() < 0.45);
    }

    #[test]
    fn pack_prompt_and_press_frames_preserve_closed_cut_metadata() {
        let cut = ClosedCut::new(CutId::new("tiny-closed"))
            .with_grain(GrainId::new("source"))
            .with_receipt(ClosureReceipt::new("custody", "source pointer retained"))
            .with_policy("tiny-policy", "derived_text_allowed");

        let pack = ContextPack::from_closed_cut(&cut, "tiny-pack");
        let prompt = PromptFrame::from_closed_cut(&cut, "stable-prefix");
        let press = PressPublicationFrame::from_closed_cut(&cut, "press-handoff", "docx");

        assert_eq!(pack.id, "pack:tiny-pack:tiny-closed");
        assert!(pack.cut_hash.starts_with("lattice-stable-v1:"));
        assert!(pack.receipt_hash.starts_with("lattice-stable-v1:"));
        assert!(pack.cache_prefix.starts_with("lattice-cache-v1:tiny-pack:"));
        assert_eq!(pack.receipt_count, 1);
        assert_eq!(prompt.contract, "closed-cut-only");
        assert_eq!(prompt.grain_ids, vec!["source"]);
        assert_eq!(press.frame_version, "lattice.press-frame.v1");
        assert!(press.handoff_contract.contains("file-contract-only"));
        assert_eq!(
            press.pack.rights_policy.as_deref(),
            Some("derived_text_allowed")
        );
    }

    #[test]
    fn profile_modes_are_extensible_weighted_contracts() {
        let profiles = lattice_profile_modes();

        assert_eq!(profiles.len(), 12);
        assert!(profiles.iter().all(|profile| profile.total_weight() == 100));
        assert!(profiles
            .iter()
            .all(|profile| profile.closure_required && !profile.metric_weights.is_empty()));
        assert!(profiles.iter().any(|profile| profile.id == "fast-answer"
            && profile.latency_class == "fast"
            && profile.pack_profile == "answer-grounding"));
        assert!(profiles.iter().any(|profile| profile.id == "audit-grade"
            && profile.audit_level == "full"
            && profile.pack_profile == "coverage-audit"));
        assert!(profiles
            .iter()
            .any(|profile| profile.id == "panel-review" && profile.context_width == "wide"));
    }

    #[test]
    fn boundary_metrics_score_closed_cut_shape() {
        let a = GrainId::new("a");
        let b = GrainId::new("b");
        let c = GrainId::new("c");
        let d = GrainId::new("d");
        let universe = BTreeSet::from([a.clone(), b.clone(), c.clone(), d.clone()]);
        let bonds = BTreeSet::from([
            Bond::new(a.clone(), b.clone(), BondKind::Requires),
            Bond::new(b.clone(), c.clone(), BondKind::Requires),
            Bond::new(c.clone(), d.clone(), BondKind::Requires),
        ]);
        let selected = BTreeSet::from([a, b]);

        let metrics = BoundaryMetrics::from_bonds(&universe, &bonds, &selected)
            .expect("selected grains are in universe");

        assert_eq!(metrics.selected_count, 2);
        assert_eq!(metrics.complement_count, 2);
        assert_eq!(metrics.selected_internal_bonds, 1);
        assert_eq!(metrics.complement_internal_bonds, 1);
        assert_eq!(metrics.boundary_bonds, 1);
        assert_eq!(metrics.total_bonds, 3);
        assert_eq!(metrics.conductance, 1.0 / 3.0);
    }

    #[test]
    fn candidate_metrics_show_recall_precision_and_closure_rescue() {
        let expected = BTreeSet::from([GrainId::new("a"), GrainId::new("b"), GrainId::new("c")]);
        let candidates = BTreeSet::from([GrainId::new("a"), GrainId::new("b"), GrainId::new("d")]);
        let closed = BTreeSet::from([
            GrainId::new("a"),
            GrainId::new("b"),
            GrainId::new("c"),
            GrainId::new("policy"),
        ]);
        let frontier = BTreeSet::from([GrainId::new("c")]);

        let metrics =
            CandidateQualityMetrics::from_sets(&expected, &candidates, &closed, &frontier);

        assert_eq!(metrics.true_positive_count, 2);
        assert_eq!(metrics.missed_expected_count, 1);
        assert_eq!(metrics.distractor_count, 1);
        assert_eq!(metrics.closure_rescue_count, 2);
        assert_eq!(metrics.frontier_false_negative_count, 1);
        assert_eq!(metrics.candidate_recall, 2.0 / 3.0);
        assert_eq!(metrics.candidate_precision, 2.0 / 3.0);
    }

    #[test]
    fn context_metrics_report_budget_usage_and_counts() {
        let cut = ClosedCut::new(CutId::new("tiny"))
            .with_grain(GrainId::new("a"))
            .with_receipt(ClosureReceipt::new("closure", "verified"));
        let budget = ContextBudget::tiny_fixture();
        let mut frontier = Frontier::new();
        frontier.record(FrontierRecord::new(
            FrontierItem::Grain(GrainId::new("b")),
            FrontierReason::BudgetLimit,
            "deferred by budget",
        ));

        let metrics =
            ContextMetrics::from_closed_cut(&cut, &budget, &frontier, 7, 512, Some(250), 1);

        assert_eq!(metrics.elapsed_ms, 7);
        assert_eq!(metrics.grain_count, 1);
        assert_eq!(metrics.receipt_count, 1);
        assert_eq!(metrics.frontier_count, 1);
        assert_eq!(metrics.budget_used_ratio, Some(0.25));
        assert_eq!(metrics.budget_status, BudgetStatus::FrontierDeferred);
    }

    #[test]
    fn closure_receipt_hash_is_stable_and_semantic() {
        let receipt = ClosureReceipt::new("closure-v1", "verified");
        let same = ClosureReceipt::new("closure-v1", "verified");
        let changed = ClosureReceipt::new("closure-v1", "frontiered");

        assert_eq!(receipt.stable_hash(), same.stable_hash());
        assert_ne!(receipt.stable_hash(), changed.stable_hash());
        assert!(receipt.stable_hash().starts_with("lattice-stable-v1:"));
    }

    #[test]
    fn closed_cut_hash_is_stable_across_insertion_order() {
        let a = Grain::new(GrainId::new("a"), "A").with_metadata(
            GrainKind::Context,
            "source",
            "derived_text_allowed",
        );
        let b = Grain::new(GrainId::new("b"), "B").with_metadata(
            GrainKind::Evidence,
            "source",
            "derived_text_allowed",
        );
        let bond = Bond::new(a.id.clone(), b.id.clone(), BondKind::Requires);

        let mut first = ClosedCut::new(CutId::new("cut"))
            .with_grain(a.id.clone())
            .with_grain(b.id.clone())
            .with_bond(bond.clone())
            .with_receipt(ClosureReceipt::new("closure-v1", "verified"))
            .with_policy("closure-v1", "derived_text_allowed");
        first.grain_records = vec![a.clone(), b.clone()];

        let mut second = ClosedCut::new(CutId::new("cut"))
            .with_grain(b.id.clone())
            .with_grain(a.id.clone())
            .with_bond(bond)
            .with_receipt(ClosureReceipt::new("closure-v1", "verified"))
            .with_policy("closure-v1", "derived_text_allowed");
        second.grain_records = vec![b, a];

        assert_eq!(first.stable_hash(), second.stable_hash());
        assert_eq!(first.receipt_hash(), second.receipt_hash());
    }
}
