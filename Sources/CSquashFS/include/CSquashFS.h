/*
 * CSquashFS.h — Umbrella header for Swift bridge to libsquashfs
 *
 * All libsqfs state is behind opaque void* handles so Swift
 * never encounters incomplete C struct types.
 */
#pragma once

#include <stdint.h>
#include <stddef.h>

/* Bring in types we CAN expose to Swift (fully defined structs) */
#include <sqfs/predef.h>
#include <sqfs/super.h>
#include <sqfs/inode.h>
#include <sqfs/dir_reader.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ── Image handle (void* — opaque to Swift) ── */

/**
 * Open a SquashFS image. Returns an opaque handle (void*).
 * Returns NULL on failure.
 * The caller must call csqfs_close() when done.
 */
void *csqfs_open(const char *path);

/**
 * Close and free all resources for a handle.
 */
void csqfs_close(void *handle);

/**
 * Get the superblock. Returns a pointer to the superblock inside the handle.
 * Valid for the lifetime of the handle.
 */
const sqfs_super_t *csqfs_get_super(const void *handle);

/**
 * Get the full directory tree.
 * The tree is owned by the handle and freed in csqfs_close().
 * Returns NULL on failure.
 */
sqfs_tree_node_t *csqfs_get_tree(void *handle);

/**
 * Read file data from the image.
 * Returns 0 on success, non-zero on error.
 */
int csqfs_read_file(void *handle,
                    const sqfs_inode_generic_t *inode,
                    uint64_t offset,
                    void *buffer,
                    uint32_t size);

/* ── Tree node helpers ── */

/** Get the name of a tree node (flexible array member access). */
const char *csqfs_tree_node_get_name(const sqfs_tree_node_t *node);

/* ── Inode helpers ── */

/** Get file size. Returns 0 for non-files. */
uint64_t csqfs_inode_get_file_size_val(const sqfs_inode_generic_t *inode);

/** Get symlink target string. Returns NULL for non-symlinks. */
const char *csqfs_inode_get_symlink_target(const sqfs_inode_generic_t *inode);

/** Get symlink target length. Returns 0 for non-symlinks. */
uint32_t csqfs_inode_get_symlink_size(const sqfs_inode_generic_t *inode);

#ifdef __cplusplus
}
#endif
