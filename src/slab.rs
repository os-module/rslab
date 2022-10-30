use super::alloc_frames;
use crate::formation::*;
use crate::{cls, frame_size, free_frames, MEM_CACHE_BOOT};
use crate::{current_cpu_id, SLAB_CACHES};
use alloc::alloc::dealloc;
use bitflags::bitflags;
use core::cmp::{max, min};
use core::fmt::{Debug, Formatter, Write};
use core::mem::forget;
use core::ops::Add;
use core::sync::atomic::AtomicUsize;
use doubly_linked_list::*;
use preprint::pprintln;
use spin::mutex::SpinMutex;
use spin::rwlock::RwLock;
use spin::Mutex;

/// 高速缓存的limit
const PER_CPU_OBJECTS: usize = 16;
/// 支持8个核
const CPUS: usize = 8;
/// 空闲链表的上限，达到上线将触发回收页面
const FREE_LIST_MAX:usize = 16;

const VAL: ArrayCache = ArrayCache::new();
static mut ARRAY_CACHE_FOR_BOOT: [ArrayCache; CPUS] = [VAL; CPUS];
static mut ARRAY_CACHE_FOR_ARRAY: [ArrayCache; CPUS] = [VAL; CPUS];
static mut ARRAY_CACHE_NODE_BOOT: ArrayCache = ArrayCache::new();
static mut ARRAY_CACHE_NODE_ARRAY: ArrayCache = ArrayCache::new();



bitflags! {
    pub struct Flags:u8{
        const SLAB_OFF = 0b0000_0000;
        const SLAB_ON = 0b0000_0001;
        const DESTROY = 0b0000_0010;
    }
}

pub struct SlabInfo {
    pub cache_name: &'static str,
    pub object_size: u32,
    pub align: u32,
    pub per_frames: u32,
    pub per_objects: u32,
    pub total_objects: u32,
    pub used_objects: u32,
    pub limit: u32,
    pub batch_count: u32,
    pub local_objects: u32,
    pub shared_objects: u32,
}

#[derive(Debug)]
pub struct MemCache {
    /// 本地高速缓存
    array_cache: [*mut ArrayCache; CPUS as usize],
    list: ListHead,
    /// 每个slab的对象数量
    per_objects: u32,
    /// 每个slab的页帧数量 2^per_frames
    per_frames: u32,
    /// 对象对齐大小
    align: u32,
    /// 对象大小
    object_size: u32,
    /// 可着色数量
    color: u32,
    /// 着色偏移==缓存行大小
    color_off: u32,
    /// 下一个着色位置
    color_next: u32,
    /// Slab管理节点
    mem_cache_node: CacheNode,
    /// Cache名称
    cache_name: &'static str,
    /// 控制信息
    flags: Flags,
}
unsafe impl Sync for MemCache {}
unsafe impl Send for MemCache {}

macro_rules! mut_ref_memcache {
    ($addr:expr) => {
        unsafe { &mut (*container_of!($addr as usize, MemCache, list)) }
    };
}
macro_rules! ref_memcache {
    ($addr:expr) => {
        unsafe { &(*container_of!($addr as usize, MemCache, list)) }
    };
}

macro_rules! mut_ref_slab {
    ($addr:expr) => {
        unsafe { &mut (*container_of!($addr as usize, Slab, list)) }
    };
}
macro_rules! ref_slab {
    ($addr:expr) => {
        unsafe { &(*container_of!($addr as usize, Slab, list)) }
    };
}


