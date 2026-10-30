use super::*;

impl CanonicalSegment {
    pub(crate) fn previous_segment_hash(&self) -> Option<[u8; 32]> {
        self.previous_segment_hash
    }
}

impl SegmentReceiver {
    pub(crate) fn accept_retained_anchor(
        &mut self,
        segment: &SignedSegment,
        identity: &RepositoryNodeIdentity,
    ) -> Result<Acceptance, ProtocolError> {
        self.accept_retained_anchor_inner(segment, identity, false)
    }

    pub(crate) fn accept_retained_anchor_with_sequence_gap(
        &mut self,
        segment: &SignedSegment,
        identity: &RepositoryNodeIdentity,
    ) -> Result<Acceptance, ProtocolError> {
        self.accept_retained_anchor_inner(segment, identity, true)
    }

    pub(crate) fn accept_retained_historical(
        &mut self,
        segment: &SignedSegment,
        identity: &RepositoryNodeIdentity,
        allow_sequence_gap: bool,
        is_deleted: impl FnMut(&Cursor, &SyncRecord) -> bool,
    ) -> Result<Acceptance, ProtocolError> {
        self.retained_anchor_mode = true;
        self.retained_anchor_sequence_gap = allow_sequence_gap;
        let result = self.accept_historical(segment, identity, is_deleted);
        self.retained_anchor_mode = false;
        self.retained_anchor_sequence_gap = false;
        result
    }

    fn accept_retained_anchor_inner(
        &mut self,
        segment: &SignedSegment,
        identity: &RepositoryNodeIdentity,
        allow_sequence_gap: bool,
    ) -> Result<Acceptance, ProtocolError> {
        self.retained_anchor_mode = true;
        self.retained_anchor_sequence_gap = allow_sequence_gap;
        let result = self.accept(segment, identity);
        self.retained_anchor_mode = false;
        self.retained_anchor_sequence_gap = false;
        result
    }
}
