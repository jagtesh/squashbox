//! Squashbox CLI (`sqb`) — SquashFS native filesystem tools for Windows.
//!
//! This is the Windows entry point for the unified `sqb` CLI. Shared commands
//! (`image`) delegate to `squashbox_core::cli`; platform-specific commands
//! (`mount`, `umount`) are implemented here with ProjFS support.
//!
//! Usage:
//!   sqb image  <FILE>        Print image info
//!   sqb mount  <FILE> <DIR>  Mount a SquashFS image as a ProjFS projected filesystem
//!   sqb umount <DIR>         Clean up a stale ProjFS mount point

use clap::{Parser, Subcommand};
use squashbox_core::provider::VirtualFsProvider;
use std::path::PathBuf;
use std::sync::Arc;

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
    /// Mount a SquashFS image as a Windows projected filesystem
    Mount {
        /// Path to the SquashFS image file
        file: PathBuf,
        /// Directory to mount the image at (will be created if needed)
        dir: PathBuf,
        /// Remove stale ProjFS reparse points from DIR before mounting.
        ///
        /// A previous mount that wasn't cleanly stopped leaves a reparse point
        /// on the directory that blocks re-mounting. This flag clears it
        /// automatically before starting ProjFS.
        #[arg(long, short = 'f')]
        force: bool,
    },
    /// Clean up a stale ProjFS mount point
    ///
    /// Removes the ProjFS virtualization reparse point from a directory that
    /// was left behind by a previous mount that wasn't cleanly unmounted.
    /// This is equivalent to 'mount --force' without starting a new mount.
    Umount {
        /// Directory with a stale ProjFS reparse point to clean up
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
        Commands::Mount { file, dir, force } => cmd_mount(&file, &dir, force),
        Commands::Umount { dir } => cmd_umount(&dir),
    }
}

fn cmd_mount(image_path: &PathBuf, mount_point: &PathBuf, force: bool) -> anyhow::Result<()> {
    if mount_point.exists() && force {
        cmd_fix(mount_point)?;
    } else if !mount_point.exists() {
        std::fs::create_dir_all(mount_point)?;
    }

    // Shared validation — format-agnostic (SquashFS, ZIP, etc.)
    let (provider, _stats) = squashbox_core::cli::open_image(image_path)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    let provider: Arc<dyn VirtualFsProvider> = provider.into();
    let source = squashbox_windows::projfs_source::SquashboxProjFsSource::new(provider);

    log::info!("Starting ProjFS at: {}", mount_point.display());
    let _pfs = windows_projfs::ProjectedFileSystem::new(mount_point, source)
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("0x8007112B") || msg.contains("reparse point") {
                anyhow::anyhow!(
                    "Mount failed: '{}' has a stale ProjFS reparse point\n\
                     from a previous mount that was not cleanly unmounted.\n\n\
                     To clean it up:\n\
                     \n  sqb umount \"{}\"\n\n\
                     Or re-run with --force to clear it automatically:\n\
                     \n  sqb mount --force \"{}\" \"{}\"\n\n\
                     Original error: {}",
                    mount_point.display(),
                    mount_point.display(),
                    image_path.display(),
                    mount_point.display(),
                    e
                )
            } else {
                anyhow::anyhow!("Mount failed: {}", e)
            }
        })?;

    println!(
        "✓ Mounted {} at {}",
        image_path.display(),
        mount_point.display()
    );
    println!("Press Ctrl+C to unmount...");

    let (tx, rx) = std::sync::mpsc::channel();
    ctrlc::set_handler(move || {
        let _ = tx.send(());
    })?;
    rx.recv()?;

    print!("Unmounting... ");
    drop(_pfs);

    if let Err(e) = cmd_fix(mount_point) {
        println!("done (with warning)");
        eprintln!(
            "Warning: could not remove reparse point from '{}': {}\n\
             Use 'sqb mount --force' next time if the directory is stale.",
            mount_point.display(), e
        );
    } else {
        println!("done.");
    }

    Ok(())
}


