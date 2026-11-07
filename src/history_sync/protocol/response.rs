use super::*;

impl EncodedResponse {
    pub(crate) fn encode(canonical: Vec<u8>) -> Result<Self, ProtocolError> {
        let canonical_len = canonical.len();
        let (encoding, wire) = encoding::encode(canonical)?;
        Ok(Self {
            encoding,
            canonical_len,
            wire,
        })
    }
    pub(crate) fn from_wire(
        encoding: PayloadEncoding,
        canonical_len: usize,
        wire: Vec<u8>,
    ) -> Result<Self, ProtocolError> {
        encoding::validate_wire(encoding, canonical_len, &wire)?;
        Ok(Self {
            encoding,
            wire,
            canonical_len,
        })
    }

    pub(crate) fn encoding(&self) -> PayloadEncoding {
        self.encoding
    }

    pub(crate) fn wire(&self) -> &[u8] {
        &self.wire
    }

    pub(crate) fn decode(&self) -> Result<Vec<u8>, ProtocolError> {
        encoding::decode(self.encoding, &self.wire, self.canonical_len)
    }
}
