use chacha20poly1305::{
    ChaCha20Poly1305, Key, KeyInit as _, Nonce,
    aead::{Aead as _, Payload},
};
use sha2::{Digest as _, Sha256};
use x25519_dalek::{PublicKey, StaticSecret};

const MAX_RELAY_FRAME_BYTES: usize = 256 * 1024;
const RELAY_ENVELOPE_BYTES: usize = 32 + 12;
const AEAD_TAG_BYTES: usize = 16;
const RELAY_KEY_CONTEXT: &[u8] = b"xp-history-sync-relay-v1";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct RelayTrafficBytes(u64);

impl RelayTrafficBytes {
    pub(crate) fn count(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct SyncControlBytes(u64);

impl SyncControlBytes {
    pub(crate) fn count(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RelayKeypair {
    private_key: [u8; 32],
    public_key: [u8; 32],
}

impl RelayKeypair {
    pub(crate) fn from_private_key(private_key: [u8; 32]) -> Self {
        let secret = StaticSecret::from(private_key);
        Self {
            private_key,
            public_key: PublicKey::from(&secret).to_bytes(),
        }
    }

    pub(crate) fn public_key(&self) -> [u8; 32] {
        self.public_key
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RelayFrame {
    sender_public_key: [u8; 32],
    nonce: [u8; 12],
    ciphertext: Vec<u8>,
}

impl RelayFrame {
    pub(crate) fn seal(
        sender: RelayKeypair,
        recipient_public_key: [u8; 32],
        nonce: [u8; 12],
        plaintext: &[u8],
        associated_data: &[u8],
    ) -> Result<Self, RelayError> {
        if plaintext.len()
            > MAX_RELAY_FRAME_BYTES.saturating_sub(RELAY_ENVELOPE_BYTES + AEAD_TAG_BYTES)
        {
            return Err(RelayError::FrameLimit);
        }
        let cipher = relay_cipher(sender.private_key, recipient_public_key);
        let ciphertext = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: plaintext,
                    aad: associated_data,
                },
            )
            .map_err(|_| RelayError::AuthenticationFailed)?;
        Ok(Self {
            sender_public_key: sender.public_key,
            nonce,
            ciphertext,
        })
    }

    pub(crate) fn open(
        &self,
        recipient: RelayKeypair,
        associated_data: &[u8],
    ) -> Result<Vec<u8>, RelayError> {
        if self.wire_len() > MAX_RELAY_FRAME_BYTES {
            return Err(RelayError::FrameLimit);
        }
        let cipher = relay_cipher(recipient.private_key, self.sender_public_key);
        cipher
            .decrypt(
                Nonce::from_slice(&self.nonce),
                Payload {
                    msg: &self.ciphertext,
                    aad: associated_data,
                },
            )
            .map_err(|_| RelayError::AuthenticationFailed)
    }

    pub(crate) fn tamper_for_test(&mut self) {
        if let Some(byte) = self.ciphertext.first_mut() {
            *byte ^= 1;
        }
    }

    #[cfg(test)]
    pub(crate) fn wire_len_for_test(&self) -> usize {
        self.wire_len()
    }

    fn wire_len(&self) -> usize {
        self.sender_public_key.len() + self.nonce.len() + self.ciphertext.len()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RelayForwardReceipt {
    relay_bytes: RelayTrafficBytes,
    sync_control_bytes: SyncControlBytes,
}

impl RelayForwardReceipt {
    pub(crate) fn relay_bytes(self) -> RelayTrafficBytes {
        self.relay_bytes
    }

    pub(crate) fn sync_control_bytes(self) -> SyncControlBytes {
        self.sync_control_bytes
    }
}

#[derive(Debug, Default)]
pub(crate) struct DynamicRelay {
    forwarded_relay_bytes: RelayTrafficBytes,
    forwarded_sync_control_bytes: SyncControlBytes,
}

impl DynamicRelay {
    pub(crate) fn forward(
        &mut self,
        frame: &RelayFrame,
    ) -> Result<RelayForwardReceipt, RelayError> {
        if frame.wire_len() > MAX_RELAY_FRAME_BYTES {
            return Err(RelayError::FrameLimit);
        }
        let frame_bytes = u64::try_from(frame.wire_len()).map_err(|_| RelayError::FrameLimit)?;
        self.forwarded_relay_bytes =
            RelayTrafficBytes(self.forwarded_relay_bytes.0.saturating_add(frame_bytes));
        self.forwarded_sync_control_bytes = SyncControlBytes(
            self.forwarded_sync_control_bytes
                .0
                .saturating_add(frame_bytes),
        );
        Ok(RelayForwardReceipt {
            relay_bytes: RelayTrafficBytes(frame_bytes),
            sync_control_bytes: SyncControlBytes(frame_bytes),
        })
    }

    pub(crate) fn forwarded_relay_bytes(&self) -> RelayTrafficBytes {
        self.forwarded_relay_bytes
    }

    pub(crate) fn forwarded_sync_control_bytes(&self) -> SyncControlBytes {
        self.forwarded_sync_control_bytes
    }
}

fn relay_cipher(private_key: [u8; 32], peer_public_key: [u8; 32]) -> ChaCha20Poly1305 {
    let secret = StaticSecret::from(private_key);
    let peer = PublicKey::from(peer_public_key);
    let shared = secret.diffie_hellman(&peer);
    let mut hasher = Sha256::new();
    hasher.update(RELAY_KEY_CONTEXT);
    hasher.update(shared.as_bytes());
    let key: [u8; 32] = hasher.finalize().into();
    ChaCha20Poly1305::new(Key::from_slice(&key))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RelayError {
    FrameLimit,
    AuthenticationFailed,
}

impl std::fmt::Display for RelayError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "history sync relay error: {self:?}")
    }
}

impl std::error::Error for RelayError {}
