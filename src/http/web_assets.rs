mod embedded {
    include!(concat!(env!("OUT_DIR"), "/embedded_web_assets.rs"));
}

pub use embedded::EmbeddedAsset;

pub fn get(path: &str) -> Option<EmbeddedAsset> {
    embedded::get(path)
}
