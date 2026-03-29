//! Dynamic ObjC class registration for FSKit.
//!
//! Uses `objc2::define_class!` to create ObjC subclasses at runtime:
//! - `SquashboxVolume` subclasses `FSVolume` and implements `FSVolumeOperations`
//! - `SquashboxUnaryFS` subclasses `FSUnaryFileSystem` and implements
//!   `FSUnaryFileSystemOperations`
//!
//! Both classes hold a raw pointer to a boxed `dyn FsKitFileSystemSource`
//! trait object. When FSKit invokes an ObjC method (e.g., `lookupItemNamed:`),
//! the method body dereferences the trait pointer and calls the corresponding
//! Rust method.
//!
//! # Safety
//!
//! The raw pointer stored in ivars must remain valid for the lifetime of the
//! ObjC object. This is enforced by the `SquashboxFsKitRuntime` which owns
//! the `Box<dyn FsKitFileSystemSource>` and must outlive the ObjC objects.

use std::cell::Cell;
use std::ptr::NonNull;
use std::sync::Arc;

use objc2::runtime::NSObject;
use objc2::{define_class, msg_send, ClassType, DefinedClass};
use objc2_foundation::NSError;

use crate::objc_bindings::*;
use crate::FsKitFileSystemSource;

// ═══════════════════════════════════════════════════════════════════
// SquashboxVolume — subclass of FSVolume
// ═══════════════════════════════════════════════════════════════════

/// Instance variables for our FSVolume subclass.
#[derive(Debug)]
pub struct SquashboxVolumeIvars {
    /// Raw pointer to the Rust source implementation.
    /// Stored as a Cell because ObjC method receivers are &self.
    source: Cell<Option<NonNull<dyn FsKitFileSystemSource>>>,
}

// Safety: The source pointer is only accessed on FSKit's dispatch queues,
// and FsKitFileSystemSource requires Send + Sync.
unsafe impl Send for SquashboxVolumeIvars {}
unsafe impl Sync for SquashboxVolumeIvars {}

impl Default for SquashboxVolumeIvars {
    fn default() -> Self {
        Self {
            source: Cell::new(None),
        }
    }
}

