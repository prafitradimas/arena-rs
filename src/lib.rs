use std::{
    alloc::{Layout, LayoutError, alloc, dealloc},
    cell::{Cell, UnsafeCell},
    fmt::Display,
    ptr::NonNull,
};

#[repr(C)]
pub struct Arena {
    blocks: Vec<UnsafeCell<Block>>,
    block_size: BlockSize,
}

impl Arena {
    pub fn new() -> Result<Self, ArenaError> {
        let block_size = DEFAULT_BLOCK_SIZE;
        Self::with_block_size(block_size)
    }

    pub fn with_block_size(size: usize) -> Result<Self, ArenaError> {
        let block = Block::new(size)?;

        Ok(Self {
            blocks: vec![UnsafeCell::new(block)],
            block_size: size,
        })
    }

    pub fn scope<Func, FuncResult>(&mut self, func: Func) -> FuncResult
    where
        Func: FnOnce(&mut Arena) -> FuncResult,
    {
        let snapshot = self.snapshot();
        let result = func(self);
        self.rewind_to(snapshot);

        result
    }

    #[inline]
    pub fn alloc<T: Sized>(&mut self, obj: T) -> Result<&mut T, ArenaError> {
        let layout = Layout::new::<T>();
        let ptr = self.try_alloc(layout)? as *mut T;
        unsafe {
            std::ptr::write(ptr, obj);
            Ok(&mut *ptr)
        }
    }

    #[inline]
    pub fn alloc_slice<T: Sized>(&mut self, length: usize) -> Result<&mut [T], ArenaError> {
        let layout = Layout::array::<T>(length)?;
        let ptr = self.try_alloc(layout)? as *mut T;
        unsafe {
            std::ptr::write_bytes(ptr, 0, length);
            Ok(&mut *std::ptr::slice_from_raw_parts_mut(ptr, length))
        }
    }

    #[inline]
    pub fn alloc_str(&mut self, str: &str) -> Result<&str, ArenaError> {
        let copied = self.copy_slice(str.as_bytes())?;
        let slice = unsafe { std::str::from_utf8_unchecked(copied) };
        Ok(slice)
    }

    #[inline]
    pub fn copy_slice<T: Copy>(&mut self, slice: &[T]) -> Result<&mut [T], ArenaError> {
        let layout = Layout::array::<T>(slice.len())?;
        let ptr = self.try_alloc(layout)? as *mut T;
        unsafe {
            std::ptr::copy_nonoverlapping(slice.as_ptr(), ptr, slice.len());
            Ok(&mut *std::ptr::slice_from_raw_parts_mut(ptr, slice.len()))
        }
    }

    #[inline]
    pub fn reset(&mut self) {
        for block in &mut self.blocks {
            block.get_mut().reset();
        }
    }

    #[inline]
    pub fn reset_zeroed(&mut self) {
        for block in &mut self.blocks {
            block.get_mut().reset_zeroed();
        }
    }

    #[inline]
    fn try_alloc(&mut self, layout: Layout) -> Result<*mut u8, ArenaError> {
        let block = match self.try_get_block(layout) {
            Some(block) => block,
            None => self.alloc_new_block(layout.size())?,
        };
        block.alloc(layout)
    }

    #[inline]
    fn alloc_new_block(&mut self, size: BlockSize) -> Result<&mut Block, ArenaError> {
        let block = Block::new(self.block_size.max(size))?;

        self.blocks.push(UnsafeCell::new(block));
        Ok(self.blocks.last_mut().unwrap().get_mut())
    }

    #[inline]
    fn try_get_block(&mut self, layout: Layout) -> Option<&mut Block> {
        for block in &mut self.blocks {
            let deref_block = block.get_mut();
            if deref_block.remaining() > layout.size() {
                return Some(deref_block);
            }
        }
        None
    }

    pub fn snapshot(&self) -> ArenaSnapshot {
        let block_idx = self.blocks.len() - 1;
        let block = unsafe { &*self.blocks[block_idx].get() };
        let offset = block.curr_ptr.get();

        ArenaSnapshot { block_idx, offset }
    }

    #[inline]
    pub fn rewind_to(&mut self, snapshot: ArenaSnapshot) {
        if let Some(block) = self.blocks.get_mut(snapshot.block_idx) {
            let block = block.get_mut();
            block.rewind_to(snapshot.offset);
        }

        for block in self.blocks.iter_mut().skip(snapshot.block_idx + 1) {
            block.get_mut().reset();
        }
    }

