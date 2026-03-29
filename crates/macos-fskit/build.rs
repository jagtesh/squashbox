//! Build script for macos-fskit: links the FSKit system framework.

fn main() {
    // Only link FSKit on macOS targets.
    let target = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target == "macos" {
        println!("cargo:rustc-link-lib=framework=FSKit");
    }
}
