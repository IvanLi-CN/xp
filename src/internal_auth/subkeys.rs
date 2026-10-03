use std::sync::{Mutex, OnceLock};

use hmac::Mac;
use openssl::{pkey::PKey, x509::X509};
use sha2::{Digest, Sha256};

use super::{AuthError, HmacSha256};

// Store only the input digest and successfully derived subkey. Fixed slots bound both memory
// and lookup work; FIFO replacement also bounds retained keys after authority changes.
const CACHE_CAPACITY: usize = 8;
type Entry = ([u8; 32], [u8; 32]);

#[derive(Default)]
struct SubkeyCache {
    entries: [Option<Entry>; CACHE_CAPACITY],
    next: usize,
}

impl SubkeyCache {
    fn get(&self, identity: &[u8; 32]) -> Option<[u8; 32]> {
        self.entries
            .iter()
            .flatten()
            .find_map(|(digest, key)| (digest == identity).then_some(*key))
    }

    fn insert(&mut self, identity: [u8; 32], key: [u8; 32]) {
        // Another caller may have filled this miss while derivation ran outside the lock.
        if self.get(&identity).is_none() {
            self.entries[self.next] = Some((identity, key));
            self.next = (self.next + 1) % CACHE_CAPACITY;
        }
    }
}

static SUBKEYS: OnceLock<Mutex<SubkeyCache>> = OnceLock::new();

pub(super) fn derive_subkey(
    cluster_ca_key_pem: &str,
    cluster_ca_cert_pem: &str,
    info: &[u8],
) -> Result<[u8; 32], AuthError> {
    let mut digest = Sha256::new();
    for input in [
        cluster_ca_key_pem.as_bytes(),
        cluster_ca_cert_pem.as_bytes(),
        info,
    ] {
        digest.update((input.len() as u64).to_be_bytes());
        digest.update(input);
    }
    let identity: [u8; 32] = digest.finalize().into();
    let cache = SUBKEYS.get_or_init(|| Mutex::new(SubkeyCache::default()));
    match cache.lock() {
        Ok(entries) => {
            if let Some(key) = entries.get(&identity) {
                return Ok(key);
            }
        }
        Err(_) => return derive_uncached(cluster_ca_key_pem, cluster_ca_cert_pem, info),
    }

    // Parsing/HKDF may allocate or fail. Neither runs under the cache lock, and errors never
    // become entries. A poisoned insertion lock leaves this successful uncached result usable.
    let key = derive_uncached(cluster_ca_key_pem, cluster_ca_cert_pem, info)?;
    if let Ok(mut entries) = cache.lock() {
        entries.insert(identity, key);
    }
    Ok(key)
}

fn derive_uncached(
    cluster_ca_key_pem: &str,
    cluster_ca_cert_pem: &str,
    info: &[u8],
) -> Result<[u8; 32], AuthError> {
    let key = PKey::private_key_from_pem(cluster_ca_key_pem.as_bytes())
        .map_err(|e| AuthError::Crypto(format!("parse CA private key: {e}")))?;
    let key_der = key
        .private_key_to_der()
        .map_err(|e| AuthError::Crypto(format!("encode CA private key: {e}")))?;
    let cert = X509::from_pem(cluster_ca_cert_pem.as_bytes())
        .map_err(|e| AuthError::Crypto(format!("parse CA certificate: {e}")))?;
    let cert_der = cert
        .to_der()
        .map_err(|e| AuthError::Crypto(format!("encode CA certificate: {e}")))?;
    let salt = Sha256::digest(cert_der);

    // HKDF-Extract(salt, IKM), then HKDF-Expand(PRK, info || 0x01).
    let mut extract =
        HmacSha256::new_from_slice(&salt).map_err(|e| AuthError::Crypto(e.to_string()))?;
    extract.update(&key_der);
    let prk = extract.finalize().into_bytes();
    let mut expand =
        HmacSha256::new_from_slice(&prk).map_err(|e| AuthError::Crypto(e.to_string()))?;
    expand.update(info);
    expand.update(&[1]);
    Ok(expand.finalize().into_bytes().into())
}
