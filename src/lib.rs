#![no_std]
#![allow(unused)]
#[macro_use]
extern crate log;
mod slab;
mod kmalloc;


pub use slab::{mem_cache_init,create_cache,test_slab_system,print_slab_system_info};
pub use kmalloc::{init_kmalloc,SlabAllocator};