impl MemCache {
    pub const fn new() -> Self {
        Self {
            array_cache: [core::ptr::null_mut(); CPUS as usize],
            list: ListHead::new(),
            per_objects: 0,
            per_frames: 0,
            align: 0,
            object_size: 0,
            color: 0,
            color_off: 0,
            color_next: 0,
            mem_cache_node: CacheNode::new(),
            cache_name: "",
            flags: Flags::empty(),
        }
    }
    /// 打印信息
    pub fn print_info(&self) {
        let slab_info = self.get_cache_info();
        pprintln!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            self.cache_name,
            self.object_size,
            self.align,
            self.per_frames,
            self.per_objects,
            slab_info.total_objects,
            slab_info.used_objects,
            PER_CPU_OBJECTS,
            PER_CPU_OBJECTS / 2,
            slab_info.local_objects,
            slab_info.shared_objects
        );
    }
    pub fn get_cache_info(&self) -> SlabInfo {
        // 计算总的对象和已使用的对象
        let per_objects = self.per_objects as usize;
        let total = self.mem_cache_node.total_slabs() * per_objects;
        let used = self.mem_cache_node.used_objects(per_objects);
        // 计算本地高速缓存的对象数量
        let mut local = 0;
        for i in 0..CPUS {
            local += unsafe { (*self.array_cache[i]).inner.lock().avail };
        }
        //计算共享高速缓存的对象数量
        let shared = unsafe { (*self.mem_cache_node.shared).inner.lock().avail };
        assert!(used as u32 >= local + shared);
        SlabInfo {
            cache_name: self.cache_name,
            object_size: self.object_size,
            align: self.align,
            per_frames: self.per_frames,
            per_objects: self.per_objects,
            total_objects: total as u32,
            used_objects: used as u32 - shared - local,
            limit: PER_CPU_OBJECTS as u32,
            batch_count: PER_CPU_OBJECTS as u32 / 2,
            local_objects: local,
            shared_objects: shared,
        }
    }

    /// 需要根据对象大小和对齐方式计算出
    /// 需要的页面数量，确保内部碎片 < 12.5%
    /// 再计算每个slab中对象的数量
    fn init_cache_object_num(&mut self) {
        let mut order = 0;
        let mut left_over = 0;
        loop {
            let total_size = frame_size() * (1 << order);
            let object_num = if self.flags == Flags::SLAB_OFF {
                // slab描述符和freelist数组在外部
                total_size / self.object_size as usize
            } else {
                // slab描述符和freelist数组在内部
                let mut object_num = (total_size - core::mem::size_of::<Slab>())
                    / (self.object_size as usize + core::mem::size_of::<u32>());
                // 计算对齐后slab描述符的大小
                while let slab_align = slab_descriptor_align_size(object_num as u32, self.align) {
                    if (slab_align + object_num as u32 * self.object_size) < total_size as u32 {
                        break;
                    }
                    object_num -= 1;
                } //找到正确的对象数量，一般是需要运行一次即可
                object_num
            };
            //检查内部碎片的比例
            left_over = total_size - object_num * self.object_size as usize;
            if self.flags == Flags::SLAB_ON {
                left_over -= slab_descriptor_align_size(object_num as u32, self.align) as usize;
            }
            if left_over * 8 < total_size {
                self.per_objects = object_num as u32;
                self.per_frames = order;
                //初始化可着色的数量
                self.color = (left_over / cls()) as u32;
                break;
            } // 找到页帧正确的数量
            order += 1;
        }
        trace!(
            "left_over is {}, total_size is {}",
            left_over,
            frame_size() * (1 << self.per_frames)
        );
    }

    /// 在使用init初始化cache后需要使用此函数完成array_cache的初始化
    /// 对于系统初始化阶段的两个初始cache不经过这里
    fn set_array_cache(&mut self) -> Result<(), SlabError> {
        //从array_cache中分配得到
        for i in 0..CPUS {
            let array_cache_addr = get_array_cache()?;
            self.array_cache[i] = array_cache_addr as *mut ArrayCache;
            unsafe { (*self.array_cache[i]).inner.lock().init() };
        }
        self.mem_cache_node.set_array_cache()?;
        Ok(())
    }

    fn init(&mut self, name: &'static str, object_size: u32, align: u32) -> Result<(), SlabError> {
        self.array_cache = [core::ptr::null_mut(); CPUS];
        self.mem_cache_node.init();
        self.cache_name = name;
        self.color_off = cls() as u32; //cache行大小
        self.align = if align.is_power_of_two() && align != 0 {
            max(align, 8)
        } else {
            core::mem::size_of::<usize>() as u32
        };
        // 对象大小对齐到align
        self.object_size = align_to!(object_size, self.align);
        self.flags = if object_size * 8 >= frame_size() as u32 {
            Flags::SLAB_OFF
        } else {
            Flags::SLAB_ON
        };
        // 分配的物理页帧起始位置由
        // slab结构体 + free_list数组构成
        // 第一个对象的地址需要对齐到align
        self.init_cache_object_num();
        Ok(())
    }
    pub fn alloc(&self) -> Result<*mut u8,SlabError> {
        if self.flags.contains(Flags::DESTROY) {
            panic!("cache had been destroyed");
        }
        /// 先从高速缓存分配
        ///
        /// todo!(多cpu访问一致性保证 ?)
        ///
        /// 如果一个cpu上的线程正在分配内存并且以及获取了cpu_id，此时其再被抢占放到另一个cpu上可能会发生错误?
        let cpu_id = unsafe { current_cpu_id() };
        let array_cache = unsafe { &mut *self.array_cache[cpu_id] };
        let mut array_cache = array_cache.inner.lock();
        if array_cache.is_empty() {
            let mut new_objects = [0usize; PER_CPU_OBJECTS];
            let mem_cache_ptr = self as *const MemCache as *mut MemCache;
            self.mem_cache_node
                .alloc(mem_cache_ptr, &mut new_objects[0..array_cache.batch_count as usize])?;
            let batch_count = array_cache.batch_count as usize;
            array_cache.push(&new_objects[0..batch_count]);
        }
        Ok(array_cache.get())
    }

    pub fn dealloc(&self, addr: *mut u8) -> Result<(), SlabError> {
        if self.flags.contains(Flags::DESTROY) {
            panic!("cache had been destroyed");
        }
        /// 判断此地址是否属于此cache
        self.mem_cache_node.is_in_cache(addr)?;
        let cpu_id = unsafe { current_cpu_id() };
        let array_cache = unsafe { &mut *self.array_cache[cpu_id] };
        let mut array_cache = array_cache.inner.lock();
        if array_cache.is_full() {
            let mut objects = [0usize; PER_CPU_OBJECTS];
            let batch_count = array_cache.batch_count as usize;
            array_cache.pop(&mut objects[0..batch_count]);
            self.mem_cache_node.dealloc(&objects[0..batch_count]);
        }
        array_cache.put(addr);
        Ok(())
    }
    /// 调用destroy会将cache管理的所有slab回收掉。
    /// 包括free/partial/full
    /// 并且对于cache本身不再可用，
    /// 由于cache本身的地址仍然会是有效的，使用者可能会再次使用已经destroy的
    /// cache分配内存，以此需要设置标志防止其再使用
    pub fn destroy(&mut self) {
        // 先把高速缓存的内存回收
        for i in 0..CPUS {
            let array_cache = self.array_cache[i];
            dealloc_to_slab(array_cache as *mut u8);
        }
        self.mem_cache_node.destroy();
        //回收掉自己
        let addr = self as *const Self as *mut u8;
        self.flags = Flags::DESTROY;
        list_del!(to_list_head_ptr!(self.list));
        dealloc_to_slab(addr);
    }
}

