// squashbox_ffi.h — C header for the Squashbox Rust static library.
//
// This header is imported by the Swift FSKit extension via a module map.
// The functions are implemented in squashbox-macos/src/ffi.rs.

#ifndef SQUASHBOX_FFI_H
#define SQUASHBOX_FFI_H

#include <stdint.h>

/// Register the ObjC classes (SquashboxVolume, SquashboxUnaryFileSystem)
/// with the Objective-C runtime. Call once at extension startup.
void squashbox_register_classes(void);

/// Create a new filesystem source. Returns an opaque handle.
/// Must be freed with squashbox_source_destroy().
void *squashbox_source_create(void);

/// Destroy a filesystem source created by squashbox_source_create().
void squashbox_source_destroy(void *handle);

/// Get the bundle identifier (static string, do not free).
const char *squashbox_bundle_identifier(void);

/// Get the filesystem short name (static string, do not free).
const char *squashbox_fs_short_name(void);

#endif // SQUASHBOX_FFI_H
