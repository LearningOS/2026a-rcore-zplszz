//! Per-process resource accounting for the deadlock reduction algorithm.

use alloc::{vec, vec::Vec};

/// Independent resource family with per-thread holdings and pending requests.
#[derive(Default)]
pub struct DeadlockDetector {
    available: Vec<usize>,
    allocation: Vec<Vec<usize>>,
    need: Vec<Vec<usize>>,
}

impl DeadlockDetector {
    /// Register a new resource slot with its initial capacity.
    pub fn add_resource(&mut self, id: usize, count: usize) {
        if id == self.available.len() {
            self.available.push(count);
            for row in self.allocation.iter_mut().chain(self.need.iter_mut()) {
                row.push(0);
            }
        } else {
            self.available[id] = count;
            for row in self.allocation.iter_mut().chain(self.need.iter_mut()) {
                row[id] = 0;
            }
        }
    }

    fn ensure_thread(&mut self, tid: usize) {
        while self.allocation.len() <= tid {
            self.allocation.push(vec![0; self.available.len()]);
            self.need.push(vec![0; self.available.len()]);
        }
    }

    /// Tentatively record a request and roll it back when reduction fails.
    pub fn request(&mut self, tid: usize, id: usize, enabled: bool) -> bool {
        self.ensure_thread(tid);
        self.need[tid][id] += 1;
        if enabled && !self.is_safe() {
            self.need[tid][id] -= 1;
            false
        } else {
            true
        }
    }

    /// Commit an acquisition after the synchronization primitive returns.
    pub fn acquire(&mut self, tid: usize, id: usize) {
        self.ensure_thread(tid);
        self.available[id] -= 1;
        self.need[tid][id] = self.need[tid][id].saturating_sub(1);
        self.allocation[tid][id] += 1;
    }

    /// Return one unit, allowing semaphore signalling by non-holders.
    pub fn release(&mut self, tid: usize, id: usize) {
        self.ensure_thread(tid);
        // A semaphore may be signalled by a thread that never acquired it.
        self.allocation[tid][id] = self.allocation[tid][id].saturating_sub(1);
        self.available[id] += 1;
    }

    /// Forget a retired TID without releasing abandoned resource units.
    pub fn retire_thread(&mut self, tid: usize) {
        if tid < self.allocation.len() {
            // Exiting does not unlock resources: leaked holdings stay unavailable.
            self.allocation[tid].fill(0);
            self.need[tid].fill(0);
        }
    }

    fn is_safe(&self) -> bool {
        let mut work = self.available.clone();
        let mut finish = vec![false; self.need.len()];
        loop {
            let mut progress = false;
            for tid in 0..finish.len() {
                if !finish[tid] && self.need[tid].iter().zip(&work).all(|(n, w)| n <= w) {
                    for (w, a) in work.iter_mut().zip(&self.allocation[tid]) {
                        *w += a;
                    }
                    finish[tid] = true;
                    progress = true;
                }
            }
            if !progress {
                return finish.iter().all(|done| *done);
            }
        }
    }
}
