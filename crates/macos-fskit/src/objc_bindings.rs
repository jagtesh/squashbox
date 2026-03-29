//! ObjC bindings for FSKit framework classes.
//!
//! Uses `objc2`'s `extern_class!` and `extern_methods!` macros to declare
//! Rust types corresponding to FSKit's Objective-C classes and protocols.

use objc2::rc::Retained;
use objc2::runtime::NSObject;
use objc2::{extern_class, extern_methods, msg_send, AnyThread};
use objc2_foundation::{NSError, NSString};

// ═══════════════════════════════════════════════════════════════════
// FSResource
// ═══════════════════════════════════════════════════════════════════

extern_class!(
    /// A resource used by FSKit (e.g., a block device or URL).
    #[unsafe(super(NSObject))]
    #[name = "FSResource"]
    pub struct FSResource;
);

// ═══════════════════════════════════════════════════════════════════
// FSFileName
// ═══════════════════════════════════════════════════════════════════

extern_class!(
    /// A filename object used by FSKit for directory entries.
    #[unsafe(super(NSObject))]
    #[name = "FSFileName"]
    pub struct FSFileName;
);

extern_methods!(
    unsafe impl FSFileName {
        #[unsafe(method(nameWithString:))]
        pub fn name_with_string(string: &NSString) -> Retained<Self>;
    }
);

impl FSFileName {
    pub fn from_str(s: &str) -> Retained<Self> {
        let nsstring = NSString::from_str(s);
        Self::name_with_string(&nsstring)
    }

    pub fn to_string_lossy(&self) -> String {
        let nsstring: Retained<NSString> = unsafe { msg_send![self, string] };
        nsstring.to_string()
    }
}

// ═══════════════════════════════════════════════════════════════════
// FSItem
// ═══════════════════════════════════════════════════════════════════

extern_class!(
    #[unsafe(super(NSObject))]
    #[name = "FSItem"]
    pub struct FSItem;
);

extern_methods!(
    unsafe impl FSItem {
        #[unsafe(method(new))]
        pub fn new() -> Retained<Self>;
    }
);

// ═══════════════════════════════════════════════════════════════════
// FSItemAttributes
// ═══════════════════════════════════════════════════════════════════

extern_class!(
    #[unsafe(super(NSObject))]
    #[name = "FSItemAttributes"]
    pub struct FSItemAttributes;
);

extern_methods!(
    unsafe impl FSItemAttributes {
        #[unsafe(method(new))]
        pub fn new() -> Retained<Self>;

        #[unsafe(method(setUid:))]
        pub fn set_uid(&self, uid: u32);

        #[unsafe(method(setGid:))]
        pub fn set_gid(&self, gid: u32);

        #[unsafe(method(setMode:))]
        pub fn set_mode(&self, mode: u32);

        #[unsafe(method(setType:))]
        pub fn set_type(&self, item_type: i64);

        #[unsafe(method(setLinkCount:))]
        pub fn set_link_count(&self, count: u32);

        #[unsafe(method(setSize:))]
        pub fn set_size(&self, size: u64);

        #[unsafe(method(setAllocSize:))]
        pub fn set_alloc_size(&self, size: u64);

        #[unsafe(method(setFileID:))]
        pub fn set_file_id(&self, file_id: u64);

        #[unsafe(method(setParentID:))]
        pub fn set_parent_id(&self, parent_id: u64);
    }
);

/// Helper to populate an ObjC FSItemAttributes from our Rust ItemAttributes.
pub fn apply_attributes(
    fs_attrs: &FSItemAttributes,
    attrs: &crate::types::ItemAttributes,
) {
    fs_attrs.set_type(attrs.item_type as i64);
    fs_attrs.set_mode(attrs.mode);
    fs_attrs.set_uid(attrs.uid);
    fs_attrs.set_gid(attrs.gid);
    fs_attrs.set_link_count(attrs.link_count);
    fs_attrs.set_size(attrs.size);
    fs_attrs.set_alloc_size(attrs.alloc_size);
    fs_attrs.set_file_id(attrs.file_id);
    fs_attrs.set_parent_id(attrs.parent_id);
}

// ═══════════════════════════════════════════════════════════════════
// FSItemGetAttributesRequest
// ═══════════════════════════════════════════════════════════════════

extern_class!(
    #[unsafe(super(NSObject))]
    #[name = "FSItemGetAttributesRequest"]
    pub struct FSItemGetAttributesRequest;
);

// ═══════════════════════════════════════════════════════════════════
// FSVolumeIdentifier
// ═══════════════════════════════════════════════════════════════════

extern_class!(
    #[unsafe(super(NSObject))]
    #[name = "FSVolumeIdentifier"]
    pub struct FSVolumeIdentifier;
);

// ═══════════════════════════════════════════════════════════════════
// FSTaskOptions
// ═══════════════════════════════════════════════════════════════════

extern_class!(
    #[unsafe(super(NSObject))]
    #[name = "FSTaskOptions"]
    pub struct FSTaskOptions;
);