define_class!(
    #[unsafe(super = FSVolume)]
    #[name = "SquashboxVolume"]
    #[ivars = SquashboxVolumeIvars]
    pub struct SquashboxVolume;

    // ── FSVolumeOperations ──

    impl SquashboxVolume {
        /// Called by FSKit to activate the volume.
        #[unsafe(method(activateWithOptions:replyHandler:))]
        fn activate_with_options(
            &self,
            _options: *mut NSObject, // FSTaskOptions
            reply: *mut block2::Block<dyn Fn(*mut FSItem, *mut NSError)>,
        ) {
            let reply = unsafe { &*reply };
            if let Some(source) = self.source_ref() {
                match source.activate() {
                    Ok(root_attrs) => {
                        let root_item = FSItem::new();
                        let objc_attrs = FSItemAttributes::new();
                        apply_attributes(&objc_attrs, &root_attrs);
                        reply.call((
                            &*root_item as *const FSItem as *mut FSItem,
                            std::ptr::null_mut(),
                        ));
                    }
                    Err(e) => {
                        log::error!("activate failed: {}", e);
                        let nserr = fskit_error_to_nserror(&e);
                        reply.call((
                            std::ptr::null_mut(),
                            &*nserr as *const NSError as *mut NSError,
                        ));
                    }
                }
            } else {
                let nserr = posix_error(libc::EIO);
                reply.call((
                    std::ptr::null_mut(),
                    &*nserr as *const NSError as *mut NSError,
                ));
            }
        }

        /// Called by FSKit to look up an item in a directory.
        #[unsafe(method(lookupItemNamed:inDirectory:replyHandler:))]
        fn lookup_item(
            &self,
            name: *mut FSFileName,
            _directory: *mut FSItem,
            reply: *mut block2::Block<dyn Fn(*mut FSItem, *mut FSFileName, *mut NSError)>,
        ) {
            let reply = unsafe { &*reply };
            let name_str = unsafe { &*name }.to_string_lossy();
            let parent_id: u64 = unsafe { msg_send![&*_directory, fileID] };

            if let Some(source) = self.source_ref() {
                match source.lookup(parent_id, &name_str) {
                    Ok((_item_id, stored_name, attrs)) => {
                        let item = FSItem::new();
                        let objc_attrs = FSItemAttributes::new();
                        apply_attributes(&objc_attrs, &attrs);
                        let fs_name = FSFileName::from_str(&stored_name);
                        reply.call((
                            &*item as *const FSItem as *mut FSItem,
                            &*fs_name as *const FSFileName as *mut FSFileName,
                            std::ptr::null_mut(),
                        ));
                    }
                    Err(e) => {
                        let nserr = fskit_error_to_nserror(&e);
                        reply.call((
                            std::ptr::null_mut(),
                            std::ptr::null_mut(),
                            &*nserr as *const NSError as *mut NSError,
                        ));
                    }
                }
            } else {
                let nserr = posix_error(libc::EIO);
                reply.call((
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    &*nserr as *const NSError as *mut NSError,
                ));
            }
        }

        /// Called by FSKit to enumerate a directory.
        #[unsafe(method(enumerateDirectory:startingAtCookie:verifier:providingAttributes:usingPacker:replyHandler:))]
        fn enumerate_directory(
            &self,
            directory: *mut FSItem,
            cookie: u64,
            _verifier: u64,
            _attributes: *mut FSItemGetAttributesRequest,
            packer: *mut FSDirectoryEntryPacker,
            reply: *mut block2::Block<dyn Fn(u64, *mut NSError)>,
        ) {
            let reply = unsafe { &*reply };
            let packer = unsafe { &*packer };
            let item_id: u64 = unsafe { msg_send![&*directory, fileID] };

            if let Some(source) = self.source_ref() {
                match source.enumerate_directory(item_id, cookie) {
                    Ok(entries) => {
                        for entry in &entries {
                            let name = FSFileName::from_str(&entry.name);
                            let should_continue = packer.pack_entry(
                                &name,
                                entry.item_type as i64,
                                entry.item_id,
                                entry.next_cookie,
                                None, // TODO: pack attributes if requested
                            );
                            if !should_continue {
                                break;
                            }
                        }
                        // Use a simple incrementing verifier
                        reply.call((1u64, std::ptr::null_mut()));
                    }
                    Err(e) => {
                        let nserr = fskit_error_to_nserror(&e);
                        reply.call((
                            0u64,
                            &*nserr as *const NSError as *mut NSError,
                        ));
                    }
                }
            } else {
                let nserr = posix_error(libc::EIO);
                reply.call((0u64, &*nserr as *const NSError as *mut NSError));
            }
        }

        /// Called by FSKit to get attributes of an item.
        #[unsafe(method(getAttributes:ofItem:replyHandler:))]
        fn get_attributes(
            &self,
            _desired: *mut FSItemGetAttributesRequest,
            item: *mut FSItem,
            reply: *mut block2::Block<dyn Fn(*mut FSItemAttributes, *mut NSError)>,
        ) {
            let reply = unsafe { &*reply };
            let item_id: u64 = unsafe { msg_send![&*item, fileID] };

            if let Some(source) = self.source_ref() {
                match source.get_attributes(item_id) {
                    Ok(attrs) => {
                        let objc_attrs = FSItemAttributes::new();
                        apply_attributes(&objc_attrs, &attrs);
                        reply.call((
                            &*objc_attrs as *const FSItemAttributes
                                as *mut FSItemAttributes,
                            std::ptr::null_mut(),
                        ));
                    }
                    Err(e) => {
                        let nserr = fskit_error_to_nserror(&e);
                        reply.call((
                            std::ptr::null_mut(),
                            &*nserr as *const NSError as *mut NSError,
                        ));
                    }
                }
            } else {
                let nserr = posix_error(libc::EIO);
                reply.call((
                    std::ptr::null_mut(),
                    &*nserr as *const NSError as *mut NSError,
                ));
            }
        }

        /// Called by FSKit to read a symbolic link.
        #[unsafe(method(readSymbolicLink:replyHandler:))]
        fn read_symbolic_link(
            &self,
            item: *mut FSItem,
            reply: *mut block2::Block<dyn Fn(*mut FSFileName, *mut NSError)>,
        ) {
            let reply = unsafe { &*reply };
            let item_id: u64 = unsafe { msg_send![&*item, fileID] };

            if let Some(source) = self.source_ref() {
                match source.read_symlink(item_id) {
                    Ok(target) => {
                        let name = FSFileName::from_str(&target);
                        reply.call((
                            &*name as *const FSFileName as *mut FSFileName,
                            std::ptr::null_mut(),
                        ));
                    }
                    Err(e) => {
                        let nserr = fskit_error_to_nserror(&e);
                        reply.call((
                            std::ptr::null_mut(),
                            &*nserr as *const NSError as *mut NSError,
                        ));
                    }
                }
            } else {
                let nserr = posix_error(libc::EIO);
                reply.call((
                    std::ptr::null_mut(),
                    &*nserr as *const NSError as *mut NSError,
                ));
            }
        }

        /// Called by FSKit to reclaim an item.
        #[unsafe(method(reclaimItem:replyHandler:))]
        fn reclaim_item(
            &self,
            item: *mut FSItem,
            reply: *mut block2::Block<dyn Fn(*mut NSError)>,
        ) {
            let reply = unsafe { &*reply };
            let item_id: u64 = unsafe { msg_send![&*item, fileID] };

            if let Some(source) = self.source_ref() {
                let _ = source.reclaim(item_id);
            }
            reply.call((std::ptr::null_mut(),));
        }

        // ── Write operations — all return EROFS ──

        #[unsafe(method(createItemNamed:type:inDirectory:attributes:replyHandler:))]
        fn create_item(
            &self,
            _name: *mut FSFileName,
            _item_type: i64,
            _directory: *mut FSItem,
            _attrs: *mut NSObject,
            reply: *mut block2::Block<dyn Fn(*mut FSItem, *mut FSFileName, *mut NSError)>,
        ) {
            let reply = unsafe { &*reply };
            let nserr = posix_error(libc::EROFS);
            reply.call((
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &*nserr as *const NSError as *mut NSError,
            ));
        }

        #[unsafe(method(removeItem:named:fromDirectory:replyHandler:))]
        fn remove_item(
            &self,
            _item: *mut FSItem,
            _name: *mut FSFileName,
            _directory: *mut FSItem,
            reply: *mut block2::Block<dyn Fn(*mut NSError)>,
        ) {
            let reply = unsafe { &*reply };
            let nserr = posix_error(libc::EROFS);
            reply.call((&*nserr as *const NSError as *mut NSError,));
        }

        #[unsafe(method(setAttributes:onItem:replyHandler:))]
        fn set_attributes(
            &self,
            _attrs: *mut NSObject,
            _item: *mut FSItem,
            reply: *mut block2::Block<dyn Fn(*mut FSItemAttributes, *mut NSError)>,
        ) {
            let reply = unsafe { &*reply };
            let nserr = posix_error(libc::EROFS);
            reply.call((
                std::ptr::null_mut(),
                &*nserr as *const NSError as *mut NSError,
            ));
        }

        // ── Mount / unmount / sync ──

        #[unsafe(method(mountWithOptions:replyHandler:))]
        fn mount(
            &self,
            _options: *mut NSObject,
            reply: *mut block2::Block<dyn Fn(*mut NSError)>,
        ) {
            let reply = unsafe { &*reply };
            // Read-only mount always succeeds
            reply.call((std::ptr::null_mut(),));
        }

        #[unsafe(method(unmountWithReplyHandler:))]
        fn unmount(
            &self,
            reply: *mut block2::Block<dyn Fn()>,
        ) {
            let reply = unsafe { &*reply };
            reply.call(());
        }

        #[unsafe(method(synchronizeWithFlags:replyHandler:))]
        fn synchronize(
            &self,
            _flags: i64,
            reply: *mut block2::Block<dyn Fn(*mut NSError)>,
        ) {
            let reply = unsafe { &*reply };
            // Read-only: nothing to sync
            reply.call((std::ptr::null_mut(),));
        }

        #[unsafe(method(deactivateWithOptions:replyHandler:))]
        fn deactivate(
            &self,
            _options: u64,
            reply: *mut block2::Block<dyn Fn(*mut NSError)>,
        ) {
            let reply = unsafe { &*reply };
            if let Some(source) = self.source_ref() {
                source.unload();
            }
            reply.call((std::ptr::null_mut(),));
        }
    }
);

