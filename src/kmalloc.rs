use crate::SLAB_CACHES;
use crate::{alloc_from_slab, create_mem_cache, dealloc_to_slab, print_slab_system_info};
use core::alloc::{GlobalAlloc, Layout};
use doubly_linked_list::*;
use spin::Mutex;

struct CacheInfo {
    size: u32,
    align: u32,
    name: &'static str,
}

impl CacheInfo {
    pub const fn new(size: u32, align: u32, name: &'static str) -> Self {
        CacheInfo { size, align, name }
    }
}

const CACHE_INFO_MAX: usize = 21;
const KMALLOC_INFO: [CacheInfo; CACHE_INFO_MAX] = [
    CacheInfo::new(8, 8, "malloc-8"),
    CacheInfo::new(16, 8, "malloc-16"),
    CacheInfo::new(32, 8, "malloc-32"),
    CacheInfo::new(64, 8, "malloc-64"),
    CacheInfo::new(128, 8, "malloc-128"),
    CacheInfo::new(256, 8, "malloc-256"),
    CacheInfo::new(512, 8, "malloc-512"),
    CacheInfo::new(1024, 8, "malloc-1024"),
    CacheInfo::new(2048, 8, "malloc-2048"),
    CacheInfo::new(4096, 8, "malloc-4096"),
    CacheInfo::new(8192, 8, "malloc-8192"),
    CacheInfo::new(4 * 4096, 8, "malloc_16384"),
    CacheInfo::new(8 * 4096, 8, "malloc_32768"),
    CacheInfo::new(16 * 4096, 8, "malloc_65536"),
    CacheInfo::new(32 * 4096, 8, "malloc_131072"),
    CacheInfo::new(64 * 4096, 8, "malloc_262144"),
    CacheInfo::new(128 * 4096, 8, "malloc_524288"),
    CacheInfo::new(256 * 4096, 8, "malloc_1048576"),
    CacheInfo::new(512 * 4096, 8, "malloc_2097152"),
    CacheInfo::new(1024 * 4096, 8, "malloc_4194304"),
    CacheInfo::new(2048 * 4096, 8, "malloc_8388608"),
];

pub fn init_kmalloc() {
    for i in 0..CACHE_INFO_MAX {
        let info = &KMALLOC_INFO[i];
        create_mem_cache(info.name, info.size, info.align);
    }
}

pub struct SlabAllocator {
    cache: Mutex<u8>,
}

impl SlabAllocator {
    pub const fn new() -> Self {
        SlabAllocator {
            cache: Mutex::new(0),
        }
    }
}

unsafe impl GlobalAlloc for SlabAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let size = layout.size();
        let align = layout.align();
        let _ = self.cache.lock();
        let ptr = alloc_from_slab(size, align);
        if ptr.is_some() {
            ptr.unwrap()
        } else {
            panic!("{:?}", layout);
        }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        dealloc_to_slab(ptr);
    }
}
