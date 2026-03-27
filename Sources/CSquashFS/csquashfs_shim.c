/*
 * csquashfs_shim.c — Implementation of the CSquashFS bridge
 */

#include "CSquashFS.h"
#include <sqfs/compressor.h>
#include <sqfs/io.h>
#include <sqfs/data_reader.h>
#include <sqfs/id_table.h>
#include <sqfs/error.h>
#include <stdlib.h>
#include <string.h>

/* ── Internal handle definition ── */

typedef struct {
    sqfs_file_t          *file;
    sqfs_super_t          super;
    sqfs_compressor_t    *compressor;
    sqfs_dir_reader_t    *dir_reader;
    sqfs_id_table_t      *id_table;
    sqfs_data_reader_t   *data_reader;
    sqfs_tree_node_t     *tree;
} csqfs_handle_internal;

static void _destroy(void *obj) {
    if (obj)
        ((sqfs_object_t *)obj)->destroy((sqfs_object_t *)obj);
}

/* ── Public API ── */

void *csqfs_open(const char *path) {
    csqfs_handle_internal *h = (csqfs_handle_internal *)calloc(1, sizeof(*h));
    if (!h) return NULL;

    int ret;

    h->file = sqfs_open_file(path, SQFS_FILE_OPEN_READ_ONLY);
    if (!h->file) goto fail;

    ret = sqfs_super_read(&h->super, h->file);
    if (ret != 0) goto fail;

    sqfs_compressor_config_t cfg;
    sqfs_compressor_config_init(&cfg,
                                (SQFS_COMPRESSOR)h->super.compression_id,
                                h->super.block_size,
                                SQFS_COMP_FLAG_UNCOMPRESS);
    ret = sqfs_compressor_create(&cfg, &h->compressor);
    if (ret != 0) goto fail;

    h->dir_reader = sqfs_dir_reader_create(&h->super, h->compressor,
                                           h->file, 0);
    if (!h->dir_reader) goto fail;

    h->id_table = sqfs_id_table_create(0);
    if (!h->id_table) goto fail;

    ret = sqfs_id_table_read(h->id_table, h->file,
                             &h->super, h->compressor);
    if (ret != 0) goto fail;

    h->data_reader = sqfs_data_reader_create(h->file,
                                             h->super.block_size,
                                             h->compressor, 0);
    if (!h->data_reader) goto fail;

    ret = sqfs_data_reader_load_fragment_table(h->data_reader, &h->super);
    if (ret != 0) goto fail;

    return h;

fail:
    csqfs_close(h);
    return NULL;
}

void csqfs_close(void *handle) {
    csqfs_handle_internal *h = (csqfs_handle_internal *)handle;
    if (!h) return;
    if (h->tree)        sqfs_dir_tree_destroy(h->tree);
    if (h->data_reader) _destroy(h->data_reader);
    if (h->id_table)    _destroy(h->id_table);
    if (h->dir_reader)  _destroy(h->dir_reader);
    if (h->compressor)  _destroy(h->compressor);
    if (h->file)        _destroy(h->file);
    free(h);
}

const sqfs_super_t *csqfs_get_super(const void *handle) {
    return &((const csqfs_handle_internal *)handle)->super;
}

sqfs_tree_node_t *csqfs_get_tree(void *handle) {
    csqfs_handle_internal *h = (csqfs_handle_internal *)handle;
    if (h->tree) return h->tree;

    int ret = sqfs_dir_reader_get_full_hierarchy(
        h->dir_reader, h->id_table, NULL, 0, &h->tree
    );
    return (ret == 0) ? h->tree : NULL;
}

int csqfs_read_file(void *handle,
                    const sqfs_inode_generic_t *inode,
                    uint64_t offset,
                    void *buffer,
                    uint32_t size) {
    csqfs_handle_internal *h = (csqfs_handle_internal *)handle;
    return sqfs_data_reader_read(h->data_reader, inode, offset, buffer, size);
}

/* ── Tree node helpers ── */

const char *csqfs_tree_node_get_name(const sqfs_tree_node_t *node) {
    return (const char *)node->name;
}

/* ── Inode helpers ── */

uint64_t csqfs_inode_get_file_size_val(const sqfs_inode_generic_t *inode) {
    sqfs_u64 size = 0;
    if (sqfs_inode_get_file_size(inode, &size) != 0)
        return 0;
    return size;
}

const char *csqfs_inode_get_symlink_target(const sqfs_inode_generic_t *inode) {
    if (inode->base.type != SQFS_INODE_SLINK &&
        inode->base.type != SQFS_INODE_EXT_SLINK)
        return NULL;
    return (const char *)inode->extra;
}

uint32_t csqfs_inode_get_symlink_size(const sqfs_inode_generic_t *inode) {
    if (inode->base.type == SQFS_INODE_SLINK)
        return inode->data.slink.target_size;
    if (inode->base.type == SQFS_INODE_EXT_SLINK)
        return inode->data.slink_ext.target_size;
    return 0;
}
