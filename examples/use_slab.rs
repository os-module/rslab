use preprint::Print;
use std::alloc::{alloc, dealloc, GlobalAlloc, Layout};
use std::fmt::Arguments;
use rslab::{init_slab_system, Object, ObjectAllocator, print_slab_system_info, SlabAllocator, SlabCache};

#[no_mangle]
unsafe fn free_frames(addr: *mut u8, num: usize) {
    dealloc(addr, Layout::from_size_align(num * 4096, 4096).unwrap());
}
#[no_mangle]
fn current_cpu_id() -> usize {
    0
}
#[no_mangle]
unsafe fn alloc_frames(num: usize) -> *mut u8 {
    let addr = alloc(Layout::from_size_align(4096 * num, 4096).unwrap());
    addr
}

struct MPrint;

impl Print for MPrint {
    fn print(&self, args: Arguments) {
        print!("{}", args);
    }
}

#[allow(unused)]
struct TestObj {
    a: [u8;56],
}
impl Object for TestObj{
    fn construct() -> Self {
        Self{
            a:[0;56]
        }
    }
}

fn main() {
    // If you want to print rslab usage, you need to initialize this trait object
    preprint::init_print(&MPrint);
    init_slab_system(4096, 64);
    use_your_cache();
    unsafe {
        use_common_cache();
    }
}
fn use_your_cache() {
    // create your own cache
    let mut cache = SlabCache::<TestObj>::new("mycache").unwrap();
    // alloc from your cache
    let ptr = cache.alloc().unwrap();
    println!("ptr: {:p}", ptr);
    let ptr1 = cache.alloc().unwrap();
    println!("ptr1: {:p}", ptr1);
    println!("ptr1 - ptr: {}", ptr as *const _ as usize - ptr1 as *const _ as usize);
    print_slab_system_info();
    cache.dealloc(ptr).unwrap();
    cache.dealloc(ptr1).unwrap();
    print_slab_system_info();
    // destruct your cache
    cache.destroy();
    // if use cache after destroy, it will panic
    // cache.alloc().unwrap();
}

unsafe fn use_common_cache(){
    let slab_allocator = SlabAllocator;
    let layout1 = Layout::from_size_align(4096, 4096).unwrap();
    let addr1 = slab_allocator.alloc(layout1);
    println!("{:p}",addr1);
    let layout2 = Layout::from_size_align(34, 8).unwrap();
    let addr2 = slab_allocator.alloc(layout2);
    println!("{:p}",addr2);

    print_slab_system_info();

    slab_allocator.dealloc(addr2, layout2);
    slab_allocator.dealloc(addr1, layout1);
    print_slab_system_info();
    let addr3 = slab_allocator.alloc(layout1);
    assert_eq!(addr1,addr3);
    let addr4 = slab_allocator.alloc(layout2);
    assert_eq!(addr2,addr4);
    slab_allocator.dealloc(addr4, layout2);
    slab_allocator.dealloc(addr3, layout1);
}