    #[cfg(feature = "debug")]
    pub fn dump(&self) {
        println!("Arena Debug Dump");
        println!("================");
        println!("Total blocks: {}", self.blocks.len());

        for (i, block) in self.blocks.iter().enumerate() {
            unsafe { &*block.get() }.dump(i);
        }

        println!();
    }
}

#[must_use]
pub struct ArenaSnapshot {
    block_idx: usize,

    /// block's save point
    offset: *mut u8,
}

type BlockPtr = NonNull<u8>;
type BlockSize = usize;
type BlockCursor = Cell<*mut u8>;

const DEFAULT_BLOCK_SIZE: BlockSize = 64 * 1024;

#[repr(C)]
struct Block {
    start_ptr: BlockPtr,
    end_ptr: BlockPtr,
    curr_ptr: BlockCursor,
    size: BlockSize,
}

impl Block {
    pub fn new(size: BlockSize) -> Result<Self, ArenaError> {
        if size == 0 {
            return Err(ArenaError::ZeroSize);
        }

        let alignment = align_of::<usize>();
        let layout = Layout::from_size_align(size, alignment)?;

        unsafe {
            let ptr = alloc(layout);
            if ptr.is_null() {
                Err(ArenaError::InsufficientMemory)
            } else {
                let start_ptr = NonNull::new_unchecked(ptr);

                Ok(Self {
                    start_ptr,
                    end_ptr: start_ptr.add(size),
                    curr_ptr: BlockCursor::new(ptr),
                    size,
                })
            }
        }
    }

    pub fn alloc(&self, layout: Layout) -> Result<*mut u8, ArenaError> {
        let size = layout.size();
        let alignment = layout.align();

        let old_ptr = self.curr_ptr.get();

        let align_mask = !(alignment - 1);
        let aligned = ((old_ptr as usize + alignment - 1) & align_mask) as *mut u8;

        let new_ptr = unsafe { aligned.add(size) };
        if new_ptr > self.end_ptr.as_ptr() {
            return Err(ArenaError::InsufficientMemory);
        }

        self.curr_ptr.set(new_ptr);
        Ok(aligned)
    }

    #[inline]
    pub fn rewind_to(&mut self, save_point: *mut u8) {
        self.curr_ptr.set(save_point);
    }

    #[inline]
    pub fn reset(&mut self) {
        self.curr_ptr.set(self.start_ptr.as_ptr());
    }

    #[inline]
    pub fn reset_zeroed(&mut self) {
        self.reset();
        unsafe { std::ptr::write_bytes(self.start_ptr.as_ptr(), 0, self.size) };
    }

    #[inline]
    pub fn remaining(&self) -> BlockSize {
        (self.end_ptr.as_ptr() as usize) - (self.curr_ptr.as_ptr() as usize)
    }

    #[cfg(test)]
    pub fn as_ptr(&self) -> *mut u8 {
        self.start_ptr.as_ptr()
    }

    #[cfg(feature = "debug")]
    pub fn dump(&self, index: usize) {
        let start = self.start_ptr.as_ptr() as usize;
        let end = self.end_ptr.as_ptr() as usize;
        let curr = self.curr_ptr.get() as usize;

        println!(
            "  Block[{index}]: size = {:>6} bytes | used = {:>6} bytes | remaining = {:>6} bytes",
            self.size,
            curr - start,
            end - curr
        );
        println!("    start = 0x{start:x}, curr = 0x{curr:x}, end = 0x{end:x}");
    }
}

impl Drop for Block {
    fn drop(&mut self) {
        unsafe {
            let layout = Layout::from_size_align_unchecked(self.size, align_of::<u8>());
            dealloc(self.start_ptr.as_ptr(), layout);
        }
    }
}

#[derive(Debug)]
#[must_use]
pub enum ArenaError {
    /// Zero-size blocks are invalid
    ZeroSize,

    /// Block alignment must be power of two
    BadAlignment,

    /// OOM, couldn't allocate block
    InsufficientMemory,
}

impl Display for ArenaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ArenaError::BadAlignment => {
                f.write_str("Size should be non-zero and must be power of two.")
            }
            ArenaError::InsufficientMemory => f.write_str("Out of Memory."),
            ArenaError::ZeroSize => write!(f, "Cannot allocate block of size zero"),
        }
    }
}

impl From<LayoutError> for ArenaError {
    fn from(_: LayoutError) -> Self {
        ArenaError::BadAlignment
    }
}

impl std::error::Error for ArenaError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_have_low_bits_eq_0() {
        let size = 32;
        let mask = size - 1;

        let block = Block::new(size).unwrap();

        // the block address bitwise AND the alignment bits (size - 1) should
        // be a mutually exclusive set of bits
        assert!((block.as_ptr() as usize & mask) ^ mask == mask);
    }
}
