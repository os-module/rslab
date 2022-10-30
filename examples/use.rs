use lazy_static::lazy_static;
use log::{Level, LevelFilter, Metadata, Record};
use preprint::Print;
use rslab::{init_slab_system, Object, ObjectAllocator, print_slab_system_info, SlabCache};
use spin::{Mutex};
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
    alloc(Layout::from_size_align(4096 * num, 4096).unwrap())
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

#[derive(Debug)]
#[allow(unused)]
pub struct Task {
    name: &'static str,
    id: usize,
    inode: Vec<usize>,
    stack: [u8; 128],
}

impl Object for Task {
    fn construct() -> Self {
        Self {
            name: "task",
            id: 190,
            inode: Vec::new(),
            stack: [1; 128],
        }
    }
}

impl Task {
    pub fn new() -> Self {
        Self::construct()
    }
}

impl Drop for Task {
    fn drop(&mut self) {
        println!("drop task");
    }
}

lazy_static! {
    pub static ref TASK_CACHE: Mutex<SlabCache<Task>> = Mutex::new(SlabCache::new("task").unwrap());
}

fn main() {
    init_log();
    preprint::init_print(&MPrint);
    init_slab_system(4096, 64);
    let mut binding = TASK_CACHE.lock();
    let task = binding.alloc().unwrap();
    println!("task: {:?}", task);
    binding.dealloc(task).unwrap();
    binding.print_info();
    print_slab_system_info();
    binding.destroy();
}
