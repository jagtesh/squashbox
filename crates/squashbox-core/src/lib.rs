//! squashbox-core: Platform-agnostic virtual filesystem provider.
//!
//! This crate provides the core abstraction layer for Squashbox. It defines
//! the `VirtualFsProvider` trait and format-specific implementations:
//! - `SquashFsProvider` — backed by the `backhand` crate
//! - `ZipFsProvider` — backed by the `zip` crate
//!
//! Both the Windows (ProjFS) and macOS (FSKit/UniFFI) drivers call into this crate.

pub mod cli;
pub mod fmt;
pub mod nfs;
pub mod provider;
pub mod squashfs;
pub mod types;
pub mod zip;

// Re-export key types at crate root for convenience.
pub use provider::VirtualFsProvider;
pub use squashfs::SquashFsProvider;
pub use types::*;
pub use zip::ZipFsProvider;
