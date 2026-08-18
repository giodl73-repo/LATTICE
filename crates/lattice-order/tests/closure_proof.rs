use lattice_model::{ContextBudget, CutId, TinyModelFixture};
use lattice_order::{close_candidate_cut, CandidateCut, ClosureResult};
use std::collections::BTreeMap;

fn expected() -> BTreeMap<&'static str, &'static str> {
    include_str!("fixtures/closure_proof.txt")
        .lines()
        .map(|line| line.split_once('=').expect("proof fixture uses key=value"))
        .collect()
}

fn tiny_candidate() -> CandidateCut {
    let fixture = TinyModelFixture::from_source("fontes:proof", "derived_text_allowed");
    let candidate = fixture.grains.into_iter().fold(
        CandidateCut::new(CutId::new("closure-proof")),
        |candidate, grain| candidate.with_grain(grain),
    );
    fixture
        .bonds
        .into_iter()
        .fold(candidate, |candidate, bond| candidate.with_bond(bond))
}

#[test]
fn closure_proof_accepts_tiny_fixture() {
    let expected = expected();
    let result = close_candidate_cut(tiny_candidate(), &ContextBudget::tiny_fixture());

    let ClosureResult::Closed { cut, budget_status } = result else {
        panic!("proof fixture should close without a frontier");
    };
    assert_eq!(budget_status.as_str(), expected["accepted.status"]);
    assert_eq!(
        cut.grains.len().to_string(),
        expected["accepted.grain_count"]
    );
    assert_eq!(cut.bonds.len().to_string(), expected["accepted.bond_count"]);
    assert_eq!(
        cut.closure_receipts.len().to_string(),
        expected["accepted.receipt_count"]
    );
}

#[test]
fn closure_proof_rejects_required_closure_over_budget() {
    let expected = expected();
    let budget = ContextBudget {
        grain_limit: Some(expected["failure.grain_limit"].parse().unwrap()),
        ..ContextBudget::tiny_fixture()
    };
    let result = close_candidate_cut(tiny_candidate(), &budget);

    let ClosureResult::BudgetFailure(failure) = result else {
        panic!("required closure should not be silently truncated");
    };
    assert_eq!(failure.status().as_str(), expected["failure.status"]);
    assert_eq!(failure.reason.as_str(), expected["failure.reason"]);
    assert_eq!(
        failure.budget.grain_limit,
        Some(expected["failure.grain_limit"].parse().unwrap())
    );
}
