/*
 * CProjFS.h — C bridge for Windows Projected File System (ProjFS)
 *
 * Wraps the Win32 ProjFS API behind a simple C interface that Swift
 * can call. Uses void* context pointers for callback dispatch.
 *
 * The provider must implement 5 callback functions:
 * - start_enum, end_enum, get_enum: directory enumeration
 * - get_placeholder_info: file/dir metadata
 * - get_file_data: file content hydration
 */
#pragma once

#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <projectedfslib.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ── Callback typedefs ── */

/*
 * All callbacks receive:
 * - ctx: the provider context (void* passed to cprojfs_start)
 * - path: the relative file path (UTF-8)
 * Additional params are callback-specific.
 * Return S_OK on success, HRESULT error code on failure.
 */

/** Start enumerating a directory. Session ID for matching start/end. */
typedef HRESULT (*cprojfs_start_enum_cb)(void *ctx, const char *path,
                                         const GUID *enumId);

/** End a directory enumeration session. */
typedef HRESULT (*cprojfs_end_enum_cb)(void *ctx, const char *path,
                                       const GUID *enumId);

/**
 * Get directory entries for an enumeration.
 * The callback should call cprojfs_add_entry() for each entry,
 * then return S_OK when done for this batch.
 */
typedef HRESULT (*cprojfs_get_enum_cb)(void *ctx, const char *path,
                                       const GUID *enumId,
                                       const char *searchExpr,
                                       void *entryBuffer);

/** Get placeholder info for a file/directory. */
typedef HRESULT (*cprojfs_get_placeholder_cb)(void *ctx, const char *path,
                                              void *nsCtx);

/** Get file data (hydration). */
typedef HRESULT (*cprojfs_get_file_data_cb)(void *ctx, const char *path,
                                            void *nsCtx,
                                            const GUID *streamId,
                                            uint64_t byteOffset,
                                            uint32_t length);

/** Notification callback (optional — we deny all writes). */
typedef HRESULT (*cprojfs_notification_cb)(void *ctx, const char *path,
                                           int isDirectory,
                                           uint32_t notification);

/* ── Callback table ── */

typedef struct {
    cprojfs_start_enum_cb      startEnum;
    cprojfs_end_enum_cb        endEnum;
    cprojfs_get_enum_cb        getEnum;
    cprojfs_get_placeholder_cb getPlaceholder;
    cprojfs_get_file_data_cb   getFileData;
    cprojfs_notification_cb    notification;   /* may be NULL */
} cprojfs_callbacks;

/* ── API ── */

/**
 * Start ProjFS virtualization on the given directory.
 * Returns an opaque handle, or NULL on failure.
 * The directory must exist and should be empty or a previous mount root.
 *
 * @param rootPath      UTF-8 path to the virtualization root directory
 * @param callbacks     Table of callback function pointers
 * @param providerCtx   Opaque context passed to every callback
 */
void *cprojfs_start(const char *rootPath,
                    const cprojfs_callbacks *callbacks,
                    void *providerCtx);

/**
 * Stop ProjFS virtualization and free the handle.
 */
void cprojfs_stop(void *handle);

/* ── Helpers for use inside callbacks ── */

/**
 * Add a directory entry to the enumeration buffer.
 * Call from inside the getEnum callback.
 *
 * @param entryBuffer   The buffer handle from the callback
 * @param name          Entry name (UTF-8)
 * @param isDirectory   Non-zero for directories
 * @param fileSize      File size in bytes (ignored for directories)
 * @return S_OK on success, HRESULT_FROM_WIN32(ERROR_INSUFFICIENT_BUFFER) if full
 */
HRESULT cprojfs_add_entry(void *entryBuffer,
                          const char *name,
                          int isDirectory,
                          int64_t fileSize);

/**
 * Write placeholder info for a file or directory.
 * Call from inside the getPlaceholder callback.
 *
 * @param nsCtx         The namespace context from the callback
 * @param relativePath  Relative path from root (UTF-8)
 * @param isDirectory   Non-zero for directories
 * @param fileSize      File size in bytes (ignored for directories)
 */
HRESULT cprojfs_write_placeholder(void *nsCtx,
                                   const char *relativePath,
                                   int isDirectory,
                                   int64_t fileSize);

/**
 * Write file data for hydration.
 * Call from inside the getFileData callback.
 *
 * @param nsCtx         The namespace context from the callback
 * @param streamId      The data stream GUID from the callback
 * @param data          Buffer containing file data
 * @param byteOffset    Offset into the file
 * @param length        Number of bytes
 */
HRESULT cprojfs_write_file_data(void *nsCtx,
                                 const GUID *streamId,
                                 const void *data,
                                 uint64_t byteOffset,
                                 uint32_t length);

/* ── Reparse point cleanup ── */

/**
 * Remove a stale ProjFS reparse point from a directory.
 * Idempotent — returns S_OK if the directory has no reparse point.
 * Returns an HRESULT error code on failure.
 */
HRESULT cprojfs_delete_reparse_point(const char *dirPath);

/**
 * Check if a directory has a reparse point.
 * Returns non-zero if it does, zero if it doesn't or doesn't exist.
 */
int cprojfs_has_reparse_point(const char *dirPath);

#ifdef __cplusplus
}
#endif
