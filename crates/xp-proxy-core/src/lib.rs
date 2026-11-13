//! Protocol and control-plane primitives for the native XP data plane.

pub mod control;
pub mod vless;

pub use control::{
    ControlOperation, ControlRequest, ControlResponse, GenerationManager, GenerationState,
    ProxySnapshot,
};
pub use vless::{VlessRequest, VlessRequestError};
