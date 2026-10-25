//! Split AV1 temporal units from a concatenated OBU stream.

pub const OBU_TEMPORAL_DELIMITER: u8 = 2;
pub const OBU_FRAME_HEADER: u8 = 3;
pub const OBU_FRAME: u8 = 6;

#[derive(Clone, Copy, Debug)]
struct Obu {
    ty: u8,
    total_len: usize,
}

pub fn take_complete_tus(buf: &mut Vec<u8>) -> Vec<Vec<u8>> {
    let mut obus = Vec::new();
    let mut off = 0usize;
    while off < buf.len() {
        match parse_obu(&buf[off..]) {
            Some(obu) => {
                obus.push((off, obu));
                off += obu.total_len;
            }
            None => break,
        }
    }
    if obus.len() < 2 {
        return Vec::new();
    }
    let td_idx: Vec<usize> = obus
        .iter()
        .enumerate()
        .filter(|(_, (_, o))| o.ty == OBU_TEMPORAL_DELIMITER)
        .map(|(i, _)| i)
        .collect();
    if td_idx.len() >= 2 {
        let mut tus = Vec::new();
        for w in td_idx.windows(2) {
            let begin = obus[w[0]].0;
            let end = obus[w[1]].0;
            if end > begin {
                tus.push(buf[begin..end].to_vec());
            }
        }
        let keep = obus[td_idx[td_idx.len() - 1]].0;
        buf.drain(..keep);
        return tus;
    }
    let mut tus = Vec::new();
    let mut au_begin = 0usize;
    for i in 0..obus.len() {
        if obus[i].1.ty == OBU_FRAME || obus[i].1.ty == OBU_FRAME_HEADER {
            if i + 1 >= obus.len() {
                break;
            }
            let begin = obus[au_begin].0;
            let end = obus[i + 1].0;
            if end > begin {
                tus.push(buf[begin..end].to_vec());
            }
            au_begin = i + 1;
        }
    }
    let keep = obus[au_begin].0;
    buf.drain(..keep);
    tus
}

#[cfg(test)]
pub fn tu_is_keyframe(tu: &[u8]) -> bool {
    let mut off = 0usize;
    while off < tu.len() {
        match parse_obu(&tu[off..]) {
            Some(obu) => {
                if obu.ty == 1 {
                    return true;
                }
                off += obu.total_len;
            }
            None => break,
        }
    }
    false
}

#[cfg(test)]
pub fn sequence_header_obu(data: &[u8]) -> Option<&[u8]> {
    let mut off = 0usize;
    while off < data.len() {
        let obu = parse_obu(&data[off..])?;
        if obu.ty == 1 {
            return Some(&data[off..off + obu.total_len]);
        }
        off += obu.total_len;
    }
    None
}

fn parse_obu(data: &[u8]) -> Option<Obu> {
    if data.is_empty() {
        return None;
    }
    let b = data[0];
    if b & 0x80 != 0 {
        return None;
    }
    let ty = (b >> 3) & 0x0F;
    let ext = (b & 0x04) != 0;
    let has_size = (b & 0x02) != 0;
    let mut i = 1usize;
    if ext {
        if i >= data.len() {
            return None;
        }
        i += 1;
    }
    if !has_size {
        return None;
    }
    let (size, n) = leb128(&data[i..])?;
    i += n;
    let total = i.checked_add(size as usize)?;
    if total > data.len() {
        return None;
    }
    Some(Obu { ty, total_len: total })
}

fn leb128(data: &[u8]) -> Option<(u64, usize)> {
    let mut value = 0u64;
    let mut shift = 0u32;
    for (i, b) in data.iter().copied().enumerate() {
        value |= u64::from(b & 0x7F) << shift;
        if b & 0x80 == 0 {
            return Some((value, i + 1));
        }
        shift += 7;
        if shift > 63 {
            return None;
        }
    }
    None
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn obu(ty: u8, payload: &[u8]) -> Vec<u8> {
        let mut out = vec![ty << 3 | 0x02];
        out.push(payload.len() as u8);
        out.extend_from_slice(payload);
        out
    }

    #[test]
    fn splits_on_temporal_delimiters() {
        let mut buf = Vec::new();
        buf.extend(obu(OBU_TEMPORAL_DELIMITER, &[]));
        buf.extend(obu(1, &[0x00, 0x01]));
        buf.extend(obu(OBU_FRAME, &[0xAA]));
        buf.extend(obu(OBU_TEMPORAL_DELIMITER, &[]));
        buf.extend(obu(OBU_FRAME, &[0xBB]));
        let tus = take_complete_tus(&mut buf);
        assert_eq!(tus.len(), 1);
        assert!(tu_is_keyframe(&tus[0]));
        assert!(sequence_header_obu(&tus[0]).is_some());
    }
}
