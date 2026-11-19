//! Reads a package the writer wrote back into its parts: each part's name and
//! inflated bytes, from the central directory, or `None` on anything else.

pub fn unzip(bytes: &[u8]) -> Option<Vec<(String, Vec<u8>)>> {
    let u16_at = |at: usize| Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?));
    let u32_at = |at: usize| Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?));
    let end = bytes.len().checked_sub(22)?;
    if u32_at(end)? != 0x0605_4b50 {
        return None;
    }
    let count = u16_at(end + 10)? as usize;
    let mut at = u32_at(end + 16)? as usize;
    let mut parts = Vec::with_capacity(count);
    for _ in 0..count {
        if u32_at(at)? != 0x0201_4b50 {
            return None;
        }
        let method = u16_at(at + 10)?;
        let crc = u32_at(at + 16)?;
        let packed = u32_at(at + 20)? as usize;
        let name_len = u16_at(at + 28)? as usize;
        let extra = u16_at(at + 30)? as usize + u16_at(at + 32)? as usize;
        let local = u32_at(at + 42)? as usize;
        let name = String::from_utf8(bytes.get(at + 46..at + 46 + name_len)?.to_vec()).ok()?;
        let local_name = u16_at(local + 26)? as usize + u16_at(local + 28)? as usize;
        let start = local.checked_add(30 + local_name)?;
        let body = bytes.get(start..start.checked_add(packed)?)?;
        let data = match method {
            0 => body.to_vec(),
            8 => miniz_oxide::inflate::decompress_to_vec(body).ok()?,
            _ => return None,
        };
        if crc32fast::hash(&data) != crc {
            return None;
        }
        parts.push((name, data));
        at += 46 + name_len + extra;
    }
    Some(parts)
}
