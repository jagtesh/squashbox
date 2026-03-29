/*
 * cprojfs_shim.c — Implementation of the CProjFS bridge
 *
 * Wraps the Win32 ProjFS API and provides callback trampolines
 * that convert wide strings to UTF-8 before forwarding to the
 * provider callbacks.
 */

#define WIN32_LEAN_AND_MEAN
#include "CProjFS.h"
#include <winioctl.h>
#include <objbase.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>

/* ── Internal handle ── */

typedef struct {
    PRJ_NAMESPACE_VIRTUALIZATION_CONTEXT nsContext;
    cprojfs_callbacks callbacks;
    void *providerCtx;
} cprojfs_handle;

/* ── UTF-8 ↔ UTF-16 conversions ── */

static wchar_t *utf8_to_wide(const char *utf8) {
    if (!utf8) return NULL;
    int len = MultiByteToWideChar(CP_UTF8, 0, utf8, -1, NULL, 0);
    if (len <= 0) return NULL;
    wchar_t *wide = (wchar_t *)malloc(len * sizeof(wchar_t));
    if (!wide) return NULL;
    MultiByteToWideChar(CP_UTF8, 0, utf8, -1, wide, len);
    return wide;
}

static char *wide_to_utf8(const wchar_t *wide) {
    if (!wide) return NULL;
    int len = WideCharToMultiByte(CP_UTF8, 0, wide, -1, NULL, 0, NULL, NULL);
    if (len <= 0) return NULL;
    char *utf8 = (char *)malloc(len);
    if (!utf8) return NULL;
    WideCharToMultiByte(CP_UTF8, 0, wide, -1, utf8, len, NULL, NULL);
    return utf8;
}

/* ── ProjFS callback trampolines ── */

static HRESULT CALLBACK trampoline_start_enum(
    const PRJ_CALLBACK_DATA *callbackData,
    const GUID *enumerationId
) {
    cprojfs_handle *h = (cprojfs_handle *)callbackData->InstanceContext;
    char *path = wide_to_utf8(callbackData->FilePathName);
    HRESULT hr = h->callbacks.startEnum(h->providerCtx, path ? path : "", enumerationId);
    free(path);
    return hr;
}

static HRESULT CALLBACK trampoline_end_enum(
    const PRJ_CALLBACK_DATA *callbackData,
    const GUID *enumerationId
) {
    cprojfs_handle *h = (cprojfs_handle *)callbackData->InstanceContext;
    char *path = wide_to_utf8(callbackData->FilePathName);
    HRESULT hr = h->callbacks.endEnum(h->providerCtx, path ? path : "", enumerationId);
    free(path);
    return hr;
}

static HRESULT CALLBACK trampoline_get_enum(
    const PRJ_CALLBACK_DATA *callbackData,
    const GUID *enumerationId,
    PCWSTR searchExpression,
    PRJ_DIR_ENTRY_BUFFER_HANDLE dirEntryBufferHandle
) {
    cprojfs_handle *h = (cprojfs_handle *)callbackData->InstanceContext;
    char *path = wide_to_utf8(callbackData->FilePathName);
    char *search = wide_to_utf8(searchExpression);
    HRESULT hr = h->callbacks.getEnum(h->providerCtx, path ? path : "",
                                       enumerationId, search,
                                       (void *)dirEntryBufferHandle);
    free(search);
    free(path);
    return hr;
}

static HRESULT CALLBACK trampoline_get_placeholder(
    const PRJ_CALLBACK_DATA *callbackData
) {
    cprojfs_handle *h = (cprojfs_handle *)callbackData->InstanceContext;
    char *path = wide_to_utf8(callbackData->FilePathName);
    HRESULT hr = h->callbacks.getPlaceholder(h->providerCtx, path ? path : "",
                                              (void *)callbackData->NamespaceVirtualizationContext);
    free(path);
    return hr;
}

static HRESULT CALLBACK trampoline_get_file_data(
    const PRJ_CALLBACK_DATA *callbackData,
    UINT64 byteOffset,
    UINT32 length
) {
    cprojfs_handle *h = (cprojfs_handle *)callbackData->InstanceContext;
    char *path = wide_to_utf8(callbackData->FilePathName);
    HRESULT hr = h->callbacks.getFileData(h->providerCtx, path ? path : "",
                                           (void *)callbackData->NamespaceVirtualizationContext,
                                           &callbackData->DataStreamId,
                                           byteOffset, length);
    free(path);
    return hr;
}

