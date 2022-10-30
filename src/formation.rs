use preprint::pprintln;


#[derive(Debug)]
pub enum SlabError {
    CantAllocFrame,
    NameDuplicate,
    NotInCache,
    ArrayCacheAllocError,
    SizeTooLarge,
}
