use crate::{Slab, SlabAllocator, SlabManager};
use core::alloc::Layout;
use std::alloc::{Allocator, Global};

const LAYOUT_64: Layout = Layout::new::<[u8; 64]>();
const LAYOUT_128: Layout = Layout::new::<[u8; 128]>();
const LAYOUT_256: Layout = Layout::new::<[u8; 256]>();
const LAYOUT_512: Layout = Layout::new::<[u8; 512]>();
const LAYOUT_1024: Layout = Layout::new::<[u8; 1024]>();
const LAYOUT_2048: Layout = Layout::new::<[u8; 2048]>();

#[test]
pub fn slab_allocate() {
    let mut slab = Slab::<64, Global>::new_in(Global).unwrap();
    assert!(slab.remaining_object_count() == 64);

    let object = slab.next_object().unwrap();
    assert!(slab.remaining_object_count() == 63);

    // Safety: Object originated from `slab`.
    unsafe {
        slab.return_object(object.as_non_null_ptr());
    }
    assert!(slab.remaining_object_count() == 64);
}

#[test]
pub fn slab_manager_allocate() {
    let mut slab_manager = SlabManager::<64, Global>::new_in(Global);
    assert!(slab_manager.remaining_object_count() == 0);

    let object = slab_manager.next_object().unwrap();
    assert!(slab_manager.remaining_object_count == 63);

    // Safety: Object originated from `slab_manager`.
    unsafe {
        slab_manager.return_object(object.as_non_null_ptr());
    }
    assert!(slab_manager.remaining_object_count == 64);
}

#[test]
pub fn slab_allocator_allocate_one() {
    let slab_allocator = SlabAllocator::new_in(Global);

    let allocate_64 = slab_allocator.allocate(LAYOUT_64).unwrap();
    assert!(allocate_64.len() == 64);

    let allocate_128 = slab_allocator.allocate(LAYOUT_128).unwrap();
    assert!(allocate_128.len() == 128);

    let allocate_256 = slab_allocator.allocate(LAYOUT_256).unwrap();
    assert!(allocate_256.len() == 256);

    let allocate_512 = slab_allocator.allocate(LAYOUT_512).unwrap();
    assert!(allocate_512.len() == 512);

    let allocate_1024 = slab_allocator.allocate(LAYOUT_1024).unwrap();
    assert!(allocate_1024.len() == 1024);

    let allocate_2048 = slab_allocator.allocate(LAYOUT_2048).unwrap();
    assert!(allocate_2048.len() == 2048);

    // Safety: Allocations are returned identically to their allocator.
    unsafe {
        slab_allocator.deallocate(allocate_64.as_non_null_ptr(), LAYOUT_64);
        slab_allocator.deallocate(allocate_128.as_non_null_ptr(), LAYOUT_128);
        slab_allocator.deallocate(allocate_256.as_non_null_ptr(), LAYOUT_256);
        slab_allocator.deallocate(allocate_512.as_non_null_ptr(), LAYOUT_512);
        slab_allocator.deallocate(allocate_1024.as_non_null_ptr(), LAYOUT_1024);
        slab_allocator.deallocate(allocate_2048.as_non_null_ptr(), LAYOUT_2048);
    }
}

#[test]
pub fn slab_allocator_allocate_extra() {
    let slab_allocator = SlabAllocator::new_in(Global);
    assert!(slab_allocator.remaining_object_count::<2048>() == 0);

    let allocation_1 = slab_allocator.allocate(LAYOUT_2048).unwrap();
    assert!(slab_allocator.remaining_object_count::<2048>() == 1);
    let allocation_2 = slab_allocator.allocate(LAYOUT_2048).unwrap();
    assert!(slab_allocator.remaining_object_count::<2048>() == 0);
    let allocation_3 = slab_allocator.allocate(LAYOUT_2048).unwrap();
    assert!(slab_allocator.remaining_object_count::<2048>() == 1);
    let allocation_4 = slab_allocator.allocate(LAYOUT_2048).unwrap();
    assert!(slab_allocator.remaining_object_count::<2048>() == 0);
    let allocation_5 = slab_allocator.allocate(LAYOUT_2048).unwrap();
    assert!(slab_allocator.remaining_object_count::<2048>() == 1);

    // Safety: Allocations are returned identically to their allocator.
    unsafe {
        slab_allocator.deallocate(allocation_1.as_non_null_ptr(), LAYOUT_2048);
        assert!(slab_allocator.remaining_object_count::<2048>() == 2);
        slab_allocator.deallocate(allocation_2.as_non_null_ptr(), LAYOUT_2048);
        assert!(slab_allocator.remaining_object_count::<2048>() == 3);
        slab_allocator.deallocate(allocation_3.as_non_null_ptr(), LAYOUT_2048);
        assert!(slab_allocator.remaining_object_count::<2048>() == 4);
        slab_allocator.deallocate(allocation_4.as_non_null_ptr(), LAYOUT_2048);
        assert!(slab_allocator.remaining_object_count::<2048>() == 5);
        slab_allocator.deallocate(allocation_5.as_non_null_ptr(), LAYOUT_2048);
        assert!(slab_allocator.remaining_object_count::<2048>() == 6);
    }
}
