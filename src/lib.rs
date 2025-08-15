#![cfg_attr(not(test), no_std)]
#![feature(allocator_api, slice_ptr_get)]

#[cfg(test)]
mod tests;

extern crate alloc;

use alloc::{
    alloc::{AllocError, Allocator},
    vec::Vec,
};
use core::{alloc::Layout, cmp::max, ops::Range, ptr::NonNull};
use spin::Mutex;

const fn objects_per_page<const OBJECT_SIZE: usize>() -> usize {
    0x1000 / OBJECT_SIZE
}

struct Slab<const OBJECT_SIZE: usize, A: Allocator> {
    bitmap: u64,
    memory: NonNull<[u8]>,
    inner: A,
}

impl<const OBJECT_SIZE: usize, A: Allocator> Slab<OBJECT_SIZE, A> {
    // Safety: Layout is known to be valid.
    const LAYOUT: Layout = unsafe { Layout::from_size_align_unchecked(0x1000, 0x1000) };

    pub fn new_in(allocator: A) -> Result<Self, AllocError> {
        assert!(OBJECT_SIZE >= 64);
        assert!(OBJECT_SIZE < 0x1000);
        assert!(OBJECT_SIZE.is_power_of_two());

        // `objects_per_page()` will never overflow `u32`.
        #[allow(clippy::cast_possible_truncation, clippy::as_conversions)]
        let objects_per_page = objects_per_page::<OBJECT_SIZE>() as u32;

        Ok(Self {
            bitmap: 1u64.unbounded_shl(objects_per_page).wrapping_sub(1),
            memory: allocator.allocate(Self::LAYOUT)?,
            inner: allocator,
        })
    }

    /// Range of addresses that are covered by this slab.
    pub fn memory_range(&self) -> Range<usize> {
        let start_address = self.memory.addr().get();
        start_address..(start_address + self.memory.len())
    }

    /// Currently remaining (free) objects in this slab.
    pub fn remaining_object_count(&self) -> usize {
        // `u64::count_ones()` will never overflow a `usize`.
        #[allow(clippy::cast_possible_truncation, clippy::as_conversions)]
        {
            self.bitmap.count_ones() as usize
        }
    }

    /// Whether the slab is empty.
    pub fn is_empty(&self) -> bool {
        self.remaining_object_count() == 0
    }

    pub fn next_object(&mut self) -> Option<NonNull<[u8]>> {
        (!self.is_empty()).then(|| {
            // `u64::trailing_zeros()` will never overflow a `usize`.
            #[allow(clippy::cast_possible_truncation, clippy::as_conversions)]
            let object_index = self.bitmap.trailing_zeros() as usize;

            debug_assert!((self.bitmap & (1 << object_index)) > 0);

            // Clear the bit in the bitmap.
            self.bitmap &= !(1 << object_index);

            let byte_index_start = object_index * OBJECT_SIZE;
            let byte_index_end = byte_index_start + OBJECT_SIZE;

            // Safety: Indexes are checked to be within bounds.
            unsafe {
                self.memory
                    .get_unchecked_mut(byte_index_start..byte_index_end)
            }
        })
    }

    /// # Safety
    ///
    /// - `object_ptr` must point to an object that originated from this slab.
    pub unsafe fn return_object(&mut self, object_ptr: NonNull<u8>) {
        debug_assert!(self.memory_range().contains(&object_ptr.addr().get()));

        // Safety:
        // - `object` is checked to be contained by `self.memory`.
        // - `object`, lying within `self.memory`, points to the same allocation.
        let byte_offset = unsafe { object_ptr.byte_offset_from_unsigned(self.memory) };
        let object_index = byte_offset >> OBJECT_SIZE.trailing_zeros();

        debug_assert!((self.bitmap & (1 << object_index)) == 0);

        // Set the bit in the bitmap.
        self.bitmap |= 1 << object_index;
    }
}

impl<const OBJECT_SIZE: usize, A: Allocator> Drop for Slab<OBJECT_SIZE, A> {
    fn drop(&mut self) {
        // Safety: `self` is being dropped, `self.slab` will no longer be used.
        unsafe {
            self.inner
                .deallocate(self.memory.as_non_null_ptr(), Self::LAYOUT);
        }
    }
}

struct SlabManager<const OBJECT_SIZE: usize, A: Allocator> {
    slabs: Vec<Slab<OBJECT_SIZE, A>, A>,
    remaining_object_count: usize,
    inner: A,
}

impl<const SIZE_BITS: usize, A: Allocator + Clone> SlabManager<SIZE_BITS, A> {
    pub fn new_in(allocator: A) -> Self {
        Self {
            slabs: Vec::new_in(allocator.clone()),
            remaining_object_count: 0,
            inner: allocator,
        }
    }

