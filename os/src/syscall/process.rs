//! Process management syscalls
use crate::task::{change_program_brk, exit_current_and_run_next, suspend_current_and_run_next};

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

/// task exits and submit an exit code
pub fn sys_exit(_exit_code: i32) -> ! {
    trace!("kernel: sys_exit");
    exit_current_and_run_next();
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel: sys_yield");
    suspend_current_and_run_next();
    0
}

/// Copy time fields through validated user mappings, including a page boundary.
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    let us = crate::timer::get_time_us();
    let fields = [us / 1_000_000, us % 1_000_000];
    let bytes = unsafe { core::slice::from_raw_parts(fields.as_ptr() as *const u8, 16) };
    let buffers = match crate::mm::checked_user_buffer(
        crate::task::current_user_token(),
        ts as *const u8,
        bytes.len(),
        true,
    ) {
        Some(buffers) => buffers,
        None => return -1,
    };
    let mut offset = 0;
    for buffer in buffers {
        let len = buffer.len();
        buffer.copy_from_slice(&bytes[offset..offset + len]);
        offset += len;
    }
    0
}

/// Inspect a user byte only when the mapping permits the requested access.
pub fn sys_trace(request: usize, id: usize, data: usize) -> isize {
    if request == 2 {
        return crate::task::syscall_count(id).map_or(-1, |count| count as isize);
    }
    if request > 1 {
        return -1;
    }
    match crate::mm::checked_user_buffer(
        crate::task::current_user_token(),
        id as *const u8,
        1,
        request == 1,
    ) {
        Some(mut buffers) => {
            if request == 0 {
                buffers[0][0] as isize
            } else {
                buffers[0][0] = data as u8;
                0
            }
        }
        None => -1,
    }
}

/// Create an anonymous mapping in the caller's address space.
pub fn sys_mmap(start: usize, len: usize, prot: usize) -> isize {
    crate::task::with_current_task(|task| task.memory_set.mmap(start, len, prot))
}

/// Remove a mapping from the caller's address space.
pub fn sys_munmap(start: usize, len: usize) -> isize {
    crate::task::with_current_task(|task| task.memory_set.munmap(start, len))
}
/// change data segment size
pub fn sys_sbrk(size: i32) -> isize {
    trace!("kernel: sys_sbrk");
    if let Some(old_brk) = change_program_brk(size) {
        old_brk as isize
    } else {
        -1
    }
}
