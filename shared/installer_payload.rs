//! Locate an EEF ZIP payload, including after Authenticode appends a certificate.
//! This parses framing only. It does NOT authenticate a signature or publisher.
//! Release signing must be verified separately; updates still verify file SHA-256.
use std::io::{self, Read, Seek, SeekFrom};
use std::ops::Range;

const MAGIC: &[u8; 8] = b"EEFINST1";
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
fn at<const N: usize>(reader: &mut (impl Read + Seek), offset: u64) -> io::Result<[u8; N]> {
    reader.seek(SeekFrom::Start(offset))?;
    let mut bytes = [0; N];
    reader.read_exact(&mut bytes)?;
    Ok(bytes)
}
fn u16_at(reader: &mut (impl Read + Seek), offset: u64) -> io::Result<u16> {
    Ok(u16::from_le_bytes(at(reader, offset)?))
}
fn u32_at(reader: &mut (impl Read + Seek), offset: u64) -> io::Result<u64> {
    Ok(u32::from_le_bytes(at(reader, offset)?) as u64)
}

/// The certificate directory contains a file offset, not an RVA. Only accept a
/// terminal, bounded certificate table; do not search arbitrary certificate data.
fn content_end(reader: &mut (impl Read + Seek), length: u64) -> io::Result<(u64, bool)> {
    if length < 64 || &at::<2>(reader, 0)? != b"MZ" {
        return Ok((length, false));
    }
    let pe = u32_at(reader, 0x3c)?;
    if pe < 64 || pe + 24 > length || &at::<4>(reader, pe)? != b"PE\0\0" {
        return Err(invalid("invalid PE header"));
    }
    let optional = pe + 24;
    let size = u16_at(reader, pe + 20)? as u64;
    if size < 2 || optional + size > length {
        return Err(invalid("invalid PE optional header"));
    }
    let directories = match u16_at(reader, optional)? {
        0x10b => 96,
        0x20b => 112,
        _ => return Err(invalid("unsupported PE optional header")),
    };
    if size < directories {
        return Err(invalid("truncated PE data directory header"));
    }
    if u32_at(reader, optional + directories - 4)? <= 4 {
        return Ok((length, false));
    }
    if size < directories + 40 {
        return Err(invalid("truncated PE certificate directory"));
    }
    let start = u32_at(reader, optional + directories + 32)?;
    let size = u32_at(reader, optional + directories + 36)?;
    if start == 0 && size == 0 {
        return Ok((length, false));
    }
    if start < optional + directories + 40 || start % 8 != 0 || size < 8 || start + size != length {
        return Err(invalid("invalid PE certificate table bounds"));
    }
    let mut position = start;
    while position < length {
        if length - position < 8 {
            return Err(invalid("truncated certificate entry"));
        }
        let entry_length = u32_at(reader, position)?;
        if entry_length < 8 || position + entry_length > length {
            return Err(invalid("invalid certificate entry length"));
        }
        position += (entry_length + 7) & !7;
    }
    if position != length {
        return Err(invalid("invalid certificate table alignment"));
    }
    Ok((start, true))
}