static HRESULT CALLBACK trampoline_notification(
    const PRJ_CALLBACK_DATA *callbackData,
    BOOLEAN isDirectory,
    PRJ_NOTIFICATION notification,
    PCWSTR destinationFileName,
    PRJ_NOTIFICATION_PARAMETERS *operationParameters
) {
    cprojfs_handle *h = (cprojfs_handle *)callbackData->InstanceContext;
    if (!h->callbacks.notification) {
        /* Default: deny all modifications (read-only filesystem) */
        return HRESULT_FROM_WIN32(ERROR_ACCESS_DENIED);
    }
    char *path = wide_to_utf8(callbackData->FilePathName);
    HRESULT hr = h->callbacks.notification(h->providerCtx, path ? path : "",
                                            isDirectory, (uint32_t)notification);
    free(path);
    return hr;
}

/* ── Public API ── */

void *cprojfs_start(const char *rootPath,
                    const cprojfs_callbacks *callbacks,
                    void *providerCtx) {
    cprojfs_handle *h = (cprojfs_handle *)calloc(1, sizeof(cprojfs_handle));
    if (!h) return NULL;

    h->callbacks = *callbacks;
    h->providerCtx = providerCtx;

    wchar_t *widePath = utf8_to_wide(rootPath);
    if (!widePath) { free(h); return NULL; }

    /* Mark the directory as a virtualization root */
    GUID instanceId;
    CoCreateGuid(&instanceId);
    HRESULT hr = PrjMarkDirectoryAsPlaceholder(widePath, NULL, NULL, &instanceId);
    if (FAILED(hr) && hr != HRESULT_FROM_WIN32(ERROR_REPARSE_POINT_ENCOUNTERED)) {
        free(widePath);
        free(h);
        return NULL;
    }

    /* Set up callbacks */
    PRJ_CALLBACKS prjCallbacks;
    memset(&prjCallbacks, 0, sizeof(prjCallbacks));
    prjCallbacks.StartDirectoryEnumerationCallback = trampoline_start_enum;
    prjCallbacks.EndDirectoryEnumerationCallback   = trampoline_end_enum;
    prjCallbacks.GetDirectoryEnumerationCallback    = trampoline_get_enum;
    prjCallbacks.GetPlaceholderInfoCallback         = trampoline_get_placeholder;
    prjCallbacks.GetFileDataCallback                = trampoline_get_file_data;
    prjCallbacks.NotificationCallback               = trampoline_notification;

    /* Start virtualization */
    PRJ_STARTVIRTUALIZING_OPTIONS options;
    memset(&options, 0, sizeof(options));

    /* Set up notification mapping to deny all writes */
    PRJ_NOTIFICATION_MAPPING notifMapping;
    notifMapping.NotificationBitMask = PRJ_NOTIFY_PRE_DELETE |
                                       PRJ_NOTIFY_PRE_RENAME |
                                       PRJ_NOTIFY_PRE_SET_HARDLINK;
    notifMapping.NotificationRoot = L"";
    options.NotificationMappings = &notifMapping;
    options.NotificationMappingsCount = 1;

    hr = PrjStartVirtualizing(widePath, &prjCallbacks, h, &options, &h->nsContext);
    free(widePath);

    if (FAILED(hr)) {
        free(h);
        return NULL;
    }

    return h;
}

void cprojfs_stop(void *handle) {
    cprojfs_handle *h = (cprojfs_handle *)handle;
    if (!h) return;
    if (h->nsContext) {
        PrjStopVirtualizing(h->nsContext);
    }
    free(h);
}

/* ── Helpers for use inside callbacks ── */

HRESULT cprojfs_add_entry(void *entryBuffer,
                          const char *name,
                          int isDirectory,
                          int64_t fileSize) {
    PRJ_DIR_ENTRY_BUFFER_HANDLE buf = (PRJ_DIR_ENTRY_BUFFER_HANDLE)entryBuffer;
    wchar_t *wideName = utf8_to_wide(name);
    if (!wideName) return E_OUTOFMEMORY;

    PRJ_FILE_BASIC_INFO info;
    memset(&info, 0, sizeof(info));
    info.IsDirectory = isDirectory ? TRUE : FALSE;
    info.FileSize = fileSize;

    HRESULT hr = PrjFillDirEntryBuffer(wideName, &info, buf);
    free(wideName);
    return hr;
}

