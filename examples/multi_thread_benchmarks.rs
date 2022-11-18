#![feature(thread_id_value)]
#![feature(allocator_api)]
#![feature(nonnull_slice_from_raw_parts)]
#![feature(alloc_layout_extra)]
#![feature(slice_ptr_get)]

use std::{thread};
use std::alloc::{ Allocator, AllocError, GlobalAlloc, Layout};
use std::collections::BTreeMap;
use std::ptr::{NonNull, null_mut};
use std::thread::sleep;
use std::time::{Instant};
use buddy_system_allocator::LockedHeap;
use core_affinity::CoreId;
use rand::Rng;
use rslab::{init_slab_system, SlabAllocator};
/// This is already enough to fill the corresponding heaps.
const BENCH_DURATION: f64 = 10.0;
/// 160 MiB heap size.
const HEAP_SIZE: usize = 0xa000000;
/// Backing memory for heap management.
static mut HEAP_MEMORY: PageAlignedBytes<HEAP_SIZE> = PageAlignedBytes([0; HEAP_SIZE]);

#[repr(align(4096))]
#[allow(unused)]
#[derive(Copy,Clone)]
struct Page{
    data: [u8; 4096],
}

static mut HEAP_PAGE_MEMORY: [Page; HEAP_SIZE*2 / 4096] = [Page{data: [0; 4096]}; HEAP_SIZE*2 / 4096];
static mut USED_PAGES: usize = 0;
static mut CURRENT_PAGE: usize = 0;


static mut MAP:BTreeMap<u64,CoreId> = BTreeMap::new();

#[no_mangle]
unsafe fn free_frames(_addr: *mut u8, num: usize) {
    USED_PAGES -= num;
}

#[no_mangle]
unsafe fn alloc_frames(num: usize) -> *mut u8 {
    if (USED_PAGES + num) * 4096 > HEAP_SIZE {
        return null_mut()
    }
    USED_PAGES += num;
    let addr = HEAP_PAGE_MEMORY[CURRENT_PAGE].data.as_mut_ptr();
    CURRENT_PAGE += num;
    addr
}

#[no_mangle]
fn current_cpu_id()->usize{
    if unsafe{MAP.is_empty()} {
        return 0
    }
    let current_thread_id = thread::current().id().as_u64().get();
    let id = unsafe{MAP.get(&current_thread_id)};
    if id.is_none() {
        println!("current_thread_id: {:?} not found", current_thread_id);
    }
    let core_id = unsafe{MAP.get(&current_thread_id).unwrap()};
    core_id.id
}

struct BuddyAllocator{
    back:LockedHeap<32>,
}

impl BuddyAllocator{
    pub fn new()->Self{
        BuddyAllocator {
            back: buddy_system_allocator::LockedHeap::empty(),
        }
    }
    pub fn init(&self,start:usize,size:usize){
        unsafe{
            self.back.lock().init(start,size);
        }
    }
}
unsafe impl Allocator for BuddyAllocator{
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        if layout.size() == 0 {
            return Ok(NonNull::slice_from_raw_parts(layout.dangling(), 0));
        }

        match self.back.lock().alloc(layout){
            Ok(ptr) => {
                Ok(NonNull::slice_from_raw_parts(ptr, layout.size()))
            }
            Err(_) => Err(AllocError),
        }
    }
    unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        if layout.size() != 0 {
            self.back.dealloc(ptr.as_ptr(), layout);
        }
    }
}


static ALLOCATOR:BuddyAllocator=BuddyAllocator{back:LockedHeap::empty()};
static SLAB_ALLOCATOR:SlabAllocator = SlabAllocator;

fn main() {
    init_slab_system(4096, 64);
    unsafe{
        ALLOCATOR.init(HEAP_MEMORY.0.as_ptr() as usize, HEAP_SIZE)
    }
    multi_thread_execute();
}


