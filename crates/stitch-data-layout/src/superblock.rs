//! SuperBlock and CacheAligned container wrappers.

use core::ops::{Deref, DerefMut};

/// Cache-line aligned container wrapper.
///
/// Ensures the wrapped value starts at a 64-byte cache boundary (or 128B).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C, align(64))]
pub struct CacheAligned<T>(pub T);

impl<T> CacheAligned<T> {
    pub const fn new(val: T) -> Self {
        Self(val)
    }

    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> Deref for CacheAligned<T> {
    type Target = T;
    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for CacheAligned<T> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// Zero-overhead cache-padded element wrapper.
///
/// Pads any structure `T` so that its overall `size_of::<CachePadded<T>>()` is rounded up
/// to the next integral multiple of the cache line, preventing false sharing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C, align(64))]
pub struct CachePadded<T> {
    value: T,
}

impl<T> CachePadded<T> {
    pub const fn new(value: T) -> Self {
        Self { value }
    }

    pub fn into_inner(self) -> T {
        self.value
    }
}

impl<T> Deref for CachePadded<T> {
    type Target = T;
    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl<T> DerefMut for CachePadded<T> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.value
    }
}

/// A contiguous "SuperBlock" tile holding an exact number of items `COUNT` such that
/// the total byte size `COUNT * size_of::<T>()` aligns with cache line boundaries.
///
/// Within a slice of `CacheSuperBlock<T, COUNT>`, individual tile boundaries **never**
/// cross or split across unaligned cache lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C, align(64))]
pub struct CacheSuperBlock<T, const COUNT: usize> {
    pub items: [T; COUNT],
}

impl<T, const COUNT: usize> CacheSuperBlock<T, COUNT> {
    /// Creates a new `CacheSuperBlock` with the given items array.
    pub const fn new(items: [T; COUNT]) -> Self {
        Self { items }
    }

    /// Number of items in this super-block.
    pub const fn len(&self) -> usize {
        COUNT
    }

    /// Whether this super-block is empty.
    pub const fn is_empty(&self) -> bool {
        COUNT == 0
    }

    /// Returns a slice of the inner items.
    #[inline(always)]
    pub fn as_slice(&self) -> &[T] {
        &self.items
    }

    /// Returns a mutable slice of the inner items.
    #[inline(always)]
    pub fn as_mut_slice(&mut self) -> &mut [T] {
        &mut self.items
    }
}

impl<T, const COUNT: usize> Deref for CacheSuperBlock<T, COUNT> {
    type Target = [T; COUNT];
    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        &self.items
    }
}

impl<T, const COUNT: usize> DerefMut for CacheSuperBlock<T, COUNT> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.items
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_aligned_size_and_alignment() {
        let aligned = CacheAligned::new(42u32);
        assert_eq!(core::mem::align_of_val(&aligned), 64);
        assert_eq!(*aligned, 42);
    }

    #[test]
    fn test_cache_padded_false_sharing_prevention() {
        let padded = CachePadded::new(100u64);
        assert_eq!(core::mem::align_of_val(&padded), 64);
        assert_eq!(core::mem::size_of_val(&padded), 64);
        assert_eq!(*padded, 100);
    }

    #[test]
    fn test_cache_superblock_tile_alignment() {
        // Mock 24-byte struct (e.g. legacy ChunkDescriptor)
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        struct Mock24([u8; 24]);

        assert_eq!(core::mem::size_of::<Mock24>(), 24);

        // 8 items of 24B = 192B = exactly 3 cache lines (3 * 64)
        let block: CacheSuperBlock<Mock24, 8> = CacheSuperBlock::new([Mock24([0; 24]); 8]);

        assert_eq!(core::mem::align_of_val(&block), 64);
        assert_eq!(core::mem::size_of_val(&block), 192);
        assert_eq!(block.len(), 8);
        assert_eq!(block.as_slice().len(), 8);
    }
}
