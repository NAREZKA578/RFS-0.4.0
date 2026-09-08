// Moved out of crates/net/src/fragment.rs (side-only testing rule).
use rfs_core::packet::PacketType;
use rfs_net::{FragmentAssembler, make_fragments};

#[test]
fn fragment_roundtrip_in_order() {
    let data: Vec<u8> = (0..5000u32).map(|i| (i % 251) as u8).collect();
    let frags = make_fragments(PacketType::StateDelta, &data, 7, 1300);
    assert!(frags.len() > 1);
    let mut asm = FragmentAssembler::new();
    for (i, f) in frags.iter().enumerate() {
        let res = asm.push(f);
        if i + 1 == frags.len() {
            let (pt, msg) = res.expect("last fragment completes the message");
            assert_eq!(pt, PacketType::StateDelta);
            assert_eq!(msg, data);
        } else {
            assert!(res.is_none());
        }
    }
}

#[test]
fn fragment_roundtrip_out_of_order() {
    let data: Vec<u8> = (0..4000u32).map(|i| (i * 7 % 253) as u8).collect();
    let mut frags = make_fragments(PacketType::State, &data, 42, 1000);
    frags.reverse();
    let mut asm = FragmentAssembler::new();
    let mut done = None;
    for f in &frags {
        if let Some(res) = asm.push(f) {
            done = Some(res);
        }
    }
    let (pt, msg) = done.expect("all chunks present despite order");
    assert_eq!(pt, PacketType::State);
    assert_eq!(msg, data);
}

#[test]
fn fragment_newer_supersedes_incomplete() {
    let mut asm = FragmentAssembler::new();
    let old = make_fragments(PacketType::StateDelta, &[1u8; 3000], 1, 1000);
    assert!(asm.push(&old[0]).is_none());
    // A newer message of the same type drops the incomplete older group...
    let new = make_fragments(PacketType::StateDelta, &[2u8; 3000], 2, 1000);
    for (i, f) in new.iter().enumerate() {
        let res = asm.push(f);
        if i + 1 == new.len() {
            let (_, msg) = res.expect("newer message completes");
            assert!(msg.iter().all(|&b| b == 2));
        }
    }
    // ...so the rest of the old message is ignored, not completed.
    assert!(asm.push(&old[1]).is_none());
    assert!(asm.push(&old[2]).is_none());
}

#[test]
fn fragment_rejects_garbage() {
    let mut asm = FragmentAssembler::new();
    assert!(asm.push(&[]).is_none());
    assert!(asm.push(&[0x21, 0x01]).is_none());
    // total_chunks == 0
    let mut bad = vec![0x21u8];
    bad.extend_from_slice(&0u16.to_le_bytes());
    bad.extend_from_slice(&0u16.to_le_bytes());
    bad.extend_from_slice(&10u32.to_le_bytes());
    bad.extend_from_slice(&1u32.to_le_bytes());
    assert!(asm.push(&bad).is_none());
    // unknown original packet type
    let mut bad2 = vec![0xFFu8];
    bad2.extend_from_slice(&1u16.to_le_bytes());
    bad2.extend_from_slice(&0u16.to_le_bytes());
    bad2.extend_from_slice(&4u32.to_le_bytes());
    bad2.extend_from_slice(&1u32.to_le_bytes());
    bad2.extend_from_slice(&[1, 2, 3, 4]);
    assert!(asm.push(&bad2).is_none());
}
