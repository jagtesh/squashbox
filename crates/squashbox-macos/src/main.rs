//! Squashbox CLI (`sqb`) — SquashFS native filesystem tools for macOS.
//!
//! This is the macOS entry point for the unified `sqb` CLI. Shared commands
//! (`image`) delegate to `squashbox_core::cli`; platform-specific commands
//! (`mount`, `umount`) are implemented here.
//!
//! Usage:
//!   sqb image  <FILE>        Print image info
//!   sqb mount  <FILE> <DIR>  Mount a SquashFS image via FSKit
//!   sqb umount <DIR>         Unmount an FSKit filesystem

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "sqb", about = "Squashbox — native SquashFS tools", version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Print detailed information about a SquashFS image
    Image {
        /// Path to the SquashFS image file
        file: PathBuf,
    },
    /// Mount a SquashFS image via FSKit
    Mount {
        /// Path to the SquashFS image file
        file: PathBuf,
        /// Mount point directory
        dir: PathBuf,
    },
    /// Unmount an FSKit filesystem
    Umount {
        /// Mount point to unmount
        dir: PathBuf,
    },
}

fn main() -> anyhow::Result<()> {
    env_logger::init();

    let cli = Cli::parse();
    match cli.command {
        // Shared command — delegates entirely to squashbox-core
        Commands::Image { file } => {
            squashbox_core::cli::cmd_image(&file)
                .map_err(|e| anyhow::anyhow!("{}", e))
        }
        // Platform-specific commands
        Commands::Mount { file, dir } => cmd_mount(&file, &dir),
        Commands::Umount { dir } => cmd_umount(&dir),
    }
}

/// Mount a SquashFS image via FSKit.
///
/// Uses the shared `validate_image()` for the common open/validate step,
/// then proceeds with macOS-specific FSKit activation.
fn cmd_mount(image_path: &PathBuf, mount_point: &PathBuf) -> anyhow::Result<()> {
    let (_provider, stats) = squashbox_core::cli::validate_image(image_path)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    // TODO: Activate FSKit extension with the image path.
    // This requires:
    // 1. The SquashboxFS.appex to be installed
    // 2. Using FSClient or diskutil to trigger the mount
    //
    // For now, print instructions:
    println!("SquashFS image validated successfully.");
    println!();
    println!("  Image:   {}", image_path.display());
    println!("  Mount:   {}", mount_point.display());
    println!("  Inodes:  {}", stats.total_inodes);
    println!("  Size:    {} bytes", stats.total_bytes);
    println!();
    println!("⚠️  FSKit mount not yet implemented.");
    println!("    The FSKit app extension (SquashboxFS.appex) needs to be");
    println!("    built and installed before mounting is available.");

    Ok(())
}

/// Unmount a mounted SquashFS filesystem.
fn cmd_umount(mount_point: &PathBuf) -> anyhow::Result<()> {
    if !mount_point.exists() {
        anyhow::bail!("Mount point not found: {}", mount_point.display());
    }

    log::info!("Unmounting: {}", mount_point.display());

    let output = std::process::Command::new("diskutil")
        .args(["unmount", &mount_point.to_string_lossy()])
        .output()?;

    if output.status.success() {
        println!("Unmounted: {}", mount_point.display());
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to unmount: {}", stderr.trim());
    }
}
