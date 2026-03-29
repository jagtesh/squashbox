//! squashbox-core: Platform-agnostic SquashFS virtual filesystem provider.
//!
//! This crate provides the core abstraction layer for Squashbox. It defines
//! the `VirtualFsProvider` trait and a backhand-backed `SquashFsProvider`
//! implementation. Both the Windows (ProjFS) and macOS (FSKit/UniFFI) drivers
//! call into this crate.

pub mod cli;
pub mod fmt;
pub mod provider;
pub mod squashfs;
pub mod types;

// Re-export key types at crate root for convenience.
pub use provider::VirtualFsProvider;
pub use squashfs::SquashFsProvider;
pub use types::*;
