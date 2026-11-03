use super::*;

impl RepositoryReplicaRuntime {
    pub(super) fn legacy_recovery_freshness(
        &self,
        cluster: &str,
        identity: &RepositoryNodeIdentity,
        now: u64,
        authority: RetainedAuthority<'_>,
    ) -> Result<(), RepositoryRuntimeError> {
        let active = self.retained_stale_generation_for_page(cluster, identity)?;
        if active || authority.is_some() {
            let (peers, generation) = authority.ok_or_else(|| {
                failure("legacy stale recovery requires current Ready authorization")
            })?;
            if !self.retained_stale_page_authorized(cluster, identity, peers, generation, now)? {
                return Err(failure("legacy stale recovery Ready authorization changed"));
            }
            return Ok(());
        }
        match self.snapshot.last_verified_unix_seconds {
            Some(time) if time > now || now - time >= TOMBSTONE_HORIZON_SECONDS => Err(failure(
                "legacy recovery cannot preserve an expired receiver deletion ledger",
            )),
            None if self.receiver.is_some() || self.snapshot.receiver.is_some() => Err(failure(
                "legacy recovery receiver deletion freshness is unknown",
            )),
            _ => Ok(()),
        }
    }

    pub(crate) fn legacy_original_recovery_state_with_retained_baseline(
        &mut self,
        cluster: &str,
        identity: &RepositoryNodeIdentity,
        now: u64,
        peers: &[(String, [u8; 32])],
        generation: u64,
    ) -> Result<LegacyRecoveryState, RepositoryRuntimeError> {
        self.legacy_original_recovery_state_inner(cluster, identity, now, Some((peers, generation)))
    }
}
