//! Squashbox CLI (`sqb`) — SquashFS native filesystem tools for macOS.
//!
//! This is the macOS entry point for the unified `sqb` CLI. Shared commands
//! (`image`) delegate to `squashbox_core::cli`; platform-specific commands
//! (`mount`, `umount`, `install`) are implemented here.
//!
//! Usage:
//!   sqb image   <FILE>        Print image info
//!   sqb mount   <FILE> <DIR>  Mount a SquashFS image via FSKit
//!   sqb umount  <DIR>         Unmount an FSKit filesystem
//!   sqb install               Install the FSKit extension

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
    /// Install the Squashbox FSKit extension
    ///
    /// Copies Squashbox.app (with the embedded SquashboxFS.appex) to
    /// ~/Applications and opens System Settings for activation.
    Install {
        /// Path to the built Squashbox.app bundle.
        /// Defaults to build/macos/Squashbox.app relative to the project root.
        #[arg(long)]
        app_path: Option<PathBuf>,

        /// Skip opening System Settings after installation
        #[arg(long)]
        no_open: bool,
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
        Commands::Install { app_path, no_open } => cmd_install(app_path, no_open),
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
    // 1. The SquashboxFS.appex to be installed (sqb install)
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
    println!();
    println!("    Run 'sqb install' to install the extension.");

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

/// Install the Squashbox FSKit extension.
///
/// 1. Locates the built Squashbox.app bundle
/// 2. Copies it to ~/Applications/
/// 3. Opens System Settings for the user to enable the extension
fn cmd_install(app_path: Option<PathBuf>, no_open: bool) -> anyhow::Result<()> {
    // ── Locate the .app bundle ──
    let source = if let Some(path) = app_path {
        path
    } else {
        // Try to find it relative to the sqb binary's location
        // (the build script puts it in build/macos/Squashbox.app)
        let exe = std::env::current_exe()?;
        let project_root = exe
            .ancestors()
            .find(|p| p.join("Cargo.toml").exists())
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| exe.parent().unwrap().to_path_buf());

        let default_path = project_root.join("build/macos/Squashbox.app");
        if !default_path.exists() {
            anyhow::bail!(
                "Squashbox.app not found at: {}\n\n\
                 Build it first:\n  ./scripts/build-macos.sh\n\n\
                 Or specify the path:\n  sqb install --app-path /path/to/Squashbox.app",
                default_path.display()
            );
        }
        default_path
    };

    if !source.exists() || !source.is_dir() {
        anyhow::bail!("Not a valid app bundle: {}", source.display());
    }

    // Verify it contains the appex
    let appex_path = source.join("Contents/PlugIns/SquashboxFS.appex");
    if !appex_path.exists() {
        anyhow::bail!(
            "Invalid Squashbox.app: missing SquashboxFS.appex at {}",
            appex_path.display()
        );
    }

    // ── Install to ~/Applications/ ──
    let home = std::env::var("HOME")
        .map_err(|_| anyhow::anyhow!("$HOME not set"))?;
    let dest_dir = PathBuf::from(&home).join("Applications");
    let dest = dest_dir.join("Squashbox.app");

    std::fs::create_dir_all(&dest_dir)?;

    // Remove old installation if present
    if dest.exists() {
        println!("  Removing previous installation...");
        std::fs::remove_dir_all(&dest)?;
    }

    println!("  Installing to: {}", dest.display());

    // Use ditto for a proper macOS bundle copy (preserves resource forks, etc.)
    let output = std::process::Command::new("ditto")
        .args([
            source.to_str().unwrap(),
            dest.to_str().unwrap(),
        ])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("ditto failed: {}", stderr.trim());
    }

    println!("  ✓ Installed Squashbox.app to ~/Applications/");
    println!();

    // ── Open System Settings ──
    if !no_open {
        println!("  Opening System Settings → File System Extensions...");
        println!();
        let _ = std::process::Command::new("open")
            .arg("x-apple.systempreferences:com.apple.LoginItems-Settings.extension")
            .spawn();
    }

    println!("╔══════════════════════════════════════════════════╗");
    println!("║           Installation Complete ✓                ║");
    println!("╚══════════════════════════════════════════════════╝");
    println!();
    println!("  Next steps:");
    println!("  1. Enable 'SquashboxFS' in System Settings →");
    println!("     General → Login Items → File System Extensions");
    println!("  2. Mount an image: sqb mount image.sqsh /Volumes/MyImage");
    println!();

    Ok(())
}