HRESULT cprojfs_write_placeholder(void *nsCtx,
                                   const char *relativePath,
                                   int isDirectory,
                                   int64_t fileSize) {
    PRJ_NAMESPACE_VIRTUALIZATION_CONTEXT ctx = (PRJ_NAMESPACE_VIRTUALIZATION_CONTEXT)nsCtx;
    wchar_t *widePath = utf8_to_wide(relativePath);
    if (!widePath) return E_OUTOFMEMORY;

    PRJ_PLACEHOLDER_INFO info;
    memset(&info, 0, sizeof(info));
    info.FileBasicInfo.IsDirectory = isDirectory ? TRUE : FALSE;
    info.FileBasicInfo.FileSize = fileSize;

    HRESULT hr = PrjWritePlaceholderInfo(ctx, widePath, &info, sizeof(info));
    free(widePath);
    return hr;
}

HRESULT cprojfs_write_file_data(void *nsCtx,
                                 const GUID *streamId,
                                 const void *data,
                                 uint64_t byteOffset,
                                 uint32_t length) {
    PRJ_NAMESPACE_VIRTUALIZATION_CONTEXT ctx = (PRJ_NAMESPACE_VIRTUALIZATION_CONTEXT)nsCtx;

    /* ProjFS requires aligned buffers */
    void *alignedBuf = PrjAllocateAlignedBuffer(ctx, length);
    if (!alignedBuf) return E_OUTOFMEMORY;

    memcpy(alignedBuf, data, length);
    HRESULT hr = PrjWriteFileData(ctx, streamId, alignedBuf, byteOffset, length);
    PrjFreeAlignedBuffer(alignedBuf);
    return hr;
}

/* ── Reparse point cleanup ── */

int cprojfs_has_reparse_point(const char *dirPath) {
    wchar_t *widePath = utf8_to_wide(dirPath);
    if (!widePath) return 0;

    DWORD attrs = GetFileAttributesW(widePath);
    free(widePath);

    if (attrs == INVALID_FILE_ATTRIBUTES) return 0;
    return (attrs & FILE_ATTRIBUTE_REPARSE_POINT) ? 1 : 0;
}

HRESULT cprojfs_delete_reparse_point(const char *dirPath) {
    wchar_t *widePath = utf8_to_wide(dirPath);
    if (!widePath) return E_OUTOFMEMORY;

    /* Check if reparse point exists */
    DWORD attrs = GetFileAttributesW(widePath);
    if (attrs == INVALID_FILE_ATTRIBUTES) {
        free(widePath);
        return HRESULT_FROM_WIN32(GetLastError());
    }
    if (!(attrs & FILE_ATTRIBUTE_REPARSE_POINT)) {
        free(widePath);
        return S_OK; /* Already clean */
    }

    /* Open with reparse point access */
    HANDLE hDir = CreateFileW(
        widePath,
        GENERIC_WRITE,
        0,
        NULL,
        OPEN_EXISTING,
        FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
        NULL
    );
    free(widePath);

    if (hDir == INVALID_HANDLE_VALUE) {
        return HRESULT_FROM_WIN32(GetLastError());
    }

    /* Read current reparse tag */
    BYTE buf[1024];
    DWORD bytesReturned = 0;
    BOOL ok = DeviceIoControl(
        hDir,
        FSCTL_GET_REPARSE_POINT,
        NULL, 0,
        buf, sizeof(buf),
        &bytesReturned,
        NULL
    );
    if (!ok) {
        HRESULT hr = HRESULT_FROM_WIN32(GetLastError());
        CloseHandle(hDir);
        return hr;
    }

    /* Extract tag and delete */
    typedef struct {
        ULONG ReparseTag;
        USHORT ReparseDataLength;
        USHORT Reserved;
    } REPARSE_DELETE_BUF;

    REPARSE_DELETE_BUF delBuf;
    memset(&delBuf, 0, sizeof(delBuf));
    memcpy(&delBuf.ReparseTag, buf, sizeof(ULONG));
    delBuf.ReparseDataLength = 0;

    ok = DeviceIoControl(
        hDir,
        FSCTL_DELETE_REPARSE_POINT,
        &delBuf, sizeof(delBuf),
        NULL, 0,
        &bytesReturned,
        NULL
    );

    HRESULT hr = ok ? S_OK : HRESULT_FROM_WIN32(GetLastError());
    CloseHandle(hDir);
    return hr;
}
