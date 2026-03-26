// Quick test: open Ubuntu image from the squashbox-windows compilation context
use squashbox_core::SquashFsProvider;
use std::path::PathBuf;

#[test]
fn open_ubuntu_from_windows_crate() {
    let home = std::env::var("USERPROFILE").unwrap();
    let path = PathBuf::from(home).join("Downloads").join("filesystem.squashfs");
    if !path.exists() { eprintln!("SKIP"); return; }
    eprintln!("Opening from squashbox-windows context: {}", path.display());
    let p = SquashFsProvider::open(&path).unwrap();
    let stats = squashbox_core::provider::VirtualFsProvider::volume_stats(&p).unwrap();
    eprintln!("OK: {} inodes", stats.total_inodes);
}