fn multi_thread_execute(){
    let bench_name = "multi_thread_benchmarks_slab";
    let alloc = &ALLOCATOR;
    let core_ids = core_affinity::get_core_ids().unwrap();
    let handles = core_ids[1..].into_iter().map(|&id| {
        thread::spawn(move || {
            core_affinity::set_for_current(id);
            let current_thread_id = thread::current().id().as_u64().get();
            println!("current_thread_id: {:?}, {}", current_thread_id,id.id);
            unsafe{MAP.insert(current_thread_id,id);}
            sleep(std::time::Duration::from_secs(1));


            let now_fn = || unsafe { x86::time::rdtscp().0 };
            let mut all_allocations = Vec::new();
            let mut all_deallocations = Vec::new();
            let mut all_alloc_measurements = Vec::new();

            let powers_of_two = [1, 2, 4, 8, 16, 32, 64];
            let mut rng = rand::thread_rng();
            // run for 10s
            let bench_begin_time = Instant::now();
            while bench_begin_time.elapsed().as_secs_f64() <= BENCH_DURATION {
                let alignment_i = rng.gen_range(0..powers_of_two.len());
                let size = rng.gen_range(8..16384);
                let layout = Layout::from_size_align(size, powers_of_two[alignment_i]).unwrap();
                let alloc_begin = now_fn();
                let alloc_res = alloc.allocate(layout);
                let alloc_ticks = now_fn() - alloc_begin;
                all_alloc_measurements.push(alloc_ticks);
                all_allocations.push(Some((layout, alloc_res)));

                // now free an arbitrary amount again to simulate intense heap usage
                // Every ~10th iteration I free 7 existing allocations; the heap will slowly
                // grow until it is full
                let count_all_allocations_not_freed_yet =
                    all_allocations.iter().filter(|x| x.is_some()).count();
                let count_allocations_to_free =
                    if count_all_allocations_not_freed_yet > 10 && rng.gen_range(0..10) == 0 {
                        7
                    } else {
                        0
                    };

                all_allocations
                    .iter_mut()
                    .filter(|x| x.is_some())
                    // .take() important; so that we don't allocate the same allocation multiple times ;)
                    .map(|x| x.take().unwrap())
                    .filter(|(_, res)| res.is_ok())
                    .map(|(layout, res)| (layout, res.unwrap()))
                    .take(count_allocations_to_free)
                    .for_each(|(layout, allocation)| unsafe {
                        // println!("dealloc: layout={:?}", layout);
                        all_deallocations.push((layout, allocation));
                        alloc.deallocate(allocation.as_non_null_ptr(), layout);
                    });
            }

            // sort
            all_alloc_measurements.sort_by(|x1, x2| x1.cmp(x2));

            let result = BenchRunResults {
                allocation_attempts: all_allocations.len() as _,
                successful_allocations: all_allocations
                    .iter()
                    .filter(|x| x.is_some())
                    .map(|x| x.as_ref().unwrap())
                    .map(|(_layout, res)| res.is_ok())
                    .count() as _,
                deallocations: all_deallocations.len() as _,
                allocation_measurements: all_alloc_measurements,
            };

            print_bench_results(bench_name,&result);
        })
    }).collect::<Vec<_>>();

    for handle in handles.into_iter() {
        handle.join().unwrap();
    }
}


fn print_bench_results(bench_name: &str, res: &BenchRunResults) {
    println!("RESULTS OF BENCHMARK: {bench_name}");
    println!(
        "    {:6} allocations, {:6} successful_allocations, {:6} deallocations",
        res.allocation_attempts, res.successful_allocations, res.deallocations
    );
    println!(
        "    median={:6} ticks, average={:6} ticks, min={:6} ticks, max={:6} ticks",
        res.allocation_measurements[res.allocation_measurements.len() / 2],
        res.allocation_measurements.iter().sum::<u64>()
            / (res.allocation_measurements.len() as u64),
        res.allocation_measurements.iter().min().unwrap(),
        res.allocation_measurements.iter().max().unwrap(),
    );
}

/// Result of a bench run.
struct BenchRunResults {
    /// Number of attempts of allocations.
    allocation_attempts: u64,
    /// Number of successful successful_allocations.
    successful_allocations: u64,
    /// Number of deallocations.
    deallocations: u64,
    /// Sorted vector of the amount of clock ticks per allocation.
    allocation_measurements: Vec<u64>,
}

#[repr(align(4096))]
struct PageAlignedBytes<const N: usize>([u8; N]);