use std::alloc::{alloc, dealloc, Layout};

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