    pub fn next_object(&mut self) -> Result<NonNull<[u8]>, AllocError> {
        if self.is_empty() {
            let mut new_slab = Slab::new_in(self.inner.clone())?;

            debug_assert!(!new_slab.is_empty());

            // Safety: Slab was just allocated.
            let object = unsafe { new_slab.next_object().unwrap_unchecked() };

            self.remaining_object_count += new_slab.remaining_object_count();

            self.slabs.push(new_slab);

            Ok(object)
        } else {
            let object = self.slabs.iter_mut().find_map(Slab::next_object).unwrap();

            self.remaining_object_count -= 1;

            Ok(object)
        }
    }
}

impl<const SIZE_BITS: usize, A: Allocator> SlabManager<SIZE_BITS, A> {
    pub fn remaining_object_count(&self) -> usize {
        self.remaining_object_count
    }

    pub fn is_empty(&self) -> bool {
        self.remaining_object_count() == 0
    }

    /// # Safety
    ///
    /// - `object_ptr` must point to an object that originated from this slab manager.
    pub unsafe fn return_object(&mut self, object_ptr: NonNull<u8>) {
        let slab = self
            .slabs
            .iter_mut()
            .find(|slab| slab.memory_range().contains(&object_ptr.addr().get()));
        debug_assert!(slab.is_some());

        // Safety: Caller is required to ensure object belongs to this slab manager.
        unsafe {
            slab.unwrap_unchecked().return_object(object_ptr);
        }

        self.remaining_object_count += 1;
    }
}

pub struct SlabAllocator<A: Allocator> {
    slab_64: Mutex<SlabManager<64, A>>,
    slab_128: Mutex<SlabManager<128, A>>,
    slab_256: Mutex<SlabManager<256, A>>,
    slab_512: Mutex<SlabManager<512, A>>,
    slab_1024: Mutex<SlabManager<1024, A>>,
    slab_2048: Mutex<SlabManager<2048, A>>,
    inner: A,
}

impl<A: Allocator + Clone> SlabAllocator<A> {
    pub fn new_in(allocator: A) -> Self {
        Self {
            slab_64: Mutex::new(SlabManager::new_in(allocator.clone())),
            slab_128: Mutex::new(SlabManager::new_in(allocator.clone())),
            slab_256: Mutex::new(SlabManager::new_in(allocator.clone())),
            slab_512: Mutex::new(SlabManager::new_in(allocator.clone())),
            slab_1024: Mutex::new(SlabManager::new_in(allocator.clone())),
            slab_2048: Mutex::new(SlabManager::new_in(allocator.clone())),
            inner: allocator,
        }
    }
}

// Safety:
// Memory blocks are not freed unless:
// - `Allocator::deallocate` is called.
// - `Self` is dropped.
unsafe impl<A: Allocator + Clone> Allocator for SlabAllocator<A> {
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        let allocation_size = max(layout.size().next_power_of_two(), layout.align());
        debug_assert!(allocation_size.is_power_of_two());

        match allocation_size {
            64 => {
                let mut slab_64 = self.slab_64.lock();
                slab_64.next_object()
            }

            128 => {
                let mut slab_128 = self.slab_128.lock();
                slab_128.next_object()
            }

            256 => {
                let mut slab_256 = self.slab_256.lock();
                slab_256.next_object()
            }

            512 => {
                let mut slab_512 = self.slab_512.lock();
                slab_512.next_object()
            }

            1024 => {
                let mut slab_1024 = self.slab_1024.lock();
                slab_1024.next_object()
            }

            2048 => {
                let mut slab_2048 = self.slab_2048.lock();
                slab_2048.next_object()
            }

            _ => self.inner.allocate(layout),
        }
    }

    unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        let allocation_size = max(layout.size().next_power_of_two(), layout.align());
        debug_assert!(allocation_size.is_power_of_two());

        match allocation_size {
            64 => {
                let mut slab_64 = self.slab_64.lock();

                // Safety: Object size matches this slab size, and so is guaranteed to originate from it.
                unsafe {
                    slab_64.return_object(ptr);
                }
            }

            128 => {
                let mut slab_128 = self.slab_128.lock();

                // Safety: Object size matches this slab size, and so is guaranteed to originate from it.
                unsafe {
                    slab_128.return_object(ptr);
                }
            }

            256 => {
                let mut slab_256 = self.slab_256.lock();

                // Safety: Object size matches this slab size, and so is guaranteed to originate from it.
                unsafe {
                    slab_256.return_object(ptr);
                }
            }

            512 => {
                let mut slab_512 = self.slab_512.lock();

                // Safety: Object size matches this slab size, and so is guaranteed to originate from it.
                unsafe {
                    slab_512.return_object(ptr);
                }
            }

            1024 => {
                let mut slab_1024 = self.slab_1024.lock();

                // Safety: Object size matches this slab size, and so is guaranteed to originate from it.
                unsafe {
                    slab_1024.return_object(ptr);
                }
            }

            2048 => {
                let mut slab_2048 = self.slab_2048.lock();

                // Safety: Object size matches this slab size, and so is guaranteed to originate from it.
                unsafe {
                    slab_2048.return_object(ptr);
                }
            }

            _ => {
                // Safety: Caller is required to maintain safety invariants.
                unsafe {
                    self.inner.deallocate(ptr, layout);
                }
            }
        }
    }
}
