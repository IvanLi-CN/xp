//! Protocol and control-plane primitives for the native XP data plane.

pub mod control;
pub mod fallback;
pub mod metrics;
pub mod reality;
pub mod tls_hello;
pub mod transport;
pub mod vless;
pub mod xhttp;

pub use control::{
    ControlOperation, ControlRequest, ControlResponse, GenerationManager, GenerationState,
    ProxySnapshot,
};
pub use metrics::{TrafficCounter, TrafficSnapshot};
pub use transport::{RealityConfig, XhttpConfig, XmuxSettings};
pub use vless::{VlessRequest, VlessRequestError};
