//! Protocol and control-plane primitives for the native XP data plane.

pub mod control;
pub mod fallback;
pub mod transport;
pub mod vless;

pub use control::{
    ControlOperation, ControlRequest, ControlResponse, GenerationManager, GenerationState,
    ProxySnapshot,
};
pub use transport::{RealityConfig, XhttpConfig, XmuxSettings};
pub use vless::{VlessRequest, VlessRequestError};
