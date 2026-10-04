//! Small public context algebra fixture over native custody closure and meet/join.
use lattice_model::{
    Bond, BondKind, ClosedCut, ContextBudget, CutId, Frontier, Grain, GrainId, GrainKind,
};
use lattice_order::{close_candidate_cut, join_closed, meet_closed, CandidateCut, ClosureResult};
use serde_json::{json, Value};
fn candidate(mask: u32, id: &str) -> CandidateCut {
    let mut c = CandidateCut::new(CutId::new(id));
    for (id, label, kind) in [
        ("source", "Public sample source", GrainKind::SourcePointer),
        ("policy", "Sample use policy", GrainKind::Policy),
        ("receipt", "Source receipt", GrainKind::Receipt),
    ] {
        c = c.with_grain(Grain::new(GrainId::new(id), label).with_metadata(
            kind,
            "public-browser-sample",
            "derived_text_allowed",
        ));
    }
    for (i, label) in [
        "Task intent",
        "Search design",
        "Validation plan",
        "Interface notes",
        "Supporting evidence",
    ]
    .iter()
    .enumerate()
    {
        if mask & (1 << i) != 0 {
            let grain_id = GrainId::new(format!("optional-{i}"));
            c = c
                .with_grain(Grain::new(grain_id.clone(), *label).with_metadata(
                    if i == 4 {
                        GrainKind::Evidence
                    } else {
                        GrainKind::Context
                    },
                    "public-browser-sample",
                    "derived_text_allowed",
                ))
                .with_bond(Bond::new(
                    GrainId::new("source"),
                    grain_id,
                    BondKind::Contains,
                ));
        }
    }
    c
}
fn unpack(result: ClosureResult) -> Result<(ClosedCut, Frontier), String> {
    match result {
        ClosureResult::Closed { cut, .. } => Ok((cut, Frontier::new())),
        ClosureResult::ClosedWithFrontier { cut, frontier, .. } => Ok((cut, frontier)),
        ClosureResult::BudgetFailure(f) => Err(f.receipt.note),
    }
}
fn describe(cut: &ClosedCut, frontier: &Frontier) -> Value {
    json!({"grain_count":cut.grains.len(),"bond_count":cut.bonds.len(),"receipt_hash":cut.receipt_hash(),"cut_hash":cut.stable_hash(),"grains":cut.grain_records.iter().map(|g|json!({"id":g.id.as_str(),"label":g.label,"kind":g.kind.as_str()})).collect::<Vec<_>>(),"receipts":cut.closure_receipts.iter().map(|r|json!({"rule":r.rule,"note":r.note,"hash":r.stable_hash()})).collect::<Vec<_>>(),"frontier":frontier.records().iter().map(|r|json!({"item":format!("{:?}",r.item),"reason":format!("{:?}",r.reason),"note":r.note})).collect::<Vec<_>>()})
}
pub fn evaluate(left: u32, right: u32, limit: usize, operation: &str) -> Result<Value, String> {
    if left > 31
        || right > 31
        || !(1..=8).contains(&limit)
        || !["join", "meet"].contains(&operation)
    {
        return Err("Choose five sample items, a 1–8 grain budget, and meet or join".into());
    }
    let mut budget = ContextBudget::unbounded();
    budget.grain_limit = Some(limit);
    let (a, fa) = unpack(close_candidate_cut(candidate(left, "sample-a"), &budget))?;
    let (b, fb) = unpack(close_candidate_cut(candidate(right, "sample-b"), &budget))?;
    let (result, frontier) = unpack(if operation == "join" {
        join_closed(&a, &b, &budget)
    } else {
        meet_closed(&a, &b, &budget)
    })?;
    Ok(
        json!({"schema":"lattice.browser.v1","synthetic":true,"operation":operation,"budget":limit,"left":describe(&a,&fa),"right":describe(&b,&fb),"result":describe(&result,&frontier)}),
    )
}
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn evaluate_json(
    left: u32,
    right: u32,
    limit: usize,
    operation: &str,
) -> Result<String, wasm_bindgen::JsValue> {
    evaluate(left, right, limit, operation)
        .map(|v| v.to_string())
        .map_err(|e| wasm_bindgen::JsValue::from_str(&e))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_join_meet_receipts_and_budget_are_deterministic() {
        let join = evaluate(3, 6, 8, "join").unwrap();
        let meet = evaluate(3, 6, 8, "meet").unwrap();
        assert_eq!(join["result"]["grain_count"], 6);
        assert_eq!(meet["result"]["grain_count"], 4);
        assert_eq!(join, evaluate(3, 6, 8, "join").unwrap());
        let tight = evaluate(31, 31, 3, "join").unwrap();
        assert_eq!(tight["result"]["grain_count"], 3);
        assert!(!tight["left"]["frontier"].as_array().unwrap().is_empty());
        assert!(evaluate(31, 31, 2, "join").is_err());
        assert!(evaluate(32, 0, 8, "meet").is_err());
    }
}
