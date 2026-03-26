//! squashbox-windows: Windows ProjFS driver for Squashbox.
//!
//! This crate adapts `squashbox-core::VirtualFsProvider` into the
//! `windows-projfs::ProjectedFileSystemSource` trait for native Windows
//! projected filesystem support.

pub mod projfs_source;