impl SquashboxVolume {
    /// Set the source implementation. Must be called before any FSKit callbacks.
    pub fn set_source(&self, source: Arc<dyn FsKitFileSystemSource>) {
        let raw = Arc::into_raw(source) as *mut dyn FsKitFileSystemSource;
        self.ivars().source.set(NonNull::new(raw));
    }

    /// Get a reference to the source. Returns None if not yet set.
    fn source_ref(&self) -> Option<&dyn FsKitFileSystemSource> {
        self.ivars().source.get().map(|ptr| unsafe { ptr.as_ref() })
    }
}

// ═══════════════════════════════════════════════════════════════════
// SquashboxUnaryFS — subclass of FSUnaryFileSystem
// ═══════════════════════════════════════════════════════════════════

/// Instance variables for our FSUnaryFileSystem subclass.
#[derive(Debug)]
pub struct SquashboxUnaryFSIvars {
    source: Cell<Option<NonNull<dyn FsKitFileSystemSource>>>,
}

unsafe impl Send for SquashboxUnaryFSIvars {}
unsafe impl Sync for SquashboxUnaryFSIvars {}

impl Default for SquashboxUnaryFSIvars {
    fn default() -> Self {
        Self {
            source: Cell::new(None),
        }
    }
}

define_class!(
    #[unsafe(super = FSUnaryFileSystem)]
    #[name = "SquashboxUnaryFileSystem"]
    #[ivars = SquashboxUnaryFSIvars]
    pub struct SquashboxUnaryFS;

    // ── FSUnaryFileSystemOperations ──

    impl SquashboxUnaryFS {
        /// Probe the resource to see if it's a valid SquashFS image.
        #[unsafe(method(probeResource:replyHandler:))]
        fn probe_resource(
            &self,
            _resource: *mut FSResource,
            reply: *mut block2::Block<dyn Fn(*mut FSProbeResult, *mut NSError)>,
        ) {
            let reply = unsafe { &*reply };
            // TODO: Get URL from resource and call source.probe()
            // For now, accept all resources
            let nserr = posix_error(libc::ENOTSUP);
            reply.call((
                std::ptr::null_mut(),
                &*nserr as *const NSError as *mut NSError,
            ));
        }

        /// Load the resource and return a volume.
        #[unsafe(method(loadResource:options:replyHandler:))]
        fn load_resource(
            &self,
            _resource: *mut FSResource,
            _options: *mut FSTaskOptions,
            reply: *mut block2::Block<dyn Fn(*mut FSVolume, *mut NSError)>,
        ) {
            let reply = unsafe { &*reply };

            if let Some(_source) = self.source_ref() {
                // TODO: Extract path from FSResource
                let nserr = posix_error(libc::ENOTSUP);
                reply.call((
                    std::ptr::null_mut(),
                    &*nserr as *const NSError as *mut NSError,
                ));
            } else {
                let nserr = posix_error(libc::EIO);
                reply.call((
                    std::ptr::null_mut(),
                    &*nserr as *const NSError as *mut NSError,
                ));
            }
        }

        /// Unload the resource.
        #[unsafe(method(unloadResource:options:replyHandler:))]
        fn unload_resource(
            &self,
            _resource: *mut FSResource,
            _options: *mut FSTaskOptions,
            reply: *mut block2::Block<dyn Fn(*mut NSError)>,
        ) {
            let reply = unsafe { &*reply };
            if let Some(source) = self.source_ref() {
                source.unload();
            }
            reply.call((std::ptr::null_mut(),));
        }
    }
);

impl SquashboxUnaryFS {
    /// Set the source implementation.
    pub fn set_source(&self, source: Arc<dyn FsKitFileSystemSource>) {
        let raw = Arc::into_raw(source) as *mut dyn FsKitFileSystemSource;
        self.ivars().source.set(NonNull::new(raw));
    }

    fn source_ref(&self) -> Option<&dyn FsKitFileSystemSource> {
        self.ivars().source.get().map(|ptr| unsafe { ptr.as_ref() })
    }
}

// ═══════════════════════════════════════════════════════════════════
// Public runtime initialization
// ═══════════════════════════════════════════════════════════════════

/// Ensure the ObjC classes are registered with the runtime.
///
/// Call this once at startup (e.g., from the appex entry point) before
/// FSKit attempts to instantiate the principal class.
pub fn register_classes() {
    // Accessing the class triggers define_class!'s lazy registration.
    let _ = SquashboxUnaryFS::class();
    let _ = SquashboxVolume::class();
    log::info!("FSKit ObjC classes registered: SquashboxUnaryFileSystem, SquashboxVolume");
}