#[inline]
fn slab_descriptor_align_size(object_num: u32, align: u32) -> u32 {
    align_to!(
        object_num * core::mem::size_of::<u32>() as u32 + core::mem::size_of::<Slab>() as u32,
        align
    )
}

/// array_cache define\
/// target: for multicore\
/// limit: 可以拥有的最大对象数量\
/// batch_count: 每次从shared或者slab系统获取的对象\
/// entries: object address\
/// 为了缓存命中率更高，
/// 取对象的时候从后往前取，放对象的时候从前往后放
struct ArrayCache {
    inner: Mutex<ArrayCacheInner>,
}

impl ArrayCache {
    const fn new() -> Self {
        Self {
            inner: Mutex::new(ArrayCacheInner::new()),
        }
    }
}

struct ArrayCacheInner {
    avail: u32,
    limit: u32,
    batch_count: u32,
    entries: [usize; PER_CPU_OBJECTS as usize],
}

impl ArrayCacheInner {
    const fn new() -> Self {
        Self {
            avail: 0,
            limit: PER_CPU_OBJECTS as u32,
            batch_count: PER_CPU_OBJECTS as u32 / 2,
            entries: [0; PER_CPU_OBJECTS],
        }
    }
    #[inline]
    fn init(&mut self) {
        *self = Self::new();
    }

    /// 需要保证
    fn push(&mut self, addrs: &[usize]) {
        //从下一层获取的batch_count个对象
        //放到array_cache中
        assert!(addrs.len() <= self.batch_count as usize);
        assert!(addrs.len() + self.avail as usize <= self.limit as usize);
        for i in 0..self.batch_count as usize {
            self.entries[self.avail as usize + i] = addrs[i];
        }
        self.avail += self.batch_count;
    }
    fn pop_back(&mut self, addrs: &mut [usize]) {
        assert_eq!(addrs.len(), self.batch_count as usize);
        assert!(self.avail >= self.batch_count);
        //从后往前取
        let begin = self.avail - self.batch_count;
        for i in 0..self.batch_count as usize {
            addrs[i] = self.entries[begin as usize + i];
        }
        self.avail -= self.batch_count;
    }
    fn pop(&mut self, addrs: &mut [usize]) {
        //从本层往下一层回收的batch_count个对象
        //从前往后取
        assert_eq!(addrs.len(), self.batch_count as usize);
        assert_eq!(self.avail, self.limit);
        for i in 0..self.batch_count as usize {
            addrs[i] = self.entries[i];
        }
        //从前往后取，所以后面的对象往前移动
        for i in self.batch_count as usize..self.avail as usize {
            self.entries[i - self.batch_count as usize] = self.entries[i];
        }
        self.avail -= self.batch_count;
    }

    /// 需要调用者保证存在可用的对象
    #[inline]
    fn get(&mut self) -> *mut u8 {
        //从本层获取一个对象
        assert!(self.avail > 0);
        let t = self.entries[self.avail as usize - 1] as *mut u8;
        self.avail -= 1;
        t
    }
    #[inline]
    fn put(&mut self, addr: *mut u8) {
        //往本层放一个对象
        assert!(self.avail < self.limit);
        self.entries[self.avail as usize] = addr as usize;
        self.avail += 1;
    }
    #[inline]
    fn is_empty(&self) -> bool {
        self.avail == 0
    }
    #[inline]
    fn is_full(&self) -> bool {
        self.avail == self.limit
    }
}

