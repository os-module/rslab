# slab分配器实现

## 模型结构

![无标题-2022-10-20-1706.excalidraw](assert/rslab.png)

## 对外接口

```rust
pub fn init_slab_system(frame_size: usize, cache_line_size: usize) 
```

此函数用于初始化slab系统，用户需要告知slab系统分配的页帧大小和缓存行大小，页帧大小用于计算对象数量，缓存行大小用于着色偏移计算。slab系统会完成第一个Cache的初始化并创建多个常用大小的Cache,这些cache对象的大小从8B-8MB

```rust
pub fn print_slab_system_info()
```

这个函数用于打印slab系统的使用情况。

```rust
pub struct SlabAllocator;
```

上述结构体已经实现`GlobalAlloc`,因此外部系统可以直接声明其为`#[global_allocator]`以启用`alloc`内的大多数数据结构。

```rust
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
pub struct SlabCache<T: Object>{..}
```

`SlabCache`是内部 Cache的封装，如果用户想单独创建一个Cache，则需要使用此数据结构进行创建，用户自定义的数据需要实现`object` 这个`trait`，这样`SlabCache` 在分配时可以进行初始化以免用户直接接触裸指针，这里为简单起见，若分配成功则返回对象的引用。

一个简单实例如下:

```rust
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
let mut cache = SlabCache::<TestObj>::new("mycache").unwrap();
    // alloc from your cache
let ptr = cache.alloc().unwrap();
```



## 对内接口

外部需要提供的接口：

```rust
pub fn alloc_frames(num:usize)->*mut u8
pub fn free_frames(addr: *mut u8, num: usize) 
pub fn current_cpu_id() -> usize
```

外部需要提供一个分配页面的接口和回收页面的接口。为了支持多核的CPU，减少核心之间的争用，定义了Per_CPU数据，因此需要一个获取当前核心的id的接口。若此分配器用户用户态，可简单将其设为返回0即可。

系统内部为空闲链表的设定了一个常数上限，当达到上限将触发回收页帧。

## 使用方式

1. 首先实现外部需要提供的三个接口

```rust
#[no_mangle]
fn alloc_frames(num: usize) -> *mut u8 
#[no_mangle]
fn free_frames(addr: *mut u8, num: usize) 
#[no_mangle]
pub fn current_cpu_id() -> usize
```

2. 初始化slab子系统

```rust
init_slab_system(FRAME_SIZE, 32);
```

3. 在rust中，声明全局全局分配器

```rust
#[global_allocator]
static HEAP_ALLOCATOR: SlabAllocator = SlabAllocator;
```

现在可以就可以正常使用slab子系统提供的分配和回收物理内存的功能了。

在内核的文件模块，进程模块，都可以为既定的结构体创建一个Cache,并使用此Cache分配对象。



## 待办事项

- [x] 每CPU缓存
- [x] 细粒度的锁
- [ ] 其它优化



