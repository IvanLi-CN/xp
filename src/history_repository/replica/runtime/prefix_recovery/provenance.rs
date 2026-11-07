use super::*;

fn record_hash(record: &StoredRecord) -> Result<[u8; 32], RepositoryRuntimeError> {
    Ok(
        Sha256::digest(serde_json::to_vec(record).map_err(|error| invalid(&error.to_string()))?)
            .into(),
    )
}

pub(crate) fn record_key(record: &StoredRecord) -> Result<String, RepositoryRuntimeError> {
    Ok(hex::encode(record_hash(record)?))
}

fn record_proof_bytes(
    bundle_digest: &str,
    record: &StoredRecord,
) -> Result<Vec<u8>, RepositoryRuntimeError> {
    let digest = hex::decode(bundle_digest).map_err(|_| invalid("invalid inventory digest"))?;
    if digest.len() != 32 {
        return Err(invalid("invalid inventory digest length"));
    }
    let mut bytes = b"xp-source-prefix-aggregate-v1\0".to_vec();
    bytes.extend(digest);
    bytes.extend(record_hash(record)?);
    Ok(bytes)
}

pub(crate) fn attest_record(
    bundle_digest: &str,
    record: &StoredRecord,
    key: &ed25519_dalek::SigningKey,
) -> Result<String, RepositoryRuntimeError> {
    Ok(URL_SAFE_NO_PAD.encode(
        key.sign(&record_proof_bytes(bundle_digest, record)?)
            .to_bytes(),
    ))
}

fn verify_signature(
    identity: &RepositoryNodeIdentity,
    message: &[u8],
    signature: &str,
) -> Result<(), RepositoryRuntimeError> {
    let key = ed25519_dalek::VerifyingKey::from_bytes(identity.ed25519_public_key().as_bytes())
        .map_err(|_| invalid("invalid aggregate proof key"))?;
    let signature = URL_SAFE_NO_PAD
        .decode(signature)
        .map_err(|_| invalid("invalid aggregate proof encoding"))?;
    let signature = ed25519_dalek::Signature::from_slice(&signature)
        .map_err(|_| invalid("invalid aggregate proof signature"))?;
    key.verify_strict(message, &signature)
        .map_err(|_| invalid("aggregate source proof mismatch"))
}

pub(super) fn verify_record(
    identity: &RepositoryNodeIdentity,
    bundle_digest: &str,
    record: &StoredRecord,
    signature: &str,
) -> Result<(), RepositoryRuntimeError> {
    if record.source_node_id != identity.node_id().as_str() {
        return Err(invalid("aggregate proof source mismatch"));
    }
    verify_signature(
        identity,
        &record_proof_bytes(bundle_digest, record)?,
        signature,
    )
}

impl PrefixRecoveryEvidence {
    pub(crate) fn verify(&self) -> Result<(), RepositoryRuntimeError> {
        let digest = hex::decode(&self.bundle_digest)
            .map_err(|_| invalid("invalid evidence inventory digest"))?;
        if digest.len() != 32
            || self.retained_record_signatures.len() != self.retained_records.len()
        {
            return Err(invalid("incomplete retained aggregate proofs"));
        }
        verify_signature(&self.identity, &digest, &self.source_signature)?;
        for record in &self.retained_records {
            let signature = self
                .retained_record_signatures
                .get(&record_key(record)?)
                .ok_or_else(|| invalid("retained aggregate proof missing"))?;
            verify_record(&self.identity, &self.bundle_digest, record, signature)?;
        }
        Ok(())
    }

    pub(crate) fn delete_records(
        &mut self,
        key: &ReplicaRecordKey,
        prefix: bool,
    ) -> Result<(), RepositoryRuntimeError> {
        let mut removed = Vec::new();
        for record in &self.retained_records {
            if record.matches_tombstone_key(key, prefix) {
                removed.push(record_key(record)?);
            }
        }
        self.retained_records
            .retain(|record| !record.matches_tombstone_key(key, prefix));
        for digest in removed {
            self.retained_record_signatures.remove(&digest);
        }
        Ok(())
    }
}
