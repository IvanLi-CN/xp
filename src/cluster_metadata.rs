use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::{
    cluster_identity::{
        JoinToken, generate_cluster_ca, generate_node_keypair_and_csr, sign_node_csr,
    },
    id::new_ulid_string,
};

pub const CLUSTER_METADATA_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClusterPaths {
    pub dir: PathBuf,
    pub metadata_json: PathBuf,
    pub cluster_ca_pem: PathBuf,
    pub cluster_ca_key_pem: PathBuf,
    pub node_csr_pem: PathBuf,
    pub node_cert_pem: PathBuf,
    pub node_key_pem: PathBuf,
}

impl ClusterPaths {
    pub fn new(data_dir: &Path) -> Self {
        let dir = data_dir.join("cluster");
        Self {
            metadata_json: dir.join("metadata.json"),
            cluster_ca_pem: dir.join("cluster_ca.pem"),
            cluster_ca_key_pem: dir.join("cluster_ca_key.pem"),
            node_csr_pem: dir.join("node_csr.pem"),
            node_cert_pem: dir.join("node_cert.pem"),
            node_key_pem: dir.join("node_key.pem"),
            dir,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClusterMetadata {
    pub schema_version: u32,
    pub cluster_id: String,
    pub node_id: String,
    pub node_name: String,
    pub public_domain: String,
    pub api_base_url: String,
    /// `true` if this node has the cluster CA private key (bootstrap leader).
    pub has_cluster_ca_key: bool,
}

impl ClusterMetadata {
    pub fn init_new_cluster(
        data_dir: &Path,
        node_name: String,
        public_domain: String,
        api_base_url: String,
    ) -> anyhow::Result<Self> {
        let paths = ClusterPaths::new(data_dir);
        if paths.metadata_json.exists() {
            anyhow::bail!(
                "cluster metadata already exists at {}",
                paths.metadata_json.display()
            );
        }

        fs::create_dir_all(&paths.dir)
            .with_context(|| format!("create cluster dir: {}", paths.dir.display()))?;

        let cluster_id = new_ulid_string();
        let node_id = new_ulid_string();

        let ca = generate_cluster_ca(&cluster_id)?;
        write_atomic(&paths.cluster_ca_pem, ca.cert_pem.as_bytes())
            .with_context(|| format!("write {}", paths.cluster_ca_pem.display()))?;
        write_atomic(&paths.cluster_ca_key_pem, ca.key_pem.as_bytes())
            .with_context(|| format!("write {}", paths.cluster_ca_key_pem.display()))?;

        let csr = generate_node_keypair_and_csr(&node_id)?;
        let signed = sign_node_csr(&cluster_id, &ca.key_pem, &csr.csr_pem)?;

        write_atomic(&paths.node_key_pem, csr.key_pem.as_bytes())
            .with_context(|| format!("write {}", paths.node_key_pem.display()))?;
        write_atomic(&paths.node_csr_pem, csr.csr_pem.as_bytes())
            .with_context(|| format!("write {}", paths.node_csr_pem.display()))?;
        write_atomic(&paths.node_cert_pem, signed.as_bytes())
            .with_context(|| format!("write {}", paths.node_cert_pem.display()))?;

        let meta = Self {
            schema_version: CLUSTER_METADATA_SCHEMA_VERSION,
            cluster_id,
            node_id,
            node_name,
            public_domain,
            api_base_url,
            has_cluster_ca_key: true,
        };

        meta.save(data_dir)?;
        Ok(meta)
    }

    pub fn save(&self, data_dir: &Path) -> anyhow::Result<()> {
        let paths = ClusterPaths::new(data_dir);
        fs::create_dir_all(&paths.dir)
            .with_context(|| format!("create cluster dir: {}", paths.dir.display()))?;
        let bytes = serde_json::to_vec_pretty(self).context("serialize cluster metadata")?;
        write_atomic(&paths.metadata_json, &bytes)
            .with_context(|| format!("write {}", paths.metadata_json.display()))?;
        Ok(())
    }

    pub fn load(data_dir: &Path) -> anyhow::Result<Self> {
        let paths = ClusterPaths::new(data_dir);
        let bytes = fs::read(&paths.metadata_json)
            .with_context(|| format!("read {}", paths.metadata_json.display()))?;
        let meta: Self = serde_json::from_slice(&bytes).context("parse cluster metadata")?;
        if meta.schema_version != CLUSTER_METADATA_SCHEMA_VERSION {
            anyhow::bail!(
                "cluster metadata schema_version mismatch: expected {}, got {}",
                CLUSTER_METADATA_SCHEMA_VERSION,
                meta.schema_version
            );
        }
        Ok(meta)
    }

    pub fn read_cluster_ca_pem(&self, data_dir: &Path) -> anyhow::Result<String> {
        let paths = ClusterPaths::new(data_dir);
        let bytes = fs::read(&paths.cluster_ca_pem)
            .with_context(|| format!("read {}", paths.cluster_ca_pem.display()))?;
        String::from_utf8(bytes).context("cluster_ca.pem is not valid utf-8")
    }

    pub fn read_cluster_ca_key_pem(&self, data_dir: &Path) -> anyhow::Result<Option<String>> {
        if !self.has_cluster_ca_key {
            return Ok(None);
        }
        let paths = ClusterPaths::new(data_dir);
        let bytes = fs::read(&paths.cluster_ca_key_pem)
            .with_context(|| format!("read {}", paths.cluster_ca_key_pem.display()))?;
        Ok(Some(
            String::from_utf8(bytes).context("cluster_ca_key.pem is not valid utf-8")?,
        ))
    }

    pub fn read_node_key_pem(&self, data_dir: &Path) -> anyhow::Result<String> {
        let paths = ClusterPaths::new(data_dir);
        let bytes = fs::read(&paths.node_key_pem)
            .with_context(|| format!("read {}", paths.node_key_pem.display()))?;
        String::from_utf8(bytes).context("node_key.pem is not valid utf-8")
    }

    pub fn read_node_cert_pem(&self, data_dir: &Path) -> anyhow::Result<String> {
        let paths = ClusterPaths::new(data_dir);
        let bytes = fs::read(&paths.node_cert_pem)
            .with_context(|| format!("read {}", paths.node_cert_pem.display()))?;
        String::from_utf8(bytes).context("node_cert.pem is not valid utf-8")
    }

    pub fn expected_join_node_id(join_token: &str) -> anyhow::Result<String> {
        let parsed = JoinToken::decode_base64url_json(join_token)
            .map_err(|e| anyhow::anyhow!("decode join token: {e}"))?;
        Ok(parsed.token_id)
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp_path = path.with_extension("tmp");
    {
        let mut f = fs::File::create(&tmp_path)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(tmp_path, path)?;
    Ok(())
}