/// Cache Node define\
/// slab_partial: 部分分配链表\
/// slab_free: 空Slab/未分配\
/// slab_full: 完全分配\
#[derive(Debug)]
pub struct CacheNode {
    shared: *mut ArrayCache,
    slab_partial: ListHead,
    slab_free: ListHead,
    slab_full: ListHead,
    free_list_len: RwLock<u32>,
}

impl CacheNode {
    const fn new() -> Self {
        CacheNode {
            shared: core::ptr::null_mut(),
            slab_partial: ListHead::new(),
            slab_free: ListHead::new(),
            slab_full: ListHead::new(),
            free_list_len: RwLock::new(0),
        }
    }
}

impl CacheNode {
    fn init(&mut self) {
        self.shared = core::ptr::null_mut();
        list_head_init!(self.slab_partial);
        list_head_init!(self.slab_free);
        list_head_init!(self.slab_full);
    }

    fn set_array_cache(&mut self) -> Result<(), SlabError> {
        //从array_cache中分配得到
        let array_cache_addr = get_array_cache()?;
        self.shared = array_cache_addr as *mut ArrayCache;
        unsafe { (*self.shared).inner.lock().init() };
        Ok(())
    }

    fn alloc_inner(&self, cache: *mut MemCache) -> Result<*mut u8,SlabError> {
        let cache = unsafe{&mut *cache};
        // 先检查partial链表
        let mut slab_list = to_list_head_ptr!(self.slab_partial);
        let slab = if !is_list_empty!(slab_list) {
            // 非空则从slab中分配
            slab_list = self.slab_partial.next; //第一个可用slab
            let slab = mut_ref_slab!(slab_list);
            slab
        } else if is_list_empty!(to_list_head_ptr!(self.slab_free)) {
            // 如果partial链表为空，则检查free链表
            // 如果free链表也为空，则需要分配新的slab
            trace!("alloc new rslab");
            Slab::new(cache)?; // 创建新的slab,并加入到cache的free链表中
            assert!(!is_list_empty!(to_list_head_ptr!(self.slab_free)));
            // 第一个可用slab
            let slab = mut_ref_slab!( self.slab_free.next);
            slab.move_to(to_list_head_ptr!(self.slab_partial));
            slab
        } else {
            // 如果free链表不为空，则将free链表中的slab移动到partial链表中
            slab_list = self.slab_free.next;
            let slab = mut_ref_slab!(slab_list);
            // 将slab移动到partial部分
            slab.move_to(to_list_head_ptr!(self.slab_partial));
            // 空闲链表数量减少
            *self.free_list_len.write() -= 1;
            slab
        };
        // 从slab中分配
        let addr = slab.alloc();
        if slab.used_object == cache.per_objects {
            // 如果slab中的对象已经全部分配完毕，则将slab移动到full链表中
            slab.move_to(to_list_head_ptr!(self.slab_full));
        }
        Ok(addr)
    }

    fn alloc(&self, cache: *mut MemCache, addrs: &mut [usize])->Result<(),SlabError> {
        // 检查共享的本地高速缓存是否有足够的对象
        let shared_array = unsafe { &mut *self.shared };
        let mut shared_array = shared_array.inner.lock();
        if shared_array.avail >= addrs.len() as u32 {
            // 从共享的本地高速缓存中获取对象
            shared_array.pop_back(addrs);
        } else {
            // 按批次从slab中分配过来
            // 直接返回给上一层的请求
            for i in 0..shared_array.batch_count as usize {
                let addr_inner = self.alloc_inner(cache)?;
                addrs[i] = addr_inner as usize;
            }
        }
        Ok(())
    }

