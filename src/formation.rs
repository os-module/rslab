#[derive(Debug)]
pub enum InitError {
    NameTooLong,
}

#[derive(Debug)]
pub enum SlabError {
    InitError(InitError),
    NameDuplicate,
    ArrayCacheAllocError,
}
