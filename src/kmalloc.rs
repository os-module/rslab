use crate::slab::{alloc_from_slab, create_mem_cache, dealloc_to_slab};
use crate::SLAB_CACHES;
use core::alloc::{Allocator, AllocError, GlobalAlloc, Layout};
use core::ptr::NonNull;
use doubly_linked_list::*;
use crate::formation::SlabError;

const CACHE_INFO_MAX: usize = 21;
const KMALLOC_INFO: [&'static str; CACHE_INFO_MAX] = [
    "malloc-8",
    "malloc-16",
    "malloc-32",
    "malloc-64",
    "malloc-128",
    "malloc-256",
    "malloc-512",
    "malloc-1024",
    "malloc-2048",
    "malloc-4096",
    "malloc-8192",
    "malloc_16384",
    "malloc_32768",
    "malloc_65536",
    "malloc_131072",
    "malloc_262144",
    "malloc_524288",
    "malloc_1048576",
    "malloc_2097152",
    "malloc_4194304",
    "malloc_8388608",
];

pub fn init_kmalloc() {
    for i in 0..CACHE_INFO_MAX {
        let info = KMALLOC_INFO[i];
        create_mem_cache(info, 1 << (i + 3), 8);
    }
}
pub struct SlabAllocator;

unsafe impl GlobalAlloc for SlabAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = alloc_from_slab(layout);
        match ptr {
            Ok(ptr)=>ptr,
            Err(err)=>panic!("{:?} {:?}",err,layout),
        }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        dealloc_to_slab(ptr,layout).unwrap();
    }
}

unsafe impl Allocator for SlabAllocator{
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        if layout.size() == 0 {
            return Ok(NonNull::slice_from_raw_parts(layout.dangling(), 0));
        }
        match alloc_from_slab(layout) {
            Ok(ptr) => {
                let ptr = NonNull::new(ptr).ok_or(AllocError)?;
                Ok(NonNull::slice_from_raw_parts(ptr, layout.size()))
            }
            Err(_) => Err(AllocError),
        }
    }
    unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        if layout.size() != 0 {
            dealloc_to_slab(ptr.as_ptr(),layout).unwrap();
        }
    }
}