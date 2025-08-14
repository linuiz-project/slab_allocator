#![cfg_attr(not(test), no_std)]
#![feature(allocator_api, slice_ptr_get)]

extern crate alloc;

use alloc::{
    alloc::{AllocError, Allocator},
    vec::Vec,
};
use core::{alloc::Layout, ptr::NonNull};
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
        assert!(OBJECT_SIZE < 0x1000);
        assert!(OBJECT_SIZE.is_power_of_two());

        Ok(Self {
            bitmap: (1 << objects_per_page::<OBJECT_SIZE>()) - 1,
            memory: allocator.allocate(Self::LAYOUT)?,
            inner: allocator,
        })
    }

    pub const fn object_count() -> usize {
        objects_per_page::<OBJECT_SIZE>()
    }

    pub fn is_empty(&self) -> bool {
        // The object count should NEVER overflow a u32.
        #[allow(clippy::cast_possible_truncation, clippy::as_conversions)]
        {
            self.bitmap.trailing_zeros() >= (Self::object_count() as u32)
        }
    }

    pub fn next_object(&mut self) -> Option<NonNull<[u8]>> {
        (!self.is_empty()).then(|| {
            // `u64::trailing_zeros()` will never overflow a `usize`.
            #[allow(clippy::cast_possible_truncation, clippy::as_conversions)]
            let object_index = self.bitmap.trailing_zeros() as usize;

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
    free_count: usize,
    inner: A,
}

impl<const SIZE_BITS: usize, A: Allocator + Clone> SlabManager<SIZE_BITS, A> {
    pub fn new_in(allocator: A) -> Self {
        Self {
            slabs: Vec::new_in(allocator.clone()),
            free_count: 0,
            inner: allocator,
        }
    }
}

impl<const SIZE_BITS: usize, A: Allocator> SlabManager<SIZE_BITS, A> {
    pub fn next_object(&mut self) -> Result<NonNull<[u8]>, AllocError> {
        if let Some(object) = self.slabs.iter_mut().find_map(Slab::next_object) {
            Ok(object)
        } else {
            let mut new_slab = Slab::new_in(self.inner.clone())?;

            debug_assert!(!new_slab.is_empty());

            // Safety: Slab was just allocated.
            let object = unsafe { new_slab.next_object().unwrap_unchecked() };

            self.slabs.push(new_slab);

            Ok(object)
        }
    }
}

pub struct SlabAllocator<A: Allocator> {
    slab_64: Mutex<SlabManager<64, A>>,
    slab_128: Mutex<SlabManager<128, A>>,
    slab_256: Mutex<SlabManager<256, A>>,
    slab_512: Mutex<SlabManager<512, A>>,
    slab_1024: Mutex<SlabManager<1024, A>>,
    slab_2048: Mutex<SlabManager<2048, A>>,
    inner: Allocator,
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
unsafe impl<A: Allocator> Allocator for SlabAllocator<A> {
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        let allocation_size = core::cmp::max(layout.size().next_power_of_two(), layout.align());
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

    unsafe fn deallocate(&self, core::ptr: NonNull<u8>, layout: Layout) {
        todo!()
    }
}
