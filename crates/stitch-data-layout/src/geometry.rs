//! Cache-line geometry calculations and compile-time layout analysis.

/// Standard x86-64 / modern AArch64 L1D cache-line size in bytes.
pub const LINE_SIZE_64: usize = 64;

/// Extended Apple Silicon / large-server L2/L3 cache-line size in bytes.
pub const LINE_SIZE_128: usize = 128;

/// Default target cache-line size for mechanical sympathy.
pub type DefaultCacheLine = CacheGeometry<LINE_SIZE_64>;

/// Static const calculators for cache geometry and straddling detection.
pub struct CacheGeometry<const LINE_SIZE: usize>;

impl<const LINE_SIZE: usize> CacheGeometry<LINE_SIZE> {
    pub const LINE: usize = LINE_SIZE;

    /// Checks if a type of size `size` evenly divides the cache line.
    ///
    /// Returns `true` if `LINE_SIZE % size == 0` (e.g. 1, 2, 4, 8, 16, 32, 64 for 64B).
    #[inline]
    pub const fn is_perfect_divisor(size: usize) -> bool {
        if size == 0 || size > LINE_SIZE {
            false
        } else {
            LINE_SIZE.is_multiple_of(size)
        }
    }

    /// Checks if an array of elements of size `size` causes cache line straddling.
    ///
    /// A type straddles if its contiguous packing crosses cache line boundaries
    /// before completing an integral multiple of lines.
    #[inline]
    pub const fn does_straddle(size: usize) -> bool {
        !Self::is_perfect_divisor(size)
    }

    /// Returns the number of items that fit into a single cache line without straddling
    /// (rounded down).
    #[inline]
    pub const fn items_per_line(size: usize) -> usize {
        match LINE_SIZE.checked_div(size) {
            Some(count) => count,
            None => 0,
        }
    }

    /// Calculates the greatest common divisor of two numbers (const Euclidean algorithm).
    pub const fn gcd(mut a: usize, mut b: usize) -> usize {
        while b != 0 {
            let t = b;
            b = a % b;
            a = t;
        }
        a
    }

    /// Calculates the least common multiple of two numbers.
    pub const fn lcm(a: usize, b: usize) -> usize {
        if a == 0 || b == 0 {
            0
        } else {
            (a / Self::gcd(a, b)) * b
        }
    }

    /// Calculates the minimal number of items `N` needed to form a "SuperBlock" tile
    /// such that `N * size` is an exact multiple of `LINE_SIZE`.
    ///
    /// For example:
    /// - `size = 24B`, `LINE_SIZE = 64B`: `lcm(24, 64) = 192B`.
    ///   `N = 192 / 24 = 8` items across `192 / 64 = 3` cache lines with **0 bytes waste**!
    /// - `size = 40B`, `LINE_SIZE = 64B`: `lcm(40, 64) = 320B`.
    ///   `N = 320 / 40 = 8` items across `320 / 64 = 5` cache lines with **0 bytes waste**!
    #[inline]
    pub const fn superblock_item_count(size: usize) -> usize {
        match Self::lcm(size, LINE_SIZE).checked_div(size) {
            Some(count) => count,
            None => 0,
        }
    }

    /// Calculates total bytes of a SuperBlock tile for a given item size.
    #[inline]
    pub const fn superblock_byte_size(size: usize) -> usize {
        Self::lcm(size, LINE_SIZE)
    }

    /// Calculates the number of cache lines spanned by one SuperBlock tile.
    #[inline]
    pub const fn superblock_lines_spanned(size: usize) -> usize {
        match Self::superblock_byte_size(size).checked_div(LINE_SIZE) {
            Some(lines) => lines,
            None => 0,
        }
    }
}

/// Compile-time assertion macro ensuring a type does not straddle cache lines.
#[macro_export]
macro_rules! assert_cache_friendly {
    ($T:ty) => {
        const _: () = {
            let size = ::core::mem::size_of::<$T>();
            if $crate::geometry::DefaultCacheLine::does_straddle(size) {
                panic!("Type straddles 64-byte cache line boundaries! Consider using CacheSuperBlock or packing to a power of two.");
            }
        };
    };
    ($T:ty, $LINE_SIZE:expr) => {
        const _: () = {
            let size = ::core::mem::size_of::<$T>();
            if $crate::geometry::CacheGeometry::<$LINE_SIZE>::does_straddle(size) {
                panic!("Type straddles cache line boundaries! Consider using CacheSuperBlock or packing to a divisor.");
            }
        };
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_perfect_divisors() {
        assert!(DefaultCacheLine::is_perfect_divisor(1));
        assert!(DefaultCacheLine::is_perfect_divisor(2));
        assert!(DefaultCacheLine::is_perfect_divisor(4));
        assert!(DefaultCacheLine::is_perfect_divisor(8));
        assert!(DefaultCacheLine::is_perfect_divisor(16));
        assert!(DefaultCacheLine::is_perfect_divisor(32));
        assert!(DefaultCacheLine::is_perfect_divisor(64));

        assert!(!DefaultCacheLine::is_perfect_divisor(3));
        assert!(!DefaultCacheLine::is_perfect_divisor(24));
        assert!(!DefaultCacheLine::is_perfect_divisor(40));
        assert!(!DefaultCacheLine::is_perfect_divisor(48));
    }

    #[test]
    fn test_superblock_lcm_calculations() {
        // 24-byte items (legacy ChunkDescriptor):
        // LCM(24, 64) = 192 bytes = 3 cache lines = 8 items!
        assert_eq!(DefaultCacheLine::superblock_item_count(24), 8);
        assert_eq!(DefaultCacheLine::superblock_byte_size(24), 192);
        assert_eq!(DefaultCacheLine::superblock_lines_spanned(24), 3);

        // 40-byte items:
        // LCM(40, 64) = 320 bytes = 5 cache lines = 8 items!
        assert_eq!(DefaultCacheLine::superblock_item_count(40), 8);
        assert_eq!(DefaultCacheLine::superblock_byte_size(40), 320);
        assert_eq!(DefaultCacheLine::superblock_lines_spanned(40), 5);

        // 48-byte items:
        // LCM(48, 64) = 192 bytes = 3 cache lines = 4 items!
        assert_eq!(DefaultCacheLine::superblock_item_count(48), 4);
        assert_eq!(DefaultCacheLine::superblock_byte_size(48), 192);
        assert_eq!(DefaultCacheLine::superblock_lines_spanned(48), 3);

        // 16-byte items (CompactChunkDescriptor):
        // Perfect divisor: 4 items = 1 cache line (64 bytes)
        assert_eq!(DefaultCacheLine::superblock_item_count(16), 4);
        assert_eq!(DefaultCacheLine::superblock_byte_size(16), 64);
        assert_eq!(DefaultCacheLine::superblock_lines_spanned(16), 1);
    }
}
