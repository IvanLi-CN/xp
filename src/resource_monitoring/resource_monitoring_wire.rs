use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as DeError};

use super::*;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum CompactResourceHistoryPayload {
    Rollup {
        resolution: String,
        #[serde(rename = "r")]
        rollup: CompactResourceRollup,
    },
    CaptureGap {
        resolution: String,
        #[serde(rename = "g")]
        gap: ResourceGap,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct CompactResourceRollup {
    #[serde(rename = "n")]
    node_id: String,
    #[serde(rename = "b")]
    bucket_start_unix_seconds: i64,
    #[serde(rename = "e")]
    expected_samples: u32,
    #[serde(rename = "a")]
    captured_samples: u32,
    #[serde(rename = "q")]
    capability: Capability,
    #[serde(rename = "v")]
    values: Vec<CompactRollupValue>,
    #[serde(rename = "u", default, skip_serializing_if = "Vec::is_empty")]
    unavailable: Vec<[String; 2]>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CompactRollupValue {
    #[serde(rename = "k")]
    key: String,
    #[serde(rename = "v")]
    aggregates: [Option<f64>; 5],
    #[serde(rename = "q")]
    capability: Capability,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum LegacyResourceHistoryPayload {
    Rollup {
        resolution: String,
        rollup: ResourceRollup,
    },
    CaptureGap {
        resolution: String,
        gap: ResourceGap,
    },
}

impl Serialize for ResourceHistoryPayload {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let compact = match self {
            ResourceHistoryPayload::Rollup { resolution, rollup } => {
                CompactResourceHistoryPayload::Rollup {
                    resolution: resolution.clone(),
                    rollup: compact_rollup(rollup).map_err(serde::ser::Error::custom)?,
                }
            }
            ResourceHistoryPayload::CaptureGap { resolution, gap } => {
                CompactResourceHistoryPayload::CaptureGap {
                    resolution: resolution.clone(),
                    gap: gap.clone(),
                }
            }
        };
        compact.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ResourceHistoryPayload {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        let is_compact =
            value.get("r").and_then(|rollup| rollup.get("v")).is_some() || value.get("g").is_some();
        if is_compact {
            let compact: CompactResourceHistoryPayload =
                serde_json::from_value(value).map_err(D::Error::custom)?;
            return expand_compact(compact).map_err(D::Error::custom);
        }
        let legacy: LegacyResourceHistoryPayload =
            serde_json::from_value(value).map_err(D::Error::custom)?;
        Ok(match legacy {
            LegacyResourceHistoryPayload::Rollup { resolution, rollup } => {
                ResourceHistoryPayload::Rollup { resolution, rollup }
            }
            LegacyResourceHistoryPayload::CaptureGap { resolution, gap } => {
                ResourceHistoryPayload::CaptureGap { resolution, gap }
            }
        })
    }
}

fn compact_rollup(rollup: &ResourceRollup) -> Result<CompactResourceRollup, &'static str> {
    let mut values = Vec::new();
    let mut unavailable = Vec::new();
    for (key, value) in &rollup.values {
        let key = compact_metric_key(key)
            .ok_or("resource_metric_not_fixed")?
            .to_string();
        let aggregates = [
            value.min,
            value.mean,
            value.max,
            value.last,
            value.counter_delta,
        ];
        if aggregates.iter().all(Option::is_none) {
            unavailable.push([key, capability_code(value.capability).to_string()]);
        } else {
            values.push(CompactRollupValue {
                key,
                aggregates,
                capability: value.capability,
            });
        }
    }
    Ok(CompactResourceRollup {
        node_id: rollup.node_id.clone(),
        bucket_start_unix_seconds: rollup.bucket_start_unix_seconds,
        expected_samples: rollup.expected_samples,
        captured_samples: rollup.captured_samples,
        capability: rollup.capability,
        values,
        unavailable,
    })
}

fn expand_compact(
    payload: CompactResourceHistoryPayload,
) -> Result<ResourceHistoryPayload, &'static str> {
    Ok(match payload {
        CompactResourceHistoryPayload::Rollup { resolution, rollup } => {
            let mut values = rollup
                .values
                .into_iter()
                .map(|value| {
                    Ok((
                        expand_metric_key(&value.key)
                            .ok_or("resource_metric_not_fixed")?
                            .to_string(),
                        RollupValue {
                            min: value.aggregates[0],
                            mean: value.aggregates[1],
                            max: value.aggregates[2],
                            last: value.aggregates[3],
                            counter_delta: value.aggregates[4],
                            capability: value.capability,
                        },
                    ))
                })
                .collect::<Result<BTreeMap<_, _>, &'static str>>()?;
            for [key, capability] in rollup.unavailable {
                values.insert(
                    expand_metric_key(&key)
                        .ok_or("resource_metric_not_fixed")?
                        .to_string(),
                    RollupValue {
                        min: None,
                        mean: None,
                        max: None,
                        last: None,
                        counter_delta: None,
                        capability: parse_capability_code(&capability)
                            .ok_or("resource_capability_invalid")?,
                    },
                );
            }
            ResourceHistoryPayload::Rollup {
                resolution,
                rollup: ResourceRollup {
                    node_id: rollup.node_id,
                    bucket_start_unix_seconds: rollup.bucket_start_unix_seconds,
                    expected_samples: rollup.expected_samples,
                    captured_samples: rollup.captured_samples,
                    capability: rollup.capability,
                    values,
                },
            }
        }
        CompactResourceHistoryPayload::CaptureGap { resolution, gap } => {
            ResourceHistoryPayload::CaptureGap { resolution, gap }
        }
    })
}

fn capability_code(capability: Capability) -> &'static str {
    match capability {
        Capability::Supported => "s",
        Capability::Partial => "p",
        Capability::Unsupported => "u",
    }
}

fn parse_capability_code(code: &str) -> Option<Capability> {
    match code {
        "s" => Some(Capability::Supported),
        "p" => Some(Capability::Partial),
        "u" => Some(Capability::Unsupported),
        _ => None,
    }
}

fn compact_metric_key(key: &str) -> Option<&'static str> {
    match key {
        "domain.cpu_busy_percent" => Some("dc"),
        "domain.cpu_iowait_percent" => Some("di"),
        "domain.load1" => Some("dl"),
        "domain.memory_available_bytes" => Some("dma"),
        "domain.memory_total_bytes" => Some("dmt"),
        "domain.swap_total_bytes" => Some("dst"),
        "domain.swap_free_bytes" => Some("dsf"),
        "domain.filesystem.root.used_percent" => Some("dru"),
        "domain.filesystem.root.used_inode_percent" => Some("dri"),
        "domain.filesystem.data.used_percent" => Some("ddu"),
        "domain.filesystem.data.used_inode_percent" => Some("ddi"),
        _ => compact_runtime_metric_key(key),
    }
}

