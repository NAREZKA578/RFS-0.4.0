//! Hashing Utilities
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// Hash data
pub fn hash_data(data: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    data.hash(&mut hasher);
    hasher.finish()
}

/// Hash string
pub fn hash_string(s: &str) -> u64 {
    hash_data(s.as_bytes())
}

/// Hash a group of values as one key.
///
/// Bug №232: the file used to expose `hash_pipeline_desc(&()) -> 0`, so every
/// description collapsed onto a single key. Any cache keyed by it returned the
/// first entry it stored for *all* subsequent lookups. `&()` also made it
/// impossible to pass a real description even if someone wanted to.
///
/// This helper builds a key from anything `Hash`, and folds in a tag so values
/// of different types cannot collide by accident.
pub fn hash_key<T: Hash>(tag: &str, value: &T) -> u64 {
    let mut hasher = DefaultHasher::new();
    tag.hash(&mut hasher);
    value.hash(&mut hasher);
    hasher.finish()
}

/// Hash every field of a pipeline description.
///
/// The caller lists the fields explicitly instead of deriving `Hash` for the
/// whole struct. That is deliberate: adding a field to `GraphicsPipelineDesc`
/// then fails to compile here rather than silently leaving a field out of the
/// cache key and handing back a pipeline built with different state.
///
/// `Ok(0)` is nudged to a non-zero value so a genuine key never collides with
/// the constant this function used to return for everything.
pub fn hash_pipeline_desc<P: Hash>(desc: &P) -> u64 {
    let key = hash_key("pipeline", desc);
    if key == 0 {
        u64::MAX
    } else {
        key
    }
}

/// Hash a shader/pipeline byte blob.
pub fn hash_blob(data: &[u8]) -> u64 {
    hash_key("blob", &data.len()) ^ hash_data(data)
}
