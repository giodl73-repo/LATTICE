#![forbid(unsafe_code)]

use metis_core::{part_kway, MetisParams, PartitionError};

pub fn partition_path(vertex_count: usize, part_count: u32) -> Result<Vec<u32>, PartitionError> {
    let (xadj, adjncy) = path_graph(vertex_count);
    part_kway(&xadj, &adjncy, &[], &[], part_count, MetisParams::default())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidatePartition {
    pub assignments: Vec<u32>,
    pub candidate_only: bool,
    pub note: String,
}

impl CandidatePartition {
    pub fn is_final_context(&self) -> bool {
        false
    }
}

pub fn partition_path_candidates(
    vertex_count: usize,
    part_count: u32,
) -> Result<CandidatePartition, PartitionError> {
    let assignments = partition_path(vertex_count, part_count)?;
    Ok(CandidatePartition {
        assignments,
        candidate_only: true,
        note: "METIS-CORE partition output is candidate structure only; close before AI use"
            .to_string(),
    })
}

fn path_graph(vertex_count: usize) -> (Vec<u32>, Vec<u32>) {
    let mut xadj = Vec::with_capacity(vertex_count + 1);
    let mut adjncy = Vec::new();
    xadj.push(0);
    for vertex in 0..vertex_count {
        if vertex > 0 {
            adjncy.push((vertex - 1) as u32);
        }
        if vertex + 1 < vertex_count {
            adjncy.push((vertex + 1) as u32);
        }
        xadj.push(adjncy.len() as u32);
    }
    (xadj, adjncy)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metis_core_partitions_path_graph() {
        let assignment = partition_path(8, 2).expect("METIS-CORE should partition a path graph");

        assert_eq!(assignment.len(), 8);
        assert!(assignment.contains(&0));
        assert!(assignment.contains(&1));
    }

    #[test]
    fn metis_candidate_partition_is_not_final_context() {
        let partition =
            partition_path_candidates(8, 2).expect("METIS-CORE should partition a path graph");

        assert_eq!(partition.assignments.len(), 8);
        assert!(partition.candidate_only);
        assert!(!partition.is_final_context());
        assert!(partition.note.contains("candidate structure only"));
    }
}
