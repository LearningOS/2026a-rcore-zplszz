//! File and filesystem-related syscalls
use crate::fs::{make_pipe, open_file, OpenFlags, Stat};
use crate::mm::{
    checked_user_buffer, translated_byte_buffer, translated_refmut, translated_str, UserBuffer,
};
use crate::task::{current_task, current_user_token};
use alloc::sync::Arc;

pub fn sys_write(fd: usize, buf: *const u8, len: usize) -> isize {
    trace!("kernel:pid[{}] sys_write", current_task().unwrap().pid.0);
    let token = current_user_token();
    let task = current_task().unwrap();
    let inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if let Some(file) = &inner.fd_table[fd] {
        if !file.writable() {
            return -1;
        }
        let file = file.clone();
        // release current task TCB manually to avoid multi-borrow
        drop(inner);
        file.write(UserBuffer::new(translated_byte_buffer(token, buf, len))) as isize
    } else {
        -1
    }
}

pub fn sys_read(fd: usize, buf: *const u8, len: usize) -> isize {
    trace!("kernel:pid[{}] sys_read", current_task().unwrap().pid.0);
    let token = current_user_token();
    let task = current_task().unwrap();
    let inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if let Some(file) = &inner.fd_table[fd] {
        let file = file.clone();
        if !file.readable() {
            return -1;
        }
        // release current task TCB manually to avoid multi-borrow
        drop(inner);
        trace!("kernel: sys_read .. file.read");
        file.read(UserBuffer::new(translated_byte_buffer(token, buf, len))) as isize
    } else {
        -1
    }
}

pub fn sys_open(path: *const u8, flags: u32) -> isize {
    trace!("kernel:pid[{}] sys_open", current_task().unwrap().pid.0);
    let task = current_task().unwrap();
    let token = current_user_token();
    let path = translated_str(token, path);
    let flags = match OpenFlags::from_bits(flags) {
        Some(flags) => flags,
        None => return -1,
    };
    if let Some(inode) = open_file(path.as_str(), flags) {
        let mut inner = task.inner_exclusive_access();
        let fd = inner.alloc_fd();
        inner.fd_table[fd] = Some(inode);
        fd as isize
    } else {
        -1
    }
}

pub fn sys_close(fd: usize) -> isize {
    trace!("kernel:pid[{}] sys_close", current_task().unwrap().pid.0);
    let task = current_task().unwrap();
    let mut inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if inner.fd_table[fd].is_none() {
        return -1;
    }
    inner.fd_table[fd].take();
    0
}

pub fn sys_pipe(pipe: *mut usize) -> isize {
    trace!("kernel:pid[{}] sys_pipe", current_task().unwrap().pid.0);
    let task = current_task().unwrap();
    let token = current_user_token();
    let mut inner = task.inner_exclusive_access();
    let (pipe_read, pipe_write) = make_pipe();
    let read_fd = inner.alloc_fd();
    inner.fd_table[read_fd] = Some(pipe_read);
    let write_fd = inner.alloc_fd();
    inner.fd_table[write_fd] = Some(pipe_write);
    *translated_refmut(token, pipe) = read_fd;
    *translated_refmut(token, unsafe { pipe.add(1) }) = write_fd;
    0
}

pub fn sys_dup(fd: usize) -> isize {
    trace!("kernel:pid[{}] sys_dup", current_task().unwrap().pid.0);
    let task = current_task().unwrap();
    let mut inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if inner.fd_table[fd].is_none() {
        return -1;
    }
    let new_fd = inner.alloc_fd();
    inner.fd_table[new_fd] = Some(Arc::clone(inner.fd_table[fd].as_ref().unwrap()));
    new_fd as isize
}

/// Copy inode metadata back to a validated, possibly cross-page user buffer.
pub fn sys_fstat(fd: usize, st: *mut Stat) -> isize {
    let task = current_task().unwrap();
    let inner = task.inner_exclusive_access();
    let stat = match inner
        .fd_table
        .get(fd)
        .and_then(|file| file.as_ref())
        .and_then(|file| file.stat())
    {
        Some(stat) => stat,
        None => return -1,
    };
    let buffers = match checked_user_buffer(
        inner.memory_set.token(),
        st as *const u8,
        core::mem::size_of::<Stat>(),
        true,
    ) {
        Some(buffers) => buffers,
        None => return -1,
    };
    let bytes = unsafe {
        core::slice::from_raw_parts(
            &stat as *const Stat as *const u8,
            core::mem::size_of::<Stat>(),
        )
    };
    let mut offset = 0;
    for buffer in buffers {
        let end = offset + buffer.len();
        buffer.copy_from_slice(&bytes[offset..end]);
        offset = end;
    }
    0
}

pub fn sys_linkat(old_name: *const u8, new_name: *const u8) -> isize {
    let token = current_user_token();
    let old = translated_str(token, old_name);
    let new = translated_str(token, new_name);
    if crate::fs::ROOT_INODE.link(&old, &new) {
        0
    } else {
        -1
    }
}

pub fn sys_unlinkat(name: *const u8) -> isize {
    let name = translated_str(current_user_token(), name);
    if crate::fs::ROOT_INODE.unlink(&name) {
        0
    } else {
        -1
    }
}
