//! A minimal PNG writer, so the render tests can produce an image artifact
//! without pulling in a dependency.
//!
//! Only what the tests need: 8-bit RGBA, non-interlaced, one `IDAT`. The
//! deflate stream uses *stored* (uncompressed) blocks, which is valid zlib and
//! any decoder reads it — a 64x64 frame is ~16 KB, and an artifact that is
//! slightly larger on disk is not worth a dependency.
//!
//! This is deliberately a test helper, not part of `rhi`: the engine has no
//! need to write PNGs.

/// Build a PNG file from tightly packed RGBA8 rows.
pub fn encode_rgba8(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    assert_eq!(
        rgba.len(),
        (width as usize) * (height as usize) * 4,
        "pixel buffer is {} bytes, expected {} for {width}x{height} RGBA",
        rgba.len(),
        (width as usize) * (height as usize) * 4
    );

    // Raw scanlines, each prefixed with filter byte 0 (None).
    let stride = width as usize * 4;
    let mut raw = Vec::with_capacity((stride + 1) * height as usize);
    for y in 0..height as usize {
        raw.push(0);
        raw.extend_from_slice(&rgba[y * stride..(y + 1) * stride]);
    }

    let mut png = Vec::new();
    png.extend_from_slice(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);

    // IHDR: 13 bytes of payload, in network byte order.
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.push(8); // bit depth
    ihdr.push(6); // colour type 6 = RGBA
    ihdr.push(0); // compression: deflate
    ihdr.push(0); // filter method
    ihdr.push(0); // interlace: none
    write_chunk(&mut png, b"IHDR", &ihdr);

    write_chunk(&mut png, b"IDAT", &zlib_stored(&raw));
    write_chunk(&mut png, b"IEND", &[]);
    png
}

fn write_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc_input = Vec::with_capacity(4 + data.len());
    crc_input.extend_from_slice(kind);
    crc_input.extend_from_slice(data);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
}

/// A zlib stream of stored deflate blocks.
fn zlib_stored(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + data.len() / 65535 * 5 + 16);
    out.push(0x78); // CM = deflate, CINFO = 32K window
    out.push(0x01); // FLEVEL = fastest, FCHECK making (0x78, 0x01) % 31 == 0

    if data.is_empty() {
        out.extend_from_slice(&[0x01, 0x00, 0x00, 0xFF, 0xFF]);
    } else {
        let mut chunks = data.chunks(0xFFFF).peekable();
        while let Some(block) = chunks.next() {
            let last = chunks.peek().is_none();
            out.push(if last { 1 } else { 0 });
            let len = block.len() as u16;
            out.extend_from_slice(&len.to_le_bytes());
            out.extend_from_slice(&(!len).to_le_bytes()); // one's complement
            out.extend_from_slice(block);
        }
    }

    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in data {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

/// CRC-32 (IEEE 802.3), the variant PNG chunks use.
fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}
