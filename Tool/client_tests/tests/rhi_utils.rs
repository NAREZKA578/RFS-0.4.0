use rhi::utils::alignment::*;
use rhi::utils::hash::*;

#[test]
fn align_up_zero() {
    assert_eq!(align_up(0, 256), 0);
}

#[test]
fn align_up_one() {
    assert_eq!(align_up(1, 256), 256);
}

#[test]
fn align_up_boundary_minus_one() {
    assert_eq!(align_up(255, 256), 256);
}

#[test]
fn align_up_exact() {
    assert_eq!(align_up(256, 256), 256);
}

#[test]
fn align_up_over() {
    assert_eq!(align_up(257, 256), 512);
}

#[test]
fn align_up_various() {
    assert_eq!(align_up(3, 4), 4);
    assert_eq!(align_up(5, 4), 8);
    assert_eq!(align_up(129, 128), 256);
    assert_eq!(align_up(128, 128), 128);
}

#[test]
fn align_down_zero() {
    assert_eq!(align_down(0, 256), 0);
}

#[test]
fn align_down_below() {
    assert_eq!(align_down(255, 256), 0);
}

#[test]
fn align_down_exact() {
    assert_eq!(align_down(256, 256), 256);
}

#[test]
fn align_down_above() {
    assert_eq!(align_down(300, 256), 256);
}

#[test]
fn align_down_various() {
    assert_eq!(align_down(7, 4), 4);
    assert_eq!(align_down(4, 4), 4);
    assert_eq!(align_down(3, 4), 0);
    assert_eq!(align_down(300, 128), 256);
}

#[test]
fn is_aligned_zero() {
    assert!(is_aligned(0, 256));
}

#[test]
fn is_aligned_exact() {
    assert!(is_aligned(256, 256));
}

#[test]
fn is_aligned_not() {
    assert!(!is_aligned(1, 256));
}

#[test]
fn is_aligned_various() {
    assert!(is_aligned(0, 4));
    assert!(is_aligned(4, 4));
    assert!(!is_aligned(5, 4));
    assert!(is_aligned(128, 128));
    assert!(!is_aligned(129, 128));
}

#[test]
fn hash_data_consistent() {
    let h1 = hash_data(b"test");
    let h2 = hash_data(b"test");
    assert_eq!(h1, h2);
}

#[test]
fn hash_data_different_inputs() {
    let h1 = hash_data(b"test");
    let h2 = hash_data(b"test2");
    assert_ne!(h1, h2);
}

#[test]
fn hash_string_consistent() {
    let h1 = hash_string("hello");
    let h2 = hash_string("hello");
    assert_eq!(h1, h2);
}

#[test]
fn hash_string_different() {
    let h1 = hash_string("hello");
    let h2 = hash_string("world");
    assert_ne!(h1, h2);
}

#[test]
fn hash_string_matches_bytes() {
    let h1 = hash_string("test");
    let h2 = hash_data(b"test");
    assert_eq!(h1, h2);
}

#[test]
fn alignment_constants() {
    assert_eq!(BUFFER_COPY_ALIGNMENT, 4);
    assert_eq!(TEXTURE_COPY_ALIGNMENT, 4);
    assert_eq!(UNIFORM_BUFFER_ALIGNMENT, 256);
    assert_eq!(STORAGE_BUFFER_ALIGNMENT, 16);
}