// ═══════════════════════════════════════════════════════════════════
// FSStatFSResult
// ═══════════════════════════════════════════════════════════════════

extern_class!(
    #[unsafe(super(NSObject))]
    #[name = "FSStatFSResult"]
    pub struct FSStatFSResult;
);

extern_methods!(
    unsafe impl FSStatFSResult {
        #[unsafe(method(initWithFileSystemTypeName:))]
        pub fn init_with_fs_type_name(
            this: objc2::rc::Allocated<Self>,
            name: &NSString,
        ) -> Retained<Self>;

        #[unsafe(method(setBlockSize:))]
        pub fn set_block_size(&self, size: i64);

        #[unsafe(method(setIoSize:))]
        pub fn set_io_size(&self, size: i64);

        #[unsafe(method(setTotalBlocks:))]
        pub fn set_total_blocks(&self, count: u64);

        #[unsafe(method(setFreeBlocks:))]
        pub fn set_free_blocks(&self, count: u64);

        #[unsafe(method(setUsedBlocks:))]
        pub fn set_used_blocks(&self, count: u64);

        #[unsafe(method(setTotalBytes:))]
        pub fn set_total_bytes(&self, count: u64);

        #[unsafe(method(setFreeBytes:))]
        pub fn set_free_bytes(&self, count: u64);

        #[unsafe(method(setUsedBytes:))]
        pub fn set_used_bytes(&self, count: u64);

        #[unsafe(method(setTotalFiles:))]
        pub fn set_total_files(&self, count: u64);

        #[unsafe(method(setFreeFiles:))]
        pub fn set_free_files(&self, count: u64);
    }
);

// ═══════════════════════════════════════════════════════════════════
// FSDirectoryEntryPacker
// ═══════════════════════════════════════════════════════════════════

extern_class!(
    #[unsafe(super(NSObject))]
    #[name = "FSDirectoryEntryPacker"]
    pub struct FSDirectoryEntryPacker;
);

extern_methods!(
    unsafe impl FSDirectoryEntryPacker {
        #[unsafe(method(packEntryWithName:itemType:itemID:nextCookie:attributes:))]
        pub fn pack_entry(
            &self,
            name: &FSFileName,
            item_type: i64,
            item_id: u64,
            next_cookie: u64,
            attributes: Option<&FSItemAttributes>,
        ) -> bool;
    }
);

// ═══════════════════════════════════════════════════════════════════
// FSMutableFileDataBuffer
// ═══════════════════════════════════════════════════════════════════

extern_class!(
    #[unsafe(super(NSObject))]
    #[name = "FSMutableFileDataBuffer"]
    pub struct FSMutableFileDataBuffer;
);

extern_methods!(
    unsafe impl FSMutableFileDataBuffer {
        #[unsafe(method(mutableBytes))]
        pub fn mutable_bytes(&self) -> *mut u8;

        #[unsafe(method(length))]
        pub fn length(&self) -> usize;
    }
);

// ═══════════════════════════════════════════════════════════════════
// FSVolume / FSUnaryFileSystem (base classes we subclass)
// ═══════════════════════════════════════════════════════════════════

extern_class!(
    #[unsafe(super(NSObject))]
    #[name = "FSVolume"]
    pub struct FSVolume;
);

extern_class!(
    #[unsafe(super(NSObject))]
    #[name = "FSUnaryFileSystem"]
    pub struct FSUnaryFileSystem;
);

// ═══════════════════════════════════════════════════════════════════
// FSProbeResult
// ═══════════════════════════════════════════════════════════════════

extern_class!(
    #[unsafe(super(NSObject))]
    #[name = "FSProbeResult"]
    pub struct FSProbeResult;
);

extern_methods!(
    unsafe impl FSProbeResult {
        #[unsafe(method(resultWithName:containerID:))]
        pub fn result_with_name(
            name: &NSString,
            container_id: &FSVolumeIdentifier,
        ) -> Retained<Self>;
    }
);

// ═══════════════════════════════════════════════════════════════════
// FSContainerStatus
// ═══════════════════════════════════════════════════════════════════

extern_class!(
    #[unsafe(super(NSObject))]
    #[name = "FSContainerStatus"]
    pub struct FSContainerStatus;
);

// ═══════════════════════════════════════════════════════════════════
// Helper: NSError from POSIX errno
// ═══════════════════════════════════════════════════════════════════

/// Create an NSError in the POSIX error domain with the given errno.
pub fn posix_error(errno: i32) -> Retained<NSError> {
    let domain = NSString::from_str("NSPOSIXErrorDomain");
    unsafe {
        NSError::initWithDomain_code_userInfo(
            NSError::alloc(),
            &domain,
            errno as isize,
            None,
        )
    }
}

/// Create an NSError from an `FsKitError`.
pub fn fskit_error_to_nserror(err: &crate::types::FsKitError) -> Retained<NSError> {
    posix_error(err.to_errno())
}
