//! Double-buffered shared memory with seqlock for torn-read prevention.

use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicU64, Ordering};
use std::ptr;

use crate::context_window::ContextWindow;

/// Double-buffered shared state with seqlock consistency.
///
/// The atomic `seq` encodes both state and active buffer index:
/// - Bit 0: 0 = stable, 1 = writing in progress
/// - Bit 1: active buffer index (0 or 1)
/// - Bits 2+: monotonic counter for ABA detection
#[repr(C)]
pub struct SharedStateBuffer {
    seq: AtomicU64,
    buffers: UnsafeCell<[ContextWindow; 2]>,
}

// SAFETY: Write side uses seqlock protocol. Only one writer assumed.
unsafe impl Sync for SharedStateBuffer {}

impl SharedStateBuffer {
    pub fn new() -> Self {
        Self {
            seq: AtomicU64::new(0),
            buffers: UnsafeCell::new([ContextWindow::zeroed(), ContextWindow::zeroed()]),
        }
    }

    /// Write a new context window (engine side).
    ///
    /// Protocol:
    /// 1. Compute next active buffer (flip bit 1)
    /// 2. Set writing bit (bit 0) → odd
    /// 3. Write to target buffer
    /// 4. Clear writing bit → even (stable)
    pub fn write(&self, ctx: &ContextWindow) {
        let current = self.seq.load(Ordering::Relaxed);
        // Flip active buffer index (bit 1), set writing bit (bit 0)
        let next = ((current >> 1) + 1) << 1 | 1;
        self.seq.store(next, Ordering::Release);

        let target = ((next >> 1) & 1) as usize;
        unsafe {
            let buffers = &mut *self.buffers.get();
            ptr::copy_nonoverlapping(ctx as *const ContextWindow, &mut buffers[target], 1);
        }

        // Clear writing bit (bit 0) → even
        self.seq.store(next & !1, Ordering::Release);
    }

    /// Read the latest context window (agent side, lock-free).
    pub fn read(&self) -> Option<ContextWindow> {
        for _ in 0..100 {
            let s = self.seq.load(Ordering::Acquire);

            // Odd = writing in progress
            if s & 1 != 0 {
                std::hint::spin_loop();
                continue;
            }

            let active = ((s >> 1) & 1) as usize;
            let mut ctx = ContextWindow::zeroed();

            unsafe {
                let buffers = &*self.buffers.get();
                ptr::copy_nonoverlapping(&buffers[active], &mut ctx, 1);
            }

            // Verify no write happened during read
            let s2 = self.seq.load(Ordering::Acquire);
            if s == s2 {
                return Some(ctx);
            }
        }
        None
    }

    pub fn size_in_bytes() -> usize {
        std::mem::size_of::<Self>()
    }
}

impl Default for SharedStateBuffer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_write_read_consistency() {
        let buf = SharedStateBuffer::new();

        let mut ctx = ContextWindow::zeroed();
        ctx.version = 42;
        ctx.set_instrument_id("ETH-USDC");
        ctx.position_size = 10.5;

        buf.write(&ctx);

        let read = buf.read().unwrap();
        assert_eq!(read.version, 42);
        assert_eq!(read.instrument_id_str(), "ETH-USDC");
        assert_eq!(read.position_size, 10.5);
    }

    #[test]
    fn test_overwrite() {
        let buf = SharedStateBuffer::new();

        for i in 0..100u64 {
            let mut ctx = ContextWindow::zeroed();
            ctx.version = i;
            buf.write(&ctx);
        }

        let read = buf.read().unwrap();
        assert_eq!(read.version, 99);
    }
}
