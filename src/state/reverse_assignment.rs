use super::StoreError;
use crate::reverse_mesh::ReverseMeshAssignment;

pub(super) fn is_identical_stale_replay(
    current: Option<&ReverseMeshAssignment>,
    expected_generation: &Option<u64>,
    assignment: &ReverseMeshAssignment,
) -> bool {
    current == Some(assignment)
        && expected_generation.is_some_and(|expected| expected < assignment.generation)
}

pub(super) fn generation_cas_is_stale_replay_with_assignment(
    current: Option<&ReverseMeshAssignment>,
    expected_generation: &Option<u64>,
    assignment: &ReverseMeshAssignment,
    generation_floor: u64,
) -> Result<bool, StoreError> {
    if is_identical_stale_replay(current, expected_generation, assignment) {
        return Ok(true);
    }
    generation_cas_is_stale_replay(
        current.map(|item| item.generation),
        expected_generation,
        assignment.generation,
        generation_floor,
    )
}

pub(super) fn generation_cas_is_stale_replay(
    current_generation: Option<u64>,
    expected_generation: &Option<u64>,
    assignment_generation: u64,
    generation_floor: u64,
) -> Result<bool, StoreError> {
    let mismatch = expected_generation.is_some() && current_generation != *expected_generation;
    if !mismatch {
        return Ok(false);
    }
    if current_generation.is_none() && generation_floor == 0 {
        return Ok(false);
    }
    let stale_replay = expected_generation == &Some(assignment_generation)
        && current_generation.is_none()
        && generation_floor == assignment_generation;
    if stale_replay {
        return Ok(true);
    }
    Err(StoreError::Migration {
        message: "reverse mesh assignment generation CAS failed".to_string(),
    })
}
