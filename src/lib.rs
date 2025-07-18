//! # Example:
//! ```rust
//! use arena::Arena;
//! use std::mem::{size_of};
//!
//! let mut arena = Arena::new(4096).expect("Should create a new arena");
//! let items = arena.alloc(Vec::new()).expect("Should allocate a vector");
//!
//! for i in 0..5 {
//!     items.push(i);
//! }
//!
//! let s_len = {
//!     let s = arena.alloc_str("test str").expect("Should allocate str");
//!     assert_eq!(s, "test str");
//!     s.len()
//! };
//! assert_eq!(arena.len(), size_of::<Vec<i32>>() + s_len);
//! ```

use std::{
    alloc::{Layout, alloc, dealloc},
    cell::UnsafeCell,
    ptr::{NonNull, null_mut},
};

pub struct Arena {
    inner: UnsafeCell<ArenaInner>,
}

struct ArenaInner {
    data: NonNull<[u8]>,
    offset: usize,
}

impl Arena {
    pub fn new(capacity: usize) -> Option<Self> {
        let layout = Layout::from_size_align(capacity, align_of::<usize>()).ok()?;

        let ptr = unsafe { alloc(layout) };
        if ptr.is_null() {
            return None;
        }

        let data = unsafe { std::slice::from_raw_parts_mut(ptr, capacity) };

        Some(Self {
            inner: ArenaInner {
                data: NonNull::from(data),
                offset: 0,
            }
            .into(),
        })
    }

    #[inline(always)]
    pub fn cap(&self) -> usize {
        unsafe { (*self.inner.get()).cap() }
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        unsafe { (*self.inner.get()).len() }
    }

    #[must_use]
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline(always)]
    pub fn reset(&mut self) {
        self.inner.get_mut().offset = 0;
    }

    pub fn alloc<T: Sized>(&mut self, obj: T) -> Option<&mut T> {
        let layout = Layout::new::<T>();
        let inner = self.inner();
        let ptr = inner.allocate(layout) as *mut T;

        if ptr.is_null() {
            return None;
        }

        unsafe {
            std::ptr::write(ptr, obj);
            Some(&mut *ptr)
        }
    }

    pub fn alloc_str(&mut self, s: &str) -> Option<&str> {
        let layout = Layout::from_size_align(s.len(), align_of::<u8>()).ok()?;
        let inner = self.inner();
        let ptr = inner.allocate(layout);

        if ptr.is_null() {
            return None;
        }

        unsafe {
            std::ptr::copy_nonoverlapping(s.as_ptr(), ptr, s.len());
            Some(std::str::from_utf8_unchecked_mut(
                std::slice::from_raw_parts_mut(ptr, s.len()),
            ))
        }
    }

    #[inline(always)]
    fn inner(&mut self) -> &mut ArenaInner {
        unsafe { &mut *self.inner.get() }
    }
}

impl ArenaInner {
    fn allocate(&mut self, layout: Layout) -> *mut u8 {
        let align = layout.align();
        let alloc_size = layout.size();

        let arena_size = self.data.len();

        let aligned_offset = (self.offset + align - 1) & !(align - 1);

        if aligned_offset + alloc_size > arena_size {
            return null_mut();
        }

        let ptr = unsafe { (self.data.as_ptr() as *mut u8).add(aligned_offset) };
        self.offset = aligned_offset + alloc_size;
        ptr
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        self.offset
    }

    #[inline(always)]
    pub fn cap(&self) -> usize {
        self.data.len()
    }
}

impl Drop for ArenaInner {
    fn drop(&mut self) {
        let layout = Layout::from_size_align(self.cap(), align_of::<usize>()).unwrap();

        unsafe {
            dealloc(self.data.as_ptr() as *mut u8, layout);
        };
    }
}
