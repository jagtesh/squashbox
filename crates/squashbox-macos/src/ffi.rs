//! C FFI exports for the FSKit App Extension.
//!
//! These `extern "C"` functions are the bridge between the Swift entry point
//! (which FSKit requires) and our Rust filesystem implementation. The Swift
//! code in the `.appex` calls these functions via a C header.
//!
//! # Lifecycle
//!
//! 1. Swift calls `squashbox_register_classes()` at extension startup
//! 2. FSKit instantiates `SquashboxUnaryFileSystem` (registered ObjC class)
//! 3. When FSKit calls `loadResource:`, the ObjC method (in runtime.rs)
//!    creates a `SquashboxFsKitSource` and attaches it to the instance
//! 4. All subsequent VFS callbacks flow through the ObjC → Rust bridge

use std::os::raw::c_char;
use std::sync::Arc;

use macos_fskit::FsKitFileSystemSource;

/// Concrete wrapper so we can round-trip through `*const c_void` without
/// losing the vtable (fat pointer → thin pointer problem).
struct SourceHandle(Arc<dyn FsKitFileSystemSource>);

/// Register the ObjC classes with the Objective-C runtime.
///
/// Must be called once before FSKit attempts to instantiate the filesystem.
/// This is idempotent — calling it multiple times is safe.
#[no_mangle]
pub extern "C" fn squashbox_register_classes() {
    // Initialize logging (best-effort, no-op if already initialized)
    let _ = env_logger::try_init();
    log::info!("squashbox_register_classes() called from Swift");
    macos_fskit::runtime::register_classes();
}

/// Create a new `SquashboxFsKitSource` and return an opaque pointer.
///
/// The caller receives a `Box<SourceHandle>` disguised as `*mut c_void`.
/// This pointer must be passed to `squashbox_source_destroy()` when no
/// longer needed.
///
/// Returns null on failure.
#[no_mangle]
pub extern "C" fn squashbox_source_create() -> *mut std::ffi::c_void {
    let source = crate::fskit_source::SquashboxFsKitSource::new();
    let arc: Arc<dyn FsKitFileSystemSource> = Arc::new(source);
    let handle = Box::new(SourceHandle(arc));
    Box::into_raw(handle) as *mut std::ffi::c_void
}

/// Destroy a source created by `squashbox_source_create()`.
///
/// # Safety
///
/// `ptr` must be a pointer previously returned by `squashbox_source_create()`,
/// and must not have been destroyed already.
#[no_mangle]
pub unsafe extern "C" fn squashbox_source_destroy(ptr: *mut std::ffi::c_void) {
    if !ptr.is_null() {
        let _ = unsafe { Box::from_raw(ptr as *mut SourceHandle) };
    }
}

/// Get the bundle identifier for the filesystem extension.
///
/// Returns a static string suitable for FSKit registration.
#[no_mangle]
pub extern "C" fn squashbox_bundle_identifier() -> *const c_char {
    // Must match CFBundleIdentifier in the appex's Info.plist
    b"com.squashbox.fs.squashfs\0".as_ptr() as *const c_char
}

/// Get the filesystem short name.
#[no_mangle]
pub extern "C" fn squashbox_fs_short_name() -> *const c_char {
    b"squashfs\0".as_ptr() as *const c_char
}
