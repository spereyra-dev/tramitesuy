//! TrámitesUY API library: router, handlers, DTOs, and error mapping.
//! Slice (c), unit C1 — the read surface (tasks 70-77).

pub mod config;
pub mod dto;
pub mod error;
pub mod handlers;
pub mod metrics;
pub mod redaction;
pub mod router;
pub mod state;

pub use router::build_router;
