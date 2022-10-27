mod common;

use slab::{create_mem_cache, init_slab_system};

#[test]
fn test_create_cache(){
    init_slab_system(4096, 64);
    let cache = create_mem_cache("my_cache", 56, 8).unwrap();
    let cache_info = cache.get_cache_info();
    assert_eq!(cache_info.cache_name, "my_cache");
    assert_eq!(cache_info.align, 8);
    assert_eq!(cache_info.per_frames,0);
    assert_eq!(cache_info.per_objects, 67);
    assert_eq!(cache_info.total_objects,0);
    assert_eq!(cache_info.used_objects,0);
    assert_eq!(cache_info.local_objects,0);
    assert_eq!(cache_info.shared_objects,0);
    let t = cache.alloc();
    assert_eq!(t.is_null(), false);
    let cache_info = cache.get_cache_info();
    assert_eq!(cache_info.total_objects,67);
    assert_eq!(cache_info.used_objects,1);
    assert_eq!(cache_info.local_objects,cache_info.batch_count-1);
    assert_eq!(cache_info.shared_objects,0);
    assert!(cache.dealloc(t).is_ok());
    let cache_info = cache.get_cache_info();
    assert_eq!(cache_info.used_objects,0);
    assert_eq!(cache_info.local_objects,cache_info.batch_count);
    assert_eq!(cache_info.shared_objects,0);
    for _ in 0..cache_info.limit+1{
        let t = cache.alloc();
        assert_eq!(t.is_null(), false);
    }
    let cache_info = cache.get_cache_info();
    assert_eq!(cache_info.used_objects,cache_info.limit+1);
    assert_eq!(cache_info.local_objects,cache_info.batch_count-1);
    cache.destroy()
}

#[test]
#[should_panic]
fn test_slab_panic(){
    let _cache = create_mem_cache("my_cache1", 56, 8).unwrap();
    // there has been a cache named "my_cache1"
    let _cache = create_mem_cache("my_cache1", 56, 8).unwrap();
}


#[test]
#[should_panic]
fn test_slab_panic1(){
    let cache = create_mem_cache("my_cache2", 56, 8).unwrap();
    cache.destroy();
    // cache has been destroyed
    cache.alloc();
}




