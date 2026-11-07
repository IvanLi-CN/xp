use super::*;
use crate::state::history_storage::SourceDeliveryJournalPage;

impl RepositoryReplicaRuntime {
    pub(crate) fn original_source_page(
        &self,
    ) -> Result<Vec<RepositoryReplicaSegment>, RepositoryRuntimeError> {
        if !self.storage.is_sqlite() {
            return Ok(self.local_source_pending_segments_page_with_budget(32, 1024 * 1024));
        }
        match self
            .storage
            .source_delivery_journal_page_with_budget(32, 1024 * 1024)
            .map_err(|error| RepositoryRuntimeError::Storage(error.to_string()))?
        {
            SourceDeliveryJournalPage::Ready(rows) => Ok(rows
                .into_iter()
                .map(|row| RepositoryReplicaSegment {
                    identity: row.identity,
                    wire: row.wire,
                })
                .collect()),
            SourceDeliveryJournalPage::Repairing => Err(RepositoryRuntimeError::Storage(
                "source journal order repair is incomplete".to_owned(),
            )),
        }
    }

    pub(crate) fn confirmed_original_segment_ids(
        &self,
        ids: &[String],
    ) -> Result<Vec<String>, RepositoryRuntimeError> {
        if ids.len() > 32 {
            return Err(RepositoryRuntimeError::StateLimitExceeded);
        }
        let Some(receiver) = self.receiver.as_ref() else {
            return Ok(Vec::new());
        };
        let mut confirmed = Vec::new();
        for stored in self.stored_segments_by_ids(ids)? {
            if hex::encode(Sha256::digest(&stored.wire)) != stored.id {
                return Err(RepositoryRuntimeError::Storage(
                    "stored original wire digest mismatch".to_owned(),
                ));
            }
            let signed = SignedSegment::from_wire(&stored.wire)?;
            signed.verify_identity(&stored.identity)?;
            if let Some((tip, _)) =
                receiver.continuous_watermark_with_hash(signed.canonical().last_cursor())?
                && tip.sequence() >= signed.canonical().last_cursor().sequence()
            {
                confirmed.push(stored.id);
            }
        }
        confirmed.sort();
        Ok(confirmed)
    }
}
