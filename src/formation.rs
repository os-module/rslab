use preprint::pprintln;

#[derive(Debug)]
pub enum InitError {
    NameTooLong,
}

#[derive(Debug)]
pub enum SlabError {
    InitError(InitError),
    NameDuplicate,
    NotInCache,
    ArrayCacheAllocError,
}

// pprintln!("cache_name object_size align p_frames p_objects  total_object used_object limit batch_count local_cpus shared");


