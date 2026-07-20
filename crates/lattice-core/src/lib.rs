#![forbid(unsafe_code)]

pub use lattice_model::{
    Bond, BondKind, BudgetFailure, BudgetStatus, ClosedCut, ClosureReceipt, ContextBudget, CutId,
    FixtureFamily, FixtureReportSkeleton, FixtureTier, Frontier, FrontierItem, FrontierReason,
    FrontierRecord, Grain, GrainId, GrainKind, TinyModelFixture,
};

pub const ALLOWED_EXTERNAL_UPSTREAM: &str = "https://github.com/giodl73-repo/METIS-CORE";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DependencyPolicy {
    pub product_neutral: bool,
    pub allowed_external_upstreams: &'static [&'static str],
}

impl DependencyPolicy {
    pub const fn public_default() -> Self {
        Self {
            product_neutral: true,
            allowed_external_upstreams: &[ALLOWED_EXTERNAL_UPSTREAM],
        }
    }

    pub fn allows_external_upstream(&self, upstream: &str) -> bool {
        self.allowed_external_upstreams
            .iter()
            .any(|allowed| upstream.eq_ignore_ascii_case(allowed))
    }
}

pub fn dependency_policy() -> DependencyPolicy {
    DependencyPolicy::public_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_allows_only_metis_core() {
        let policy = dependency_policy();

        assert!(policy.product_neutral);
        assert!(policy.allows_external_upstream(ALLOWED_EXTERNAL_UPSTREAM));
        assert!(!policy.allows_external_upstream("https://github.com/giodl73-repo/RLINE"));
    }
}
