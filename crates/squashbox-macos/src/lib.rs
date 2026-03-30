//! squashbox-macos: macOS FSKit driver for Squashbox.
//!
//! This crate adapts `squashbox-core::VirtualFsProvider` into the
//! `macos-fskit::FsKitFileSystemSource` trait for native macOS
//! FSKit filesystem extension support.

pub mod ffi;
pub mod fskit_source;