    fn is_in_cache(&self, addr: *mut u8) -> Result<&mut Slab,SlabError> {
        // 查找此对象所在的slab
        // 这个地址可能位于partial / full
        let slab_list = self.slab_partial.iter().find(|&slab_list| {
            let slab = mut_ref_slab!(slab_list);
            slab.is_in_slab(addr)
        });
        if slab_list.is_some() {
            return Ok(mut_ref_slab!(slab_list.unwrap()) );
        }
        let slab_list = self.slab_full.iter().find(|&slab_list| {
            let slab = mut_ref_slab!(slab_list) ;
            slab.is_in_slab(addr)
        });
        if slab_list.is_some() {
            return Ok(mut_ref_slab!(slab_list.unwrap()));
        }
        Err(SlabError::NotInCache)
    }
    fn dealloc_inner(&self, addr: *mut u8) {
        // 查找此对象所在的slab
        // 这个地址可能位于partial / full
        let slab = self.is_in_cache(addr).unwrap();
        slab.dealloc(addr);
        if slab.used_object == 0 {
            // 如果slab中的对象已经全部释放，则将slab移动到free链表中
            slab.move_to(to_list_head_ptr!(self.slab_free));
            *self.free_list_len.write() +=1;
            // 检查是否需要释放slab回收页帧
            self.check_and_reclaim();
        } else {
            slab.move_to(to_list_head_ptr!(self.slab_partial));
        }
    }
    /// 检查空闲的slab是否超过了最大值
    /// 如果超过了，则释放多余的slab
    fn check_and_reclaim(&self){
        let mut free_len = self.free_list_len.write();
        if *free_len > FREE_LIST_MAX as u32 {
            // 如果超过了最大值，则释放一部分
            self.slab_free.iter().take(*free_len as usize - FREE_LIST_MAX).for_each(|slab_list|{
                let slab = mut_ref_slab!(slab_list);
                slab.reclaim();
                list_del!(slab_list);
            });
            *free_len = FREE_LIST_MAX as u32;
        }
    }
    fn dealloc(&self, addrs: &[usize]) {
        let shared_array = unsafe { &mut *self.shared };
        let mut shared_array = shared_array.inner.lock();
        if shared_array.is_full() {
            // 如果共享的本地高速缓存已经满了,
            // 将缓存中旧的对象释放
            let mut temp = [0usize; PER_CPU_OBJECTS];
            let batch_count = shared_array.batch_count as usize;
            shared_array.pop(&mut temp[0..batch_count]);
            for i in 0..shared_array.batch_count as usize {
                self.dealloc_inner(temp[i] as *mut u8);
            }
        }
        // 如果共享的本地高速缓存没有满，则将对象放入共享的本地高速缓存中
        shared_array.push(addrs);
    }

    fn total_slabs(&self) -> usize {
        self.slab_partial.len() + self.slab_full.len() + self.slab_free.len()
    }
    fn used_objects(&self, per_objects: usize) -> usize {
        self.slab_partial
            .iter()
            .map(|slab_list|  {
                mut_ref_slab!(slab_list).used_object as usize
            })
            .sum::<usize>()
            + self.slab_full.len() * per_objects
    }
    fn destroy(&self) {
        //回收本地共享高速缓存
        let shared = self.shared;
        dealloc_to_slab(shared as *mut u8);
        self.slab_partial.iter().for_each(|slab_list| {
            let slab = mut_ref_slab!(slab_list);
            slab.reclaim();
            // 从slab_partial链表中移除
            list_del!(slab_list);
        });
        self.slab_full.iter().for_each(|slab_list| {
            let slab = mut_ref_slab!(slab_list);
            slab.reclaim();
            // 从slab_full链表中移除
            list_del!(slab_list);
        });
        self.slab_free.iter().for_each(|slab_list| {
            let slab = mut_ref_slab!(slab_list);
            slab.reclaim();
            // 从slab_free链表中移除
            list_del!(slab_list);
        });
    }
}

/// Slab define\
/// cache: 指向所属的Cache\
/// used_object：已分配的对象数量\
/// next_free: 下一个空闲的对象\
/// first_object: 第一个对象的地址\
/// free_list: 数组索引用来记录空闲的对象\
pub struct Slab {
    list: ListHead,
    cache: *mut MemCache,
    used_object: u32,
    next_free: u32,
    color_off: u32,
    fist_object: usize,
    free_list: *mut u32,
}

impl Debug for Slab {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        f.write_fmt(format_args!(
            "Slab{{\n\
            \tlist:{:?},\n\
            \tcache:{:?},\n\
            \tused_object:{},\n\
            \tnext_free:{},\n\
            \tfist_object:{:#x},\n\
            \tfree_list:{:?}\
            }}",
            self.list,
            self.cache,
            self.used_object,
            self.next_free,
            self.fist_object,
            self.free_list
        ))
    }
}

