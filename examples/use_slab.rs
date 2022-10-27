use log::{Level, LevelFilter, Metadata, Record};
use preprint::Print;
use rslab::{alloc_from_slab, create_mem_cache, dealloc_to_slab, init_slab_system, print_slab_system_info};
use std::alloc::{alloc, dealloc, Layout};
use std::fmt::Arguments;

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

struct Logger;
impl log::Log for Logger {
    fn enabled(&self, _metadata: &Metadata) -> bool {
        true
    }
    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let color = match record.level() {
            Level::Error => 31, // Red
            Level::Warn => 93,  // BrightYellow
            Level::Info => 34,  // Blue
            Level::Debug => 32, // Green
            Level::Trace => 90, // BrightBlack
        };
        println!(
            "\u{1B}[{}m[{:>5}] {}\u{1B}[0m",
            color,
            record.level(),
            record.args(),
        );
    }
    fn flush(&self) {}
}

fn init_log() {
    log::set_logger(&Logger).unwrap();
    log::set_max_level(match option_env!("LOG") {
        Some("ERROR") => LevelFilter::Error,
        Some("WARN") => LevelFilter::Warn,
        Some("INFO") => LevelFilter::Info,
        Some("DEBUG") => LevelFilter::Debug,
        Some("TRACE") => LevelFilter::Trace,
        _ => LevelFilter::Off,
    });
}



fn main() {
    // There are some log information in rslab system
    init_log();
    // If you want to print rslab usage, you need to initialize this trait object
    preprint::init_print(&MPrint);
    init_slab_system(4096, 64);
    // create your own cache
    let cache = create_mem_cache("my_cache", 56, 8).unwrap();
    let cache1 = create_mem_cache("my_cache", 56, 8);
    assert!(cache1.is_err()); // cache name already exists
    // alloc from your cache
    let ptr = cache.alloc();
    println!("ptr: {:p}", ptr);
    let ptr1 = cache.alloc();
    println!("ptr1: {:p}", ptr1);
    println!("ptr1 - ptr: {}", ptr as usize - ptr1 as usize);
    print_slab_system_info();
    cache.dealloc(ptr).unwrap();
    cache.dealloc(ptr1).unwrap();
    print_slab_system_info();
    // destruct your cache
    cache.destroy();
    // if use cache after destroy, it will panic
    // cache.alloc();
    let ptr = alloc_from_slab(56, 8);
    print_slab_system_info();
    let ptr = ptr.unwrap();
    dealloc_to_slab(ptr).unwrap();
    print_slab_system_info();
    // if you dealloc a ptr which is not from rslab, it will return error
    let error = dealloc_to_slab(123123123 as *mut u8);
    assert!(error.is_err());
}
