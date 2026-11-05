use super::{
    block_cache_sync_all, get_block_cache, BlockDevice, DirEntry, DiskInode, DiskInodeType,
    EasyFileSystem, DIRENT_SZ,
};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::{Mutex, MutexGuard};
/// Virtual filesystem layer over easy-fs
pub struct Inode {
    block_id: usize,
    inode_id: u32,
    block_offset: usize,
    fs: Arc<Mutex<EasyFileSystem>>,
    block_device: Arc<dyn BlockDevice>,
}

impl Drop for Inode {
    fn drop(&mut self) {
        let mut fs = self.fs.lock();
        let count = fs.open_inodes.get_mut(&self.inode_id).unwrap();
        *count -= 1;
        if *count == 0 {
            fs.open_inodes.remove(&self.inode_id);
            if self.read_disk_inode(|inode| inode.nlink == 0) {
                fs.reclaim_inode(self.inode_id);
                block_cache_sync_all();
            }
        }
    }
}

impl Inode {
    /// Create a vfs inode
    pub fn new(
        block_id: u32,
        block_offset: usize,
        fs: Arc<Mutex<EasyFileSystem>>,
        block_device: Arc<dyn BlockDevice>,
        fs_inner: &mut EasyFileSystem,
    ) -> Self {
        let inode_id = fs_inner.get_disk_inode_id(block_id, block_offset);
        *fs_inner.open_inodes.entry(inode_id).or_insert(0) += 1;
        Self {
            inode_id,
            block_id: block_id as usize,
            block_offset,
            fs,
            block_device,
        }
    }
    /// Call a function over a disk inode to read it
    fn read_disk_inode<V>(&self, f: impl FnOnce(&DiskInode) -> V) -> V {
        get_block_cache(self.block_id, Arc::clone(&self.block_device))
            .lock()
            .read(self.block_offset, f)
    }
    /// Call a function over a disk inode to modify it
    fn modify_disk_inode<V>(&self, f: impl FnOnce(&mut DiskInode) -> V) -> V {
        get_block_cache(self.block_id, Arc::clone(&self.block_device))
            .lock()
            .modify(self.block_offset, f)
    }
    /// Find inode under a disk inode by name
    fn find_inode_id(&self, name: &str, disk_inode: &DiskInode) -> Option<u32> {
        // assert it is a directory
        assert!(disk_inode.is_dir());
        let file_count = (disk_inode.size as usize) / DIRENT_SZ;
        let mut dirent = DirEntry::empty();
        for i in 0..file_count {
            assert_eq!(
                disk_inode.read_at(DIRENT_SZ * i, dirent.as_bytes_mut(), &self.block_device,),
                DIRENT_SZ,
            );
            if !name.is_empty() && dirent.name() == name {
                return Some(dirent.inode_id() as u32);
            }
        }
        None
    }
    fn vacant_dirent(&self, root: &DiskInode) -> usize {
        let mut entry = DirEntry::empty();
        for offset in (0..root.size as usize).step_by(DIRENT_SZ) {
            root.read_at(offset, entry.as_bytes_mut(), &self.block_device);
            if entry.name().is_empty() {
                return offset;
            }
        }
        root.size as usize
    }