impl Slab {
    fn new(cache: &MemCache)->Result<(),SlabError> {
        // 创建一个slab
        // 从cache获取需要申请的页面和对象大小
        // 申请页面
        // 初始化slab
        // 将slab添加到cache的slab_partial链表中
        let per_frames = cache.per_frames;
        let start_addr = alloc_frames_for_cache(1 << per_frames);
        if start_addr.is_null(){
            return Err(SlabError::CantAllocFrame);
        }
        let start_addr = start_addr as usize;
        let mut slab_desc_align_size = 0; //确定slab描述符对齐后大小
        if cache.flags == Flags::SLAB_ON {
            slab_desc_align_size = slab_descriptor_align_size(cache.per_objects, cache.align);
        }
        let mut first_object_addr = start_addr.add(slab_desc_align_size as usize);
        //需要根据cache的着色偏移来调整
        first_object_addr += cache.color_off as usize * cache.color_next as usize;

        let (slab_ptr, free_list_addr) = if cache.flags == Flags::SLAB_ON {
            (start_addr, start_addr.add(core::mem::size_of::<Slab>()))
        } else {
            //从外面分配对象来保存slab描述符以及free_list
            let free_list_ptr = alloc_from_slab(
                cache.per_objects as usize * core::mem::size_of::<u32>() as usize,
                8,
            )?;
            let slab_ptr =
                alloc_from_slab(core::mem::size_of::<Slab>(), core::mem::align_of::<Slab>())?;
            (slab_ptr as usize, free_list_ptr as usize)
        };
        let slab = Slab {
            list: ListHead::new(),
            cache: cache as *const MemCache as *mut MemCache,
            used_object: 0,
            next_free: 0,
            color_off: cache.color_next,
            fist_object: first_object_addr as usize,
            free_list: free_list_addr as *mut u32,
        };
        // 写入slab信息到开始位置
        unsafe {
            core::ptr::write(slab_ptr as *mut Slab, slab);
            // 初始化free_list
            for i in 0..cache.per_objects {
                core::ptr::write(
                    free_list_addr.add(i as usize * core::mem::size_of::<u32>()) as *mut u32,
                    i,
                );
            }
        }
        let slab = unsafe { &mut *(slab_ptr as *mut Slab) };
        list_head_init!(slab.list);
        trace!("{:?}", slab);
        // 加入到cache的slab_free链表中
        list_add_tail!(
            to_list_head_ptr!(slab.list),
            to_list_head_ptr!(cache.mem_cache_node.slab_free)
        );
        let mut cache = slab.cache;
        unsafe {
            if (*cache).color_next == (*cache).color {
                (*cache).color_next = 0; //从0开始新的循环
            } //更新 cache的着色偏移
        }
        Ok(())
    }

    fn alloc(&mut self) -> *mut u8 {
        let cache = unsafe { &mut *self.cache };
        let per_objects = cache.per_objects;
        if self.next_free < per_objects {
            let pos = unsafe { self.free_list.add(self.next_free as usize).read_volatile() };
            let addr = self
                .fist_object
                .add(pos as usize * cache.object_size as usize);
            self.next_free += 1;
            self.used_object += 1;
            return addr as *mut u8;
        }
        core::ptr::null_mut()
    }
    fn dealloc(&mut self, addr: *mut u8) {
        let cache = unsafe { &mut *self.cache };
        let pos = (addr as usize - self.fist_object) / cache.object_size as usize;
        self.next_free -= 1;
        unsafe {
            self.free_list
                .add(self.next_free as usize)
                .write_volatile(pos as u32);
        }
        self.used_object -= 1;
        trace!(
            "rslab dealloc {:?}, object_size is {}, used: {}",
            addr,
            cache.object_size,
            self.used_object
        );
    }
    fn reclaim(&self) {
        // 回收自己的页面
        // 如果是SLAB_ON,则正常释放内存即可
        // 如果是SLAB_OFF,则需要释放slab描述符和free_list
        let cache = unsafe { &mut *self.cache };
        let per_frames = cache.per_frames;
        if cache.flags == Flags::SLAB_OFF {
            //释放slab描述符和free_list
            dealloc_to_slab(self as *const Slab as *mut u8);
            dealloc_to_slab(self.free_list as *mut u8);
        }
        unsafe {
            free_frames(self.start() as *const Slab as *mut u8, 1 << per_frames);
        }
    }
    fn start(&self) -> usize {
        // 返回slab页面起始地址
        let cache = unsafe { &mut *self.cache };
        if cache.flags == Flags::SLAB_ON {
            self as *const Slab as usize
        } else {
            self.fist_object
        }
    }

    #[inline]
    fn move_to(&mut self, to: *mut ListHead) {
        list_del!(to_list_head_ptr!(self.list));
        list_add_tail!(to_list_head_ptr!(self.list), to);
    }
    fn is_in_slab(&self, addr: *mut u8) -> bool {
        //检查此地址是否位于slab中
        let addr = addr as usize;
        let cache = unsafe { &mut *self.cache };
        let start_addr = self.start();
        let end_addr = start_addr.add((1 << cache.per_frames as usize) * unsafe { frame_size() });
        (start_addr <= addr) && (addr < end_addr)
    }
}

/// 请求num个frame
fn alloc_frames_for_cache(num: u32) -> *mut u8 {
    trace!("alloc {} frames for cache", num);
    unsafe { alloc_frames(num as usize) }
}

fn get_array_cache()->Result<*mut u8,SlabError>{
    let cache_head = unsafe { &mut MEM_CACHE_BOOT };
    let next_cache = mut_ref_memcache!(cache_head.list.next);
    assert_eq!(next_cache.cache_name,"array_cache");
    next_cache.alloc()
}

