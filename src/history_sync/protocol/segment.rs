use super::CanonicalSegment;

impl CanonicalSegment {
    pub(crate) fn cluster_id(&self) -> &str {
        &self.cluster_id
    }
}
