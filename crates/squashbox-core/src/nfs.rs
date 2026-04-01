use crate::provider::VirtualFsProvider;
use crate::types::{EntryAttributes, EntryType};
use async_trait::async_trait;
use nfsserve::nfs::*;
use nfsserve::vfs::{DirEntry, NFSFileSystem, ReadDirResult, VFSCapabilities};
use std::sync::Arc;

pub struct NfsFsWrapper<T: VirtualFsProvider + ?Sized> {
    pub provider: Arc<T>,
}

impl<T: VirtualFsProvider + ?Sized> NfsFsWrapper<T> {
    pub fn new(provider: Arc<T>) -> Self {
        Self { provider }
    }

    fn map_attr(&self, attr: &EntryAttributes, inode: u64) -> fattr3 {
        let ftype = match attr.entry_type {
            EntryType::Directory => ftype3::NF3DIR,
            EntryType::File => ftype3::NF3REG,
            EntryType::Symlink => ftype3::NF3LNK,
            EntryType::CharDevice => ftype3::NF3CHR,
            EntryType::BlockDevice => ftype3::NF3BLK,
        };

        fattr3 {
            ftype,
            mode: attr.mode,
            nlink: attr.nlink,
            uid: attr.uid,
            gid: attr.gid,
            size: attr.size,
            used: attr.size,
            rdev: specdata3 { specdata1: 0, specdata2: 0 },
            fsid: 1,
            fileid: inode,
            atime: nfstime3 { seconds: attr.mtime_secs as u32, nseconds: 0 },
            mtime: nfstime3 { seconds: attr.mtime_secs as u32, nseconds: 0 },
            ctime: nfstime3 { seconds: attr.mtime_secs as u32, nseconds: 0 },
        }
    }
}

#[async_trait]
impl<T: VirtualFsProvider + ?Sized + 'static> NFSFileSystem for NfsFsWrapper<T> {
    fn capabilities(&self) -> VFSCapabilities {
        VFSCapabilities::ReadOnly
    }

    fn root_dir(&self) -> fileid3 {
        1 
    }

    async fn lookup(&self, dirid: fileid3, filename: &filename3) -> Result<fileid3, nfsstat3> {
        let name_str = String::from_utf8_lossy(filename).to_string();
        
        let entry_opt = self.provider.lookup(dirid, &name_str)
            .map_err(|_| nfsstat3::NFS3ERR_IO)?;
            
        if let Some(entry) = entry_opt {
            Ok(entry.attributes.inode)
        } else {
            Err(nfsstat3::NFS3ERR_NOENT)
        }
    }

    async fn getattr(&self, id: fileid3) -> Result<fattr3, nfsstat3> {
        let attr = self.provider.get_attributes(id)
            .map_err(|_| nfsstat3::NFS3ERR_NOENT)?;
        Ok(self.map_attr(&attr, id))
    }

    async fn setattr(&self, _id: fileid3, _setattr: sattr3) -> Result<fattr3, nfsstat3> {
        Err(nfsstat3::NFS3ERR_ROFS)
    }

    async fn read(&self, id: fileid3, offset: u64, count: u32) -> Result<(Vec<u8>, bool), nfsstat3> {
        let data = self.provider.read_file(id, offset, count as u64)
            .map_err(|_| nfsstat3::NFS3ERR_IO)?;
        
        let eof = data.len() < count as usize;
        
        Ok((data, eof))
    }

    async fn write(&self, _id: fileid3, _offset: u64, _data: &[u8]) -> Result<fattr3, nfsstat3> {
        Err(nfsstat3::NFS3ERR_ROFS)
    }

    async fn create(&self, _dirid: fileid3, _filename: &filename3, _attr: sattr3) -> Result<(fileid3, fattr3), nfsstat3> {
        Err(nfsstat3::NFS3ERR_ROFS)
    }

    async fn create_exclusive(&self, _dirid: fileid3, _filename: &filename3) -> Result<fileid3, nfsstat3> {
        Err(nfsstat3::NFS3ERR_ROFS)
    }

    async fn mkdir(&self, _dirid: fileid3, _dirname: &filename3) -> Result<(fileid3, fattr3), nfsstat3> {
        Err(nfsstat3::NFS3ERR_ROFS)
    }

    async fn remove(&self, _dirid: fileid3, _filename: &filename3) -> Result<(), nfsstat3> {
        Err(nfsstat3::NFS3ERR_ROFS)
    }

    async fn rename(&self, _from_dirid: fileid3, _from_filename: &filename3, _to_dirid: fileid3, _to_filename: &filename3) -> Result<(), nfsstat3> {
        Err(nfsstat3::NFS3ERR_ROFS)
    }

    async fn readdir(&self, dirid: fileid3, start_after: fileid3, max_entries: usize) -> Result<ReadDirResult, nfsstat3> {
        let mut all_entries = Vec::new();
        let mut cookie = 0;
        
        loop {
            let batch = self.provider.list_directory(dirid, cookie)
                .map_err(|_| nfsstat3::NFS3ERR_IO)?;
            
            for entry in batch.entries {
                all_entries.push(entry);
            }
            cookie = batch.next_cookie;
            if cookie == 0 {
                break;
            }
        }
        
        let mut entries = Vec::new();
        let mut started = start_after == 0;
        
        for e in all_entries.iter() {
            if !started {
                if e.attributes.inode == start_after {
                    started = true;
                }
                continue;
            }
            
            let inode = e.attributes.inode;
            let attr = &e.attributes;
            
            entries.push(DirEntry {
                fileid: inode,
                name: nfsstring(e.name.clone().into_bytes()),
                attr: self.map_attr(attr, inode),
            });
            
            if entries.len() >= max_entries {
                break;
            }
        }
        
        let end = entries.is_empty() || entries.last().unwrap().fileid == all_entries.last().unwrap().attributes.inode;
        
        Ok(ReadDirResult { entries, end })
    }

    async fn symlink(&self, _dirid: fileid3, _linkname: &filename3, _symlink: &nfspath3, _attr: &sattr3) -> Result<(fileid3, fattr3), nfsstat3> {
        Err(nfsstat3::NFS3ERR_ROFS)
    }

    async fn readlink(&self, id: fileid3) -> Result<nfspath3, nfsstat3> {
        let target = self.provider.read_symlink(id)
            .map_err(|_| nfsstat3::NFS3ERR_NOENT)?;
        Ok(nfsstring(target.into_bytes()))
    }
}