/// Remove stale ProjFS virtualization reparse points from a directory.
///
/// ProjFS marks a directory as a "virtualization root" by attaching a reparse
/// point (`IO_REPARSE_TAG_PROJFS`, `FILE_ATTRIBUTE_REPARSE_POINT` 0x400) to it.
/// If the process exits without cleanly calling `PrjStopVirtualizing`, the
/// reparse point is left behind and prevents re-mounting.
///
/// We call `FSCTL_DELETE_REPARSE_POINT` via `DeviceIoControl` — the proper
/// Win32 API for removing reparse points at the NTFS level without deleting
/// the directory. This is the same API that `fsutil reparsepoint delete` uses.
///
/// Reference: https://learn.microsoft.com/en-us/windows/win32/api/winioctl/ni-winioctl-fsctl_delete_reparse_point
///
/// This function is **idempotent**: if the directory is already clean (no
/// reparse point), it is a no-op.
fn cmd_fix(dir: &PathBuf) -> anyhow::Result<()> {
    if !dir.exists() {
        std::fs::create_dir_all(dir)?;
        println!("✓ Created clean mount directory: {}", dir.display());
        return Ok(());
    }

    // Don't rely on FILE_ATTRIBUTE_REPARSE_POINT from metadata — ProjFS
    // directory reparse points (tag 0x9000001c) are not always visible
    // in the metadata attributes. Instead, try to query+delete directly.
    match delete_reparse_point(dir) {
        Ok(()) => {
            println!("✓ Reparse point removed: {}", dir.display());
        }
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("4390") {
                // ERROR_NOT_A_REPARSE_POINT (4390) — directory is already clean
                println!("✓ Directory is already clean: {}", dir.display());
            } else {
                eprintln!("Warning: FSCTL_DELETE_REPARSE_POINT failed: {}", e);
                eprintln!("Falling back to directory cleanup...");
                std::fs::remove_dir_all(dir)
                    .map_err(|e| anyhow::anyhow!(
                        "Could not remove '{}': {}\n\
                         Make sure no processes have the directory open and try again.",
                        dir.display(), e
                    ))?;
                std::fs::create_dir_all(dir)?;
                println!("✓ Directory cleaned and ready for mounting: {}", dir.display());
            }
        }
    }

    Ok(())
}

/// Call FSCTL_DELETE_REPARSE_POINT on a directory via the Win32 API.
fn delete_reparse_point(dir: &PathBuf) -> anyhow::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;

    const GENERIC_WRITE: u32 = 0x40000000;
    const OPEN_EXISTING: u32 = 3;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x02000000;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x00200000;
    const FSCTL_GET_REPARSE_POINT: u32 = 0x000900A8;
    const FSCTL_DELETE_REPARSE_POINT: u32 = 0x000900AC;
    const INVALID_HANDLE_VALUE: isize = -1;

    #[repr(C)]
    struct ReparseDataBuffer {
        reparse_tag: u32,
        reparse_data_length: u16,
        reserved: u16,
    }

    extern "system" {
        fn CreateFileW(
            lpFileName: *const u16,
            dwDesiredAccess: u32,
            dwShareMode: u32,
            lpSecurityAttributes: *const u8,
            dwCreationDisposition: u32,
            dwFlagsAndAttributes: u32,
            hTemplateFile: isize,
        ) -> isize;
        fn DeviceIoControl(
            hDevice: isize,
            dwIoControlCode: u32,
            lpInBuffer: *const u8,
            nInBufferSize: u32,
            lpOutBuffer: *mut u8,
            nOutBufferSize: u32,
            lpBytesReturned: *mut u32,
            lpOverlapped: *const u8,
        ) -> i32;
        fn CloseHandle(hObject: isize) -> i32;
        fn GetLastError() -> u32;
    }

    let wide_path: Vec<u16> = dir.as_os_str().encode_wide().chain(std::iter::once(0)).collect();

    let handle = unsafe {
        CreateFileW(
            wide_path.as_ptr(),
            GENERIC_WRITE,
            0,
            ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            0,
        )
    };

    if handle == INVALID_HANDLE_VALUE {
        let err = unsafe { GetLastError() };
        anyhow::bail!("CreateFileW failed on '{}': Win32 error {}", dir.display(), err);
    }

    let mut buf = [0u8; 1024];
    let mut bytes_returned: u32 = 0;
    let ok = unsafe {
        DeviceIoControl(
            handle,
            FSCTL_GET_REPARSE_POINT,
            ptr::null(),
            0,
            buf.as_mut_ptr(),
            buf.len() as u32,
            &mut bytes_returned,
            ptr::null(),
        )
    };

    if ok == 0 {
        let err = unsafe { GetLastError() };
        unsafe { CloseHandle(handle) };
        anyhow::bail!("FSCTL_GET_REPARSE_POINT failed: Win32 error {}", err);
    }

    let reparse_tag = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);

    let delete_buf = ReparseDataBuffer {
        reparse_tag,
        reparse_data_length: 0,
        reserved: 0,
    };

    let ok = unsafe {
        DeviceIoControl(
            handle,
            FSCTL_DELETE_REPARSE_POINT,
            &delete_buf as *const _ as *const u8,
            std::mem::size_of::<ReparseDataBuffer>() as u32,
            ptr::null_mut(),
            0,
            &mut bytes_returned,
            ptr::null(),
        )
    };

    unsafe { CloseHandle(handle) };

    if ok == 0 {
        let err = unsafe { GetLastError() };
        anyhow::bail!("FSCTL_DELETE_REPARSE_POINT failed: Win32 error {}", err);
    }

    Ok(())
}

fn cmd_umount(dir: &PathBuf) -> anyhow::Result<()> {
    if !dir.exists() {
        anyhow::bail!("Directory not found: {}", dir.display());
    }
    cmd_fix(dir)
}