pub fn payload_range(reader: &mut (impl Read + Seek)) -> io::Result<Range<u64>> {
    let length = reader.seek(SeekFrom::End(0))?;
    let (end, signed_container) = content_end(reader, length)?;
    // Authenticode may insert at most seven zero alignment bytes before its table.
    // Unsigned files must have the footer exactly at EOF.
    for padding in 0..=if signed_container { 7 } else { 0 } {
        let Some(footer) = end.checked_sub(padding + 16) else {
            continue;
        };
        if &at::<8>(reader, footer + 8)? != MAGIC {
            continue;
        }
        if (0..padding).any(|n| at::<1>(reader, end - padding + n).map_or(true, |b| b != [0])) {
            return Err(invalid("nonzero certificate alignment padding"));
        }
        let payload_length = u64::from_le_bytes(at(reader, footer)?);
        let start = footer
            .checked_sub(payload_length)
            .ok_or_else(|| invalid("invalid installer payload length"))?;
        if payload_length < 4 || &at::<4>(reader, start)? != b"PK\x03\x04" {
            return Err(invalid("installer payload is not a ZIP archive"));
        }
        return Ok(start..footer);
    }
    Err(invalid("installer payload footer is missing"))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(pe32: bool, signed: bool, padding: usize) -> Vec<u8> {
        let mut bytes = vec![0u8; 512];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[0x3c..0x40].copy_from_slice(&64u32.to_le_bytes());
        bytes[64..68].copy_from_slice(b"PE\0\0");
        bytes[84..86].copy_from_slice(&240u16.to_le_bytes());
        bytes[88..90].copy_from_slice(&(if pe32 { 0x10bu16 } else { 0x20b }).to_le_bytes());
        let directory = 88 + if pe32 { 96 } else { 112 };
        bytes[directory - 4..directory].copy_from_slice(&16u32.to_le_bytes());
        let zip = [b'P', b'K', 3, 4];
        bytes.extend(vec![1; (8 - padding + 4) % 8]);
        bytes.extend(zip);
        bytes.extend((zip.len() as u64).to_le_bytes());
        bytes.extend(MAGIC);
        if signed {
            bytes.extend(vec![0; padding]);
            let start = bytes.len() as u32;
            assert_eq!(start % 8, 0);
            bytes[directory + 32..directory + 36].copy_from_slice(&start.to_le_bytes());
            bytes[directory + 36..directory + 40].copy_from_slice(&16u32.to_le_bytes());
            // Synthetic framing, NOT a real or trusted signature.
            bytes.extend(16u32.to_le_bytes());
            bytes.extend(0x200u16.to_le_bytes());
            bytes.extend(2u16.to_le_bytes());
            bytes.extend([0; 8]);
        }
        bytes
    }
    #[test]
    fn unsigned_and_both_signed_pe_formats_with_all_padding_lengths() {
        for pe32 in [true, false] {
            for signed in [false, true] {
                for padding in 0..8 {
                    let bytes = fixture(pe32, signed, padding);
                    let range = payload_range(&mut io::Cursor::new(&bytes)).unwrap();
                    assert_eq!(
                        &bytes[range.start as usize..range.end as usize],
                        b"PK\x03\x04"
                    );
                }
            }
        }
    }
    #[test]
    fn rejects_truncated_invalid_and_unframed_inputs() {
        let bytes = fixture(false, true, 0);
        for end in 0..bytes.len() {
            assert!(payload_range(&mut io::Cursor::new(&bytes[..end])).is_err());
        }
        let mut bad = bytes.clone();
        bad.extend([0]);
        assert!(payload_range(&mut io::Cursor::new(&bad)).is_err());
        let mut bad = fixture(false, true, 1);
        let position = bad.len() - 17;
        bad[position] = 1;
        assert!(payload_range(&mut io::Cursor::new(&bad)).is_err());
        let mut bad = fixture(false, false, 0);
        bad.extend([0]);
        assert!(payload_range(&mut io::Cursor::new(&bad)).is_err());
    }
    #[test]
    fn rejects_bad_offsets_lengths_and_fake_footer_in_certificate() {
        let valid = fixture(false, true, 0);
        for (offset, value) in [(0x3c, u32::MAX), (232, u32::MAX), (236, 7)] {
            let mut bad = valid.clone();
            bad[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            assert!(payload_range(&mut io::Cursor::new(&bad)).is_err());
        }
        let mut bad = valid.clone();
        let entry = bad.len() - 16;
        bad[entry..entry + 4].copy_from_slice(&0u32.to_le_bytes());
        assert!(payload_range(&mut io::Cursor::new(&bad)).is_err());
        let mut bad = valid;
        let end = bad.len();
        bad[end - 8..].copy_from_slice(MAGIC);
        bad[end - 24..end - 16].fill(0);
        assert!(payload_range(&mut io::Cursor::new(&bad)).is_err());
    }
}
