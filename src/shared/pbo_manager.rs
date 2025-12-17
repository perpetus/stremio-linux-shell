use std::cell::UnsafeCell;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering},
};

/// Number of buffers in the pool
const BUFFER_COUNT: usize = 3;

/// A single buffer entry in the pool
pub struct PoolEntry {
    /// The actual buffer data (wrapped in Arc for sharing)
    /// We use UnsafeCell to allow mutation when we genuinely own the buffer (refcount == 1)
    pub data: UnsafeCell<Arc<Vec<u8>>>,
    /// The width of the current frame in this buffer
    pub width: AtomicI32,
    /// The height of the current frame in this buffer  
    pub height: AtomicI32,
}

// Safety: We ensure exclusive access via synchronization or refcount checks
unsafe impl Send for PoolEntry {}
unsafe impl Sync for PoolEntry {}

impl Default for PoolEntry {
    fn default() -> Self {
        Self {
            data: UnsafeCell::new(Arc::new(Vec::new())),
            width: AtomicI32::new(0),
            height: AtomicI32::new(0),
        }
    }
}

/// Buffer pool manager that reuses pre-allocated buffers to avoid per-frame allocation
/// Optimized for zero-copy sharing via Arc
pub struct BufferPool {
    /// The pool of reusable buffers
    pub buffers: [PoolEntry; BUFFER_COUNT],
    /// Current write index (round-robin)
    write_index: AtomicUsize,
}

impl Default for BufferPool {
    fn default() -> Self {
        Self {
            buffers: std::array::from_fn(|_| PoolEntry::default()),
            write_index: AtomicUsize::new(0),
        }
    }
}

impl BufferPool {
    /// Acquire a buffer for writing frame data.
    /// Returns the buffer index and a mutable pointer to the data if successful.
    /// This is optimistic - if the buffer at the current index is in use (refcount > 1),
    /// it checks subsequent buffers. If all are busy, returns None (caller should drop frame).
    pub fn acquire_for_write(&self, width: i32, height: i32) -> Option<(usize, *mut u8)> {
        let required_size = (width * height * 4) as usize;

        // Try each buffer in round-robin fashion
        for attempt in 0..BUFFER_COUNT {
            let idx = (self.write_index.load(Ordering::Relaxed) + attempt) % BUFFER_COUNT;
            let entry = &self.buffers[idx];

            // Check if we have exclusive access (refcount == 1 means only the pool holds a reference)
            unsafe {
                let arc_ptr = entry.data.get();
                // We need to be careful here. In a multi-threaded scenario, proper locking is safer.
                // However, `acquire_for_write` is called ONLY from the Render Thread.
                // The only other references are held by Frames traversing to Main Thread.
                // Reference count check is atomic.
                if Arc::strong_count(&*arc_ptr) == 1 {
                    // We own it exclusively! Safe to mutate.
                    // We can cast away the immutability of Arc because we verified uniqueness.
                    // Actually, Arc::get_mut is the safe way, but we have it inside UnsafeCell.
                    // We can replace the Arc with a new one if needed, or mutate the Vec inside.

                    let vec_ptr = &mut *arc_ptr;
                    // Note: `get_mut` on Arc returns Option<&mut T>.
                    // Since we checked strong_count == 1, `get_mut` should succeed unless weak refs exist.
                    // (We don't use weak refs).
                    if let Some(vec) = Arc::get_mut(vec_ptr) {
                        if vec.len() < required_size {
                            vec.resize(required_size, 0);
                            tracing::debug!(
                                "BufferPool: Resized buffer {} to {} bytes",
                                idx,
                                required_size
                            );
                        }

                        entry.width.store(width, Ordering::Relaxed);
                        entry.height.store(height, Ordering::Relaxed);

                        // Update write index for next time (skip this one)
                        self.write_index
                            .store((idx + 1) % BUFFER_COUNT, Ordering::Relaxed);

                        return Some((idx, vec.as_mut_ptr()));
                    }
                }
            }
        }

        // All buffers are busy (consumer is slow). Drop frame.
        None
    }

    /// Get a shared reference (Arc) to the buffer data for sending to consumer.
    /// Increments refcount, preventing reuse until consumer drops it.
    pub fn get_buffer_arc(&self, idx: usize) -> Option<Arc<Vec<u8>>> {
        if idx >= BUFFER_COUNT {
            return None;
        }
        unsafe {
            let arc_ptr = self.buffers[idx].data.get();
            // Clone the Arc, increasing refcount.
            Some((*arc_ptr).clone())
        }
    }
}

// Keep the old types for compatibility during migration
use epoxy::types::GLuint;

#[derive(Debug, Default)]
pub struct PboBuffer {
    pub id: GLuint,
    pub ptr: usize,
    pub width: i32,
    pub height: i32,
    pub is_free: AtomicBool,
}

unsafe impl Send for PboBuffer {}
unsafe impl Sync for PboBuffer {}

#[derive(Default)]
pub struct PboManager {
    pub buffers: std::sync::RwLock<[PboBuffer; 5]>,
}
