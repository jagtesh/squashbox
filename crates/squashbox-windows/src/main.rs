//! Squashbox CLI entry point for Windows.
//!
//! Mounts a SquashFS image as a ProjFS projected filesystem.

use squashbox_core::SquashFsProvider;
use squashbox_windows::projfs_source::SquashboxProjFsSource;
use std::path::PathBuf;
use std::sync::Arc;

fn main() -> anyhow::Result<()> {
    env_logger::init();

    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("Usage: squashbox <image.sqsh> <mount_point>");
        eprintln!("  Mount a SquashFS image as a Windows projected filesystem.");
        std::process::exit(1);
    }

    let image_path = PathBuf::from(&args[1]);
    let mount_point = PathBuf::from(&args[2]);

    // Validate image exists
    if !image_path.exists() {
        anyhow::bail!("Image file not found: {}", image_path.display());
    }

    // Create mount point directory if it doesn't exist
    if !mount_point.exists() {
        std::fs::create_dir_all(&mount_point)?;
    }

    // Open SquashFS image
    log::info!("Opening SquashFS image: {}", image_path.display());
    let provider = Arc::new(SquashFsProvider::open(&image_path)?);

    let stats = provider.volume_stats()?;
    log::info!(
        "Image opened: {} inodes, {} bytes",
        stats.total_inodes,
        stats.total_bytes
    );

    // Create ProjFS source adapter
    let source = SquashboxProjFsSource::new(provider);

    // Start projected file system
    log::info!("Starting ProjFS at: {}", mount_point.display());
    let pfs = windows_projfs::ProjectedFileSystem::new(&mount_point, source)?;
    pfs.start()?;

    println!(
        "✓ Mounted {} at {}",
        image_path.display(),
        mount_point.display()
    );
    println!("Press Ctrl+C to unmount...");

    // Block until Ctrl+C
    let (tx, rx) = std::sync::mpsc::channel();
    ctrlc::set_handler(move || {
        let _ = tx.send(());
    })?;
    rx.recv()?;

    println!("Unmounting...");
    // pfs is dropped here, which calls PrjStopVirtualizing
    Ok(())
}