    /// Find inode under current inode by name
    pub fn find(&self, name: &str) -> Option<Arc<Inode>> {
        let mut fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| {
            self.find_inode_id(name, disk_inode).map(|inode_id| {
                let (block_id, block_offset) = fs.get_disk_inode_pos(inode_id);
                Arc::new(Self::new(
                    block_id,
                    block_offset,
                    self.fs.clone(),
                    self.block_device.clone(),
                    &mut fs,
                ))
            })
        })
    }
    /// Increase the size of a disk inode
    fn increase_size(
        &self,
        new_size: u32,
        disk_inode: &mut DiskInode,
        fs: &mut MutexGuard<EasyFileSystem>,
    ) {
        if new_size < disk_inode.size {
            return;
        }
        let blocks_needed = disk_inode.blocks_num_needed(new_size);
        let mut v: Vec<u32> = Vec::new();
        for _ in 0..blocks_needed {
            v.push(fs.alloc_data());
        }
        disk_inode.increase_size(new_size, v, &self.block_device);
    }
    /// Create inode under current inode by name
    pub fn create(&self, name: &str) -> Option<Arc<Inode>> {
        if name.is_empty() || name.len() > 27 || name.contains('/') {
            return None;
        }
        let mut fs = self.fs.lock();
        let op = |root_inode: &DiskInode| {
            // assert it is a directory
            assert!(root_inode.is_dir());
            // has the file been created?
            self.find_inode_id(name, root_inode)
        };
        if self.read_disk_inode(op).is_some() {
            return None;
        }
        // create a new file
        // alloc a inode with an indirect block
        let new_inode_id = fs.alloc_inode();
        // initialize inode
        let (new_inode_block_id, new_inode_block_offset) = fs.get_disk_inode_pos(new_inode_id);
        get_block_cache(new_inode_block_id as usize, Arc::clone(&self.block_device))
            .lock()
            .modify(new_inode_block_offset, |new_inode: &mut DiskInode| {
                new_inode.initialize(DiskInodeType::File);
            });
        self.modify_disk_inode(|root_inode| {
            // append file in the dirent
            let offset = self.vacant_dirent(root_inode);
            let new_size = (offset + DIRENT_SZ).max(root_inode.size as usize);
            // increase size
            self.increase_size(new_size as u32, root_inode, &mut fs);
            // write dirent
            let dirent = DirEntry::new(name, new_inode_id);
            root_inode.write_at(offset, dirent.as_bytes(), &self.block_device);
        });

        let (block_id, block_offset) = fs.get_disk_inode_pos(new_inode_id);
        block_cache_sync_all();
        // return inode
        Some(Arc::new(Self::new(
            block_id,
            block_offset,
            self.fs.clone(),
            self.block_device.clone(),
            &mut fs,
        )))
        // release efs lock automatically by compiler
    }
    /// Stable identity, file type and persistent hard-link count.
    pub fn stat(&self) -> (u64, bool, u32) {
        let _fs = self.fs.lock();
        self.read_disk_inode(|inode| (self.inode_id as u64, inode.is_dir(), inode.nlink))
    }

    /// Add a name for an existing inode; no file data is copied.
    pub fn link(&self, old: &str, new: &str) -> bool {
        if old == new || new.is_empty() || new.len() > 27 || new.contains('/') {
            return false;
        }
        let mut fs = self.fs.lock();
        let id = self.read_disk_inode(|root| {
            if self.find_inode_id(new, root).is_some() {
                None
            } else {
                self.find_inode_id(old, root)
            }
        });
        let id = match id {
            Some(id) => id,
            None => return false,
        };
        let (block, offset) = fs.get_disk_inode_pos(id);
        let linked = get_block_cache(block as usize, self.block_device.clone())
            .lock()
            .modify(offset, |inode: &mut DiskInode| {
                if !inode.is_file() || inode.nlink == u32::MAX {
                    return false;
                }
                inode.nlink += 1;
                true
            });
        if !linked {
            return false;
        }
        self.modify_disk_inode(|root| {
            let offset = self.vacant_dirent(root);
            self.increase_size(
                (offset + DIRENT_SZ).max(root.size as usize) as u32,
                root,
                &mut fs,
            );
            root.write_at(
                offset,
                DirEntry::new(new, id).as_bytes(),
                &self.block_device,
            );
        });
        block_cache_sync_all();
        true
    }

    /// Remove a directory entry; open descriptors keep an unlinked inode alive.
    pub fn unlink(&self, name: &str) -> bool {
        if name.is_empty() {
            return false;
        }
        let mut fs = self.fs.lock();
        let id = self.modify_disk_inode(|root| {
            let count = root.size as usize / DIRENT_SZ;
            let mut entry = DirEntry::empty();
            for index in 0..count {
                root.read_at(index * DIRENT_SZ, entry.as_bytes_mut(), &self.block_device);
                if entry.name() != name {
                    continue;
                }
                let id = entry.inode_id();
                root.write_at(
                    index * DIRENT_SZ,
                    DirEntry::empty().as_bytes(),
                    &self.block_device,
                );
                return Some(id);
            }
            None
        });
        let id = match id {
            Some(id) => id,
            None => return false,
        };
        let (block, offset) = fs.get_disk_inode_pos(id);
        let links = get_block_cache(block as usize, self.block_device.clone())
            .lock()
            .modify(offset, |inode: &mut DiskInode| {
                inode.nlink -= 1;
                inode.nlink
            });
        if links == 0 && !fs.open_inodes.contains_key(&id) {
            fs.reclaim_inode(id);
        }
        block_cache_sync_all();
        true
    }

    /// List inodes under current inode
    pub fn ls(&self) -> Vec<String> {
        let _fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| {
            let file_count = (disk_inode.size as usize) / DIRENT_SZ;
            let mut v: Vec<String> = Vec::new();
            for i in 0..file_count {
                let mut dirent = DirEntry::empty();
                assert_eq!(
                    disk_inode.read_at(i * DIRENT_SZ, dirent.as_bytes_mut(), &self.block_device,),
                    DIRENT_SZ,
                );
                if !dirent.name().is_empty() {
                    v.push(String::from(dirent.name()));
                }
            }
            v
        })
    }
    /// Read data from current inode
    pub fn read_at(&self, offset: usize, buf: &mut [u8]) -> usize {
        let _fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| disk_inode.read_at(offset, buf, &self.block_device))
    }
    /// Write data to current inode
    pub fn write_at(&self, offset: usize, buf: &[u8]) -> usize {
        let mut fs = self.fs.lock();
        let size = self.modify_disk_inode(|disk_inode| {
            self.increase_size((offset + buf.len()) as u32, disk_inode, &mut fs);
            disk_inode.write_at(offset, buf, &self.block_device)
        });
        block_cache_sync_all();
        size
    }
    /// Clear the data in current inode
    pub fn clear(&self) {
        let mut fs = self.fs.lock();
        self.modify_disk_inode(|disk_inode| {
            let size = disk_inode.size;
            let data_blocks_dealloc = disk_inode.clear_size(&self.block_device);
            assert!(data_blocks_dealloc.len() == DiskInode::total_blocks(size) as usize);
            for data_block in data_blocks_dealloc.into_iter() {
                fs.dealloc_data(data_block);
            }
        });
        block_cache_sync_all();
    }
}