fn compact_runtime_metric_key(key: &str) -> Option<&'static str> {
    let (role, metric) = key.split_once('.')?;
    let role_code = match role {
        "xp" => "x",
        "xray" => "r",
        "cloudflared" => "f",
        "canary" => "c",
        _ => return None,
    };
    let metric_code = match metric {
        "cpu_percent" => "c",
        "rss_bytes" => "r",
        "pss_bytes" => "p",
        "read_bytes_per_second" => "i",
        "write_bytes_per_second" => "o",
        "fd_count" => "f",
        "thread_count" => "t",
        _ => return None,
    };
    match (role_code, metric_code) {
        ("x", "c") => Some("xc"),
        ("x", "r") => Some("xr"),
        ("x", "p") => Some("xp"),
        ("x", "i") => Some("xi"),
        ("x", "o") => Some("xo"),
        ("x", "f") => Some("xf"),
        ("x", "t") => Some("xt"),
        ("r", "c") => Some("rc"),
        ("r", "r") => Some("rr"),
        ("r", "p") => Some("rp"),
        ("r", "i") => Some("ri"),
        ("r", "o") => Some("ro"),
        ("r", "f") => Some("rf"),
        ("r", "t") => Some("rt"),
        ("f", "c") => Some("fc"),
        ("f", "r") => Some("fr"),
        ("f", "p") => Some("fp"),
        ("f", "i") => Some("fi"),
        ("f", "o") => Some("fo"),
        ("f", "f") => Some("ff"),
        ("f", "t") => Some("ft"),
        ("c", "c") => Some("cc"),
        ("c", "r") => Some("cr"),
        ("c", "p") => Some("cp"),
        ("c", "i") => Some("ci"),
        ("c", "o") => Some("co"),
        ("c", "f") => Some("cf"),
        ("c", "t") => Some("ct"),
        _ => None,
    }
}

fn expand_metric_key(key: &str) -> Option<&'static str> {
    match key {
        "dc" => Some("domain.cpu_busy_percent"),
        "di" => Some("domain.cpu_iowait_percent"),
        "dl" => Some("domain.load1"),
        "dma" => Some("domain.memory_available_bytes"),
        "dmt" => Some("domain.memory_total_bytes"),
        "dst" => Some("domain.swap_total_bytes"),
        "dsf" => Some("domain.swap_free_bytes"),
        "dru" => Some("domain.filesystem.root.used_percent"),
        "dri" => Some("domain.filesystem.root.used_inode_percent"),
        "ddu" => Some("domain.filesystem.data.used_percent"),
        "ddi" => Some("domain.filesystem.data.used_inode_percent"),
        "xc" => Some("xp.cpu_percent"),
        "xr" => Some("xp.rss_bytes"),
        "xp" => Some("xp.pss_bytes"),
        "xi" => Some("xp.read_bytes_per_second"),
        "xo" => Some("xp.write_bytes_per_second"),
        "xf" => Some("xp.fd_count"),
        "xt" => Some("xp.thread_count"),
        "rc" => Some("xray.cpu_percent"),
        "rr" => Some("xray.rss_bytes"),
        "rp" => Some("xray.pss_bytes"),
        "ri" => Some("xray.read_bytes_per_second"),
        "ro" => Some("xray.write_bytes_per_second"),
        "rf" => Some("xray.fd_count"),
        "rt" => Some("xray.thread_count"),
        "fc" => Some("cloudflared.cpu_percent"),
        "fr" => Some("cloudflared.rss_bytes"),
        "fp" => Some("cloudflared.pss_bytes"),
        "fi" => Some("cloudflared.read_bytes_per_second"),
        "fo" => Some("cloudflared.write_bytes_per_second"),
        "ff" => Some("cloudflared.fd_count"),
        "ft" => Some("cloudflared.thread_count"),
        "cc" => Some("canary.cpu_percent"),
        "cr" => Some("canary.rss_bytes"),
        "cp" => Some("canary.pss_bytes"),
        "ci" => Some("canary.read_bytes_per_second"),
        "co" => Some("canary.write_bytes_per_second"),
        "cf" => Some("canary.fd_count"),
        "ct" => Some("canary.thread_count"),
        _ => None,
    }
}
