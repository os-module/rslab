use log::{Level, LevelFilter, Metadata, Record};
use preprint::Print;
use slab::{create_mem_cache, init_slab_system, print_slab_system_info};
use std::alloc::{alloc, dealloc, Layout};
use std::arch::x86_64::__cpuid;
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
    init_log();
    preprint::init_print(&MPrint);
    init_slab_system(4096, 64);
    let cache = create_mem_cache("my_cache",56,8).unwrap();
    print_slab_system_info();

}
