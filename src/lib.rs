#![feature(allocator_api)]
#![feature(nonnull_slice_from_raw_parts)]
#![feature(alloc_layout_extra)]
#![allow(irrefutable_let_patterns)]
#![no_std]
#![allow(unused)]

#[macro_use]
extern crate log;
extern crate alloc;
mod formation;
mod kmalloc;
mod slab;

use crate::formation::SlabError;
use crate::slab::{create_mem_cache, MemCache, SlabInfo};
use core::marker::PhantomData;
use doubly_linked_list::*;
use preprint::pprintln;

pub use crate::slab::{print_slab_system_info};
pub use kmalloc::SlabAllocator;

/// Cache链表头
static mut SLAB_CACHES: ListHead = ListHead::new();
static mut MEM_CACHE_BOOT: MemCache = MemCache::new();

/// 默认frame_size大小:0x1000 4k
static mut FRAME_SIZE: usize = 0x1000;
/// 默认cache_line_size大小:16
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
    /// 用户需要向slab系统提供获取frame的接口
    fn alloc_frames(num: usize) -> *mut u8;
    /// 用户需要向slab系统提供释放frame的接口
    fn free_frames(addr: *mut u8, num: usize);
    /// 对于多核cpu,需要用户提供一个获取当前cpu id的接口
    fn current_cpu_id() -> usize;
}

/// 初始化slab系统，需要知道frame_size与cache_line_size
pub fn init_slab_system(frame_size: usize, cache_line_size: usize) {
    init_slab_info(frame_size, cache_line_size);
    /// 初始化slab系统的两个基本cache
    slab::mem_cache_init();
    /// 初始化常用的Cache
    kmalloc::init_kmalloc();
}

/// 设置slab系统的基本信息
#[inline]
fn init_slab_info(frame_size: usize, cache_line_size: usize) {
    unsafe {
        FRAME_SIZE = frame_size;
        CACHE_LINE_SIZE = cache_line_size;
    }
}

/// 自定义对象
///
/// 其需要实现一个构造函数，用于初始化分配的内存
pub trait Object {
    fn construct() -> Self;
}

/// 对象分配器接口，用于专门分配用户自定义对象
pub trait ObjectAllocator<T: Object> {
    /// 分配一个对象,返回对象的可变引用，如果分配失败则返回失败原因
    fn alloc(&self) -> Result<&mut T,SlabError>;
    /// 释放一个对象，如果释放失败则返回失败原因
    fn dealloc(&self, obj: &mut T) -> Result<(), SlabError>;
    /// 销毁对象分配器
    fn destroy(&mut self);
}

pub struct SlabCache<T: Object> {
    cache: &'static mut MemCache,
    obj_type: PhantomData<T>,
}

impl<T: Object> SlabCache<T> {
    pub fn new(name: &'static str) -> Result<SlabCache<T>, SlabError> {
        let size = core::mem::size_of::<T>() as u32;
        let align = core::mem::align_of::<T>() as u32;
        let cache = create_mem_cache(name, size, align)?;
        Ok(SlabCache {
            cache,
            obj_type: PhantomData,
        })
    }
    pub fn print_info(&self) {
        self.cache.print_info();
    }
    pub fn get_cache_info(&self)->SlabInfo{
        self.cache.get_cache_info()
    }
}

impl<T: Object> ObjectAllocator<T> for SlabCache<T> {
    fn alloc(&self) -> Result<&mut T,SlabError> {
        let obj_ptr = self.cache.alloc()?;
        unsafe {
            let obj = obj_ptr as *mut T;
            obj.write(T::construct());
            Ok(&mut *obj)
        }
    }
    fn dealloc(&self, obj: &mut T) -> Result<(), SlabError> {
        self.cache.dealloc(obj as *mut T as *mut u8)
    }
    fn destroy(&mut self) {
        self.cache.destroy();
    }
}