/// 初始化第一个cache
pub fn mem_cache_init()->Result<(),SlabError> {
    unsafe {
        list_head_init!(SLAB_CACHES);
    }
    let cache = unsafe { &mut MEM_CACHE_BOOT };
    cache.init(
        "kmem_cache",
        core::mem::size_of::<MemCache>() as u32,
        core::mem::align_of::<MemCache>() as u32,
    );
    /// 初始化本地高速缓存信息
    unsafe {
        for i in 0..CPUS {
            cache.array_cache[i] = &mut ARRAY_CACHE_FOR_BOOT[i] as *mut ArrayCache;
        }
        cache.mem_cache_node.shared = &mut ARRAY_CACHE_NODE_BOOT as *mut ArrayCache;
    }
    list_add_tail!(
        to_list_head_ptr!(cache.list),
        to_list_head_ptr!(SLAB_CACHES)
    );

    /// 初始化array_cache，用于后面分配本地高速缓存对象
    let array_cache = create(
        "array_cache",
        core::mem::size_of::<ArrayCache>() as u32,
        core::mem::align_of::<ArrayCache>() as u32,
    )?;
    unsafe {
        for i in 0..CPUS {
            array_cache.array_cache[i] = &mut ARRAY_CACHE_FOR_ARRAY[i] as *mut ArrayCache;
        }
        array_cache.mem_cache_node.shared = &mut ARRAY_CACHE_NODE_ARRAY as *mut ArrayCache;
    }
    Ok(())
}

/// 创建自定义的cache
pub fn create_mem_cache(
    name: &'static str,
    object_size: u32,
    align: u32,
) -> Result<&mut MemCache, SlabError> {
    // 创建一个自定义cache
    let cache_head = unsafe { &mut SLAB_CACHES };
    let find = cache_head.iter().find(|&cache_list| {
        let cache = mut_ref_memcache!(cache_list);
        // //查找是否存在同名的cache
        cache.cache_name.eq(name)
    });
    if find.is_some() {
        return Err(SlabError::NameDuplicate);
    }
    let cache_object = create(name, object_size, align)?;
    // 初始化高速缓存
    cache_object.set_array_cache()?;
    Ok(cache_object)
}

fn create(name: &'static str, object_size: u32, align: u32) -> Result<&mut MemCache, SlabError> {
    /// 从第一个初始化的cache中分配一个cached对象
    let cache = unsafe { &mut MEM_CACHE_BOOT };
    let cache_object_addr = cache.alloc()?;
    let cache_object_addr =  cache_object_addr as *mut MemCache;
    let cache_object = unsafe { &mut (*cache_object_addr) };
    /// 初始化cache
    cache_object.init(name, object_size, align).unwrap();
    /// 将cache加入到SLAB_CACHES链表中
    list_add_tail!(
        to_list_head_ptr!(cache_object.list),
        to_list_head_ptr!(SLAB_CACHES)

    );
    Ok(cache_object)
}


/// 分配一个指定大小和对齐方式的内存
/// 这里暂时忽略了对齐带来的影响
pub fn alloc_from_slab(size: usize, _align: usize) -> Result<*mut u8,SlabError> {
    // 遍历所有的cache，找到第一个能够分配的cache
    // 跳过第一个cache，因为第一个cache是用来分配cache的
    // 跳过第二个cache，因为第二个slab是用来分配array_cache的
    // 不在用户创建的cache上分配
    let cache_list = unsafe { &mut SLAB_CACHES };

    // 8B-8MB == 3-23
    let index = size.next_power_of_two();
    let index = index.trailing_zeros() as usize;
    if index > 23 {
        return Err(SlabError::SizeTooLarge);
    }
    let find = cache_list.iter().find(|&cache_list| {
        let cache = mut_ref_memcache!(cache_list);
        // 查找是否存在同名的cache
        cache.object_size.trailing_zeros() as usize >= index && cache.cache_name !="array_cache" && cache.cache_name!="kmem_cache"
    }).unwrap();
    let cache = mut_ref_memcache!(find);
    cache.alloc()
}

/// 将分配的对象还给slab系统
pub fn dealloc_to_slab(addr: *mut u8) -> Result<(), SlabError> {
    let cache_list = unsafe { &SLAB_CACHES };
    let mut ok = false;
    cache_list.iter().for_each(|cache| {
        let cache = mut_ref_memcache!(cache);
        let ans = cache.dealloc(addr);
        if ans.is_ok() {
            ok = true;
            return;
        }
    });
    if ok {
        Ok(())
    } else {
        Err(SlabError::NotInCache)
    }
}

