use anyhow::Result;
use clap::Parser;
use xp_proxy_core::ProxySnapshot;

#[derive(Debug, Parser)]
#[command(name = "xp-proxy")]
struct Args {
    #[arg(long)]
    validate_snapshot: Option<std::path::PathBuf>,
}

fn main() -> Result<()> {
    let args = Args::parse();
    if let Some(path) = args.validate_snapshot {
        let bytes = std::fs::read(path)?;
        let snapshot: ProxySnapshot = serde_json::from_slice(&bytes)?;
        snapshot.validate().map_err(anyhow::Error::msg)?;
        println!("snapshot_valid generation={}", snapshot.generation);
        return Ok(());
    }
    anyhow::bail!("xp-proxy runtime listener is not implemented yet")
}
