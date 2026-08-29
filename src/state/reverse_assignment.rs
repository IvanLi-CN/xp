use super::StoreError;

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