use std::path::Path;
use nfsserve::tcp::{NFSTcp, NFSTcpListener};

/// Spawns the NFS server and mounts it locally.
pub async fn mount_and_serve_nfs<T: VirtualFsProvider + ?Sized + 'static>(
    provider: Arc<T>,
    mount_point: &Path,
) -> anyhow::Result<()> {
    if !mount_point.exists() {
        anyhow::bail!("Mount point '{}' does not exist. Please create the directory first.", mount_point.display());
    }
    if !mount_point.is_dir() {
        anyhow::bail!("Mount point '{}' is not a directory.", mount_point.display());
    }

    let fs = NfsFsWrapper::new(provider);
    let listener = NFSTcpListener::bind("127.0.0.1:0", fs).await?;
    let port = listener.get_listen_port();

    log::info!("NFS Server listening on 127.0.0.1:{}", port);

    let server_handle = tokio::spawn(async move {
        let _ = listener.handle_forever().await;
    });

    // Yield back to the tokio scheduler so the server task can actually begin
    // listening before we block the current thread with a synchronous OS command.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let mount_point_str = mount_point.to_string_lossy();
    
    #[cfg(target_os = "macos")]
    {
        log::info!("Executing native mount: mount -t nfs -o port={},mountport={},tcp,nolocks,locallocks,nfc nfs://localhost/ {}", port, port, mount_point_str);
        
        let status = tokio::process::Command::new("mount")
            .args([
                "-t", "nfs",
                "-o", &format!("port={},mountport={},tcp,nolocks,locallocks,nfc", port, port),
                "localhost:/",
                mount_point_str.as_ref()
            ])
            .status()
            .await?;
            
        if !status.success() {
            anyhow::bail!("Failed to mount NFS natively on macOS. Ensure the mount point is not already in use and you have proper permissions.");
        }
    }
    
    #[cfg(target_os = "linux")]
    {
        let status = tokio::process::Command::new("mount")
            .args([
                "-t", "nfs",
                "-o", &format!("port={},nolock,vers=3,tcp,mountport={}", port, port),
                "localhost:/",
                mount_point_str.as_ref()
            ])
            .status()
            .await?;
            
        if !status.success() {
            anyhow::bail!("Failed to mount NFS natively on Linux");
        }
    }
    
    #[cfg(target_os = "windows")]
    {
        log::info!("Executing native mount: mount -o mtype=hard,nolock localhost:/ {}", mount_point_str);
        let status = tokio::process::Command::new("mount")
            .args([
                "-o", "mtype=hard,nolock",
                "localhost:/",
                mount_point_str.as_ref()
            ])
            .status()
            .await?;
            
        if !status.success() {
            anyhow::bail!("Failed to mount NFS natively on Windows. Please ensure 'Client for NFS' is enabled in Windows Features.");
        }
    }

    println!("Mounted natively via NFS on {}. Press Ctrl+C to unmount.", mount_point_str);

    tokio::signal::ctrl_c().await?;
    println!("\nUnmounting...");

    let _ = tokio::process::Command::new("umount").arg(mount_point_str.as_ref()).status().await;

    server_handle.abort();

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{CoreError, CoreResult, DirEntry, DirEntryBatch, ROOT_INODE, VolumeStats};
    use std::path::Path;

    struct TestProvider;

    impl VirtualFsProvider for TestProvider {
        fn resolve_path(&self, _path: &Path) -> CoreResult<Option<u64>> { Ok(Some(ROOT_INODE)) }
        fn get_attributes(&self, inode: u64) -> CoreResult<EntryAttributes> {
            match inode {
                ROOT_INODE => Ok(EntryAttributes {
                    inode: ROOT_INODE, entry_type: EntryType::Directory,
                    size: 0, mode: 0o755, uid: 100, gid: 200, mtime_secs: 1000, nlink: 2,
                }),
                10 => Ok(EntryAttributes {
                    inode: 10, entry_type: EntryType::File,
                    size: 5, mode: 0o644, uid: 100, gid: 200, mtime_secs: 1000, nlink: 1,
                }),
                _ => Err(CoreError::NotFound("inode".into())),
            }
        }
        fn list_directory(&self, inode: u64, cookie: u64) -> CoreResult<DirEntryBatch> {
            if inode == ROOT_INODE && cookie == 0 {
                Ok(DirEntryBatch {
                    entries: vec![DirEntry { name: "test.txt".into(), attributes: self.get_attributes(10)? }],
                    next_cookie: 0,
                })
            } else {
                Ok(DirEntryBatch { entries: vec![], next_cookie: 0 })
            }
        }
        fn lookup(&self, parent: u64, name: &str) -> CoreResult<Option<DirEntry>> {
            if parent == ROOT_INODE && name == "test.txt" {
                Ok(Some(DirEntry { name: name.into(), attributes: self.get_attributes(10)? }))
            } else {
                Ok(None)
            }
        }
        fn read_file(&self, inode: u64, offset: u64, length: u64) -> CoreResult<Vec<u8>> {
            if inode == 10 {
                let data = b"hello";
                let start = (offset as usize).min(data.len());
                let end = (start + length as usize).min(data.len());
                Ok(data[start..end].to_vec())
            } else {
                Err(CoreError::NotFound("file".into()))
            }
        }
        fn read_symlink(&self, _inode: u64) -> CoreResult<String> { Err(CoreError::NotASymlink(0)) }
        fn list_xattrs(&self, _inode: u64) -> CoreResult<Vec<String>> { Ok(vec![]) }
        fn get_xattr(&self, _inode: u64, _name: &str) -> CoreResult<Vec<u8>> { Err(CoreError::NotFound("xattr".into())) }
        fn check_access(&self, _inode: u64, _mask: u32) -> CoreResult<bool> { Ok(true) }
        fn volume_stats(&self) -> CoreResult<VolumeStats> {
            Ok(VolumeStats { total_bytes: 0, used_bytes: 0, total_inodes: 0, used_inodes: 0, block_size: 0 })
        }
    }

    #[tokio::test]
    async fn test_nfs_wrapper_capabilities() {
        let wrapper = NfsFsWrapper::new(Arc::new(TestProvider));
        assert!(matches!(wrapper.capabilities(), VFSCapabilities::ReadOnly));
        assert_eq!(wrapper.root_dir(), ROOT_INODE);
    }

    #[tokio::test]
    async fn test_nfs_wrapper_lookup() {
        let wrapper = NfsFsWrapper::new(Arc::new(TestProvider));
        let test_name = nfsstring("test.txt".as_bytes().to_vec());
        let missing_name = nfsstring("missing.txt".as_bytes().to_vec());
        
        let found = wrapper.lookup(ROOT_INODE, &test_name).await.unwrap();
        assert_eq!(found, 10);
        
        let err = wrapper.lookup(ROOT_INODE, &missing_name).await.unwrap_err();
        assert!(matches!(err, nfsstat3::NFS3ERR_NOENT));
    }

    #[tokio::test]
    async fn test_nfs_wrapper_getattr() {
        let wrapper = NfsFsWrapper::new(Arc::new(TestProvider));
        
        let root_attr = wrapper.getattr(ROOT_INODE).await.unwrap();
        assert!(matches!(root_attr.ftype, ftype3::NF3DIR));
        assert_eq!(root_attr.fileid, ROOT_INODE);
        
        let file_attr = wrapper.getattr(10).await.unwrap();
        assert!(matches!(file_attr.ftype, ftype3::NF3REG));
        assert_eq!(file_attr.size, 5);
        assert_eq!(file_attr.fileid, 10);
    }

    #[tokio::test]
    async fn test_nfs_wrapper_read() {
        let wrapper = NfsFsWrapper::new(Arc::new(TestProvider));
        
        let (data, eof) = wrapper.read(10, 0, 100).await.unwrap();
        assert_eq!(data, b"hello");
        assert!(eof);
        
        let (data_part, eof_part) = wrapper.read(10, 1, 2).await.unwrap();
        assert_eq!(data_part, b"el");
        assert!(!eof_part); // 2 bytes requested, length of data > 2 so technically false but nfsserve boolean logic is `data.len() < count`
    }

    #[tokio::test]
    async fn test_nfs_wrapper_readdir() {
        let wrapper = NfsFsWrapper::new(Arc::new(TestProvider));
        
        let result = wrapper.readdir(ROOT_INODE, 0, 100).await.unwrap();
        assert_eq!(result.entries.len(), 1);
        assert_eq!(result.entries[0].fileid, 10);
        assert_eq!(result.entries[0].name.0, b"test.txt");
        assert!(result.end);
    }
}