/// 打印系统内的所有cache 信息
pub fn print_slab_system_info() {
    let cache_list = unsafe { &SLAB_CACHES };
    pprintln!("There are {} caches in system:", cache_list.len());
    pprintln!("cache_name object_size align p_frames p_objects  total_object used_object limit batch_count local_cpus shared");
    cache_list.iter().for_each(|cache| {
        let cache = ref_memcache!(cache);
        pprintln!("----------------------------------------------------------------------------------------------------------");
        cache.print_info();
    });
}



#[cfg(test)]
mod array_cache_test {
    use crate::slab::{ArrayCache, PER_CPU_OBJECTS};
    #[test]
    fn test_push_pop() {
        let mut cache = ArrayCache::new();
        let mut inner = cache.inner.lock();
        assert_eq!(inner.is_empty(), true);
        assert_eq!(inner.batch_count as usize, PER_CPU_OBJECTS / 2);
        assert_eq!(inner.limit as usize, PER_CPU_OBJECTS);
        let mut data = [0; PER_CPU_OBJECTS];
        let batch = inner.batch_count as usize;
        inner.push(&data[0..batch]);
        inner.push(&data[0..batch]);
        assert_eq!(inner.is_empty(), false);
        assert_eq!(inner.avail as usize, PER_CPU_OBJECTS);
        inner.pop(&mut data[0..batch]);
        assert_eq!(inner.avail, PER_CPU_OBJECTS as u32 / 2);
    }
    #[test]
    #[should_panic]
    fn test_push_pop_panic1() {
        let mut cache = ArrayCache::new();
        let mut inner = cache.inner.lock();
        let mut data = [0; PER_CPU_OBJECTS];
        let batch = inner.batch_count as usize;
        // 需要保证按批次送入
        inner.push(&data[0..]);
    }
    #[test]
    #[should_panic]
    fn test_push_pop_panic2() {
        let mut cache = ArrayCache::new();
        let mut inner = cache.inner.lock();
        let mut data = [0; PER_CPU_OBJECTS];
        let batch = inner.batch_count as usize;
        // 只有在队列满的情况下才会回收对象
        inner.push(&data[0..batch]);
        inner.pop(&mut data[0..batch]);
    }
    #[test]
    fn test_get_put() {
        let mut cache = ArrayCache::new();
        let mut inner = cache.inner.lock();
        let mut data = [10; PER_CPU_OBJECTS];
        let batch = inner.batch_count as usize;
        // 只有在队列满的情况下才会回收对象
        inner.push(&data[0..batch]);
        let t = inner.get();
        assert_eq!(10, t as usize);
        assert_eq!(inner.avail as usize, batch - 1);
    }
    #[test]
    #[should_panic]
    fn test_get_put_panic() {
        let mut cache = ArrayCache::new();
        let mut inner = cache.inner.lock();
        inner.get();
    }
    #[test]
    #[should_panic]
    fn test_get_put_panic1() {
        let mut cache = ArrayCache::new();
        let mut inner = cache.inner.lock();
        let mut data = [0; PER_CPU_OBJECTS];
        let batch = inner.batch_count as usize;
        // 只有在队列满的情况下才会回收对象
        inner.push(&data[0..batch]);
        inner.push(&data[0..batch]);
        inner.put(10 as *mut u8);
    }
}

#[cfg(test)]
mod slab_test {
    use crate::slab::{mem_cache_init, CacheNode, Flags};
    use crate::MemCache;
    #[no_mangle]
    unsafe fn free_frames(addr: *mut u8, num: usize) {}
    #[no_mangle]
    fn current_cpu_id() -> usize {
        0
    }
    #[no_mangle]
    unsafe fn alloc_frames(num: usize) -> *mut u8 {
        core::ptr::null_mut()
    }

    #[test]
    fn test_init_cache_small_obj() {
        let mut cache = MemCache::new();
        cache.init("test_cache", 128, 7);
        assert_eq!(cache.align, 8);
        assert_eq!(cache.cache_name, "test_cache");
        assert_eq!(cache.object_size, 128);
        assert_eq!(cache.flags, Flags::SLAB_ON);
        assert_eq!(cache.per_frames, 0);
        assert_eq!(cache.per_objects, 30);
        assert_eq!(cache.color, 5);
        cache.init("test_cache", 127, 7);
        assert_eq!(cache.object_size, 128);
    }

    #[test]
    fn test_init_cache_big_obj() {
        let mut cache = MemCache::new();
        cache.init("test_cache", 512, 7);
        assert_eq!(cache.flags, Flags::SLAB_OFF);
        assert_eq!(cache.color, 0);
        assert_eq!(cache.per_frames, 0);
        assert_eq!(cache.per_objects, 8);
    }

    #[test]
    fn test_cache_node() {
        let mut node = CacheNode::new();
        node.init();
        let x = node.total_slabs();
        assert_eq!(x, 0);
        assert_eq!(node.used_objects(10), 0);
    }
}
