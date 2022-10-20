#![allow(irrefutable_let_patterns)]
#![no_std]
#![allow(unused)]
#[macro_use]
extern crate log;
extern crate alloc;

use doubly_linked_list::*;
mod formation;
mod kmalloc;
mod slab;
use slab::*;

pub use kmalloc::SlabAllocator;
pub use slab::{
    alloc_from_slab, create_mem_cache, dealloc_to_slab, print_slab_system_info,
    reclaim_frame_from_cache,MemCache
};

// 管理所有的list_head链表
static mut SLAB_CACHES: ListHead = ListHead::new();
static mut MEM_CACHE_BOOT: MemCache = MemCache::new();

/// default:0x1000 4k
static mut FRAME_SIZE: usize = 0x1000;
// 缓存行大小
static mut CACHE_LINE_SIZE: usize = 16;

#[inline]
fn frame_size() -> usize {
    unsafe { FRAME_SIZE }
}

#[inline]
fn cls() -> usize {
    unsafe { CACHE_LINE_SIZE }
}

extern "C" {
    fn alloc_frames(num: usize) -> *mut u8;
    fn free_frames(addr: *mut u8, num: usize);
}

pub fn init_slab_system(frame_size: usize, cache_line_size: usize) {
    init_slab_info(frame_size, cache_line_size);
    mem_cache_init(); //初始化第一个Cache
    kmalloc::init_kmalloc(); //初始化常用的Cache
}

/// it should be called before any other function
fn init_slab_info(frame_size: usize, cache_line_size: usize) {
    trace!(
        "init_slab frame_size:{:#x} cache_line_size:{:#x}",
        frame_size,
        cache_line_size
    );
    unsafe {
        FRAME_SIZE = frame_size;
        CACHE_LINE_SIZE = cache_line_size;
    }
}
