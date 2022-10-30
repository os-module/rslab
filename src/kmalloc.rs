use crate::slab::{alloc_from_slab, create_mem_cache, dealloc_to_slab};
use crate::SLAB_CACHES;
use core::alloc::{GlobalAlloc, Layout};
use doubly_linked_list::*;

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
        let size = layout.size();
        let align = layout.align();
        let ptr = alloc_from_slab(size, align);
        match ptr {
            Ok(ptr)=>ptr,
            Err(err)=>panic!("{:?} {:?}",err,layout),
        }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        dealloc_to_slab(ptr).unwrap();
    }
}
