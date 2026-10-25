//! Split Annex-B H.264 access units from a concatenated byte stream.

pub fn take_complete_aus(buf: &mut Vec<u8>) -> Vec<Vec<u8>> {
    let starts = find_start_codes(buf);
    if starts.len() < 2 {
        return Vec::new();
    }
    let mut aus = Vec::new();
    let mut au_begin = 0usize;
    for i in 0..starts.len() - 1 {
        let payload = starts[i].0 + starts[i].1;
        if payload >= buf.len() {
            break;
        }
        let nal_type = buf[payload] & 0x1F;
        if nal_type == 1 || nal_type == 5 {
            let begin = starts[au_begin].0;
            let end = starts[i + 1].0;
            if end > begin {
                aus.push(buf[begin..end].to_vec());
            }
            au_begin = i + 1;
        }
    }
    let keep = starts[au_begin].0;
    buf.drain(..keep);
    aus
}

fn find_start_codes(buf: &[u8]) -> Vec<(usize, usize)> {
    let mut starts = Vec::new();
    let mut i = 0;
    while i + 2 < buf.len() {
        if i + 3 < buf.len() && buf[i] == 0 && buf[i + 1] == 0 && buf[i + 2] == 0 && buf[i + 3] == 1
        {
            starts.push((i, 4));
            i += 4;
            continue;
        }
        if buf[i] == 0 && buf[i + 1] == 0 && buf[i + 2] == 1 {
            starts.push((i, 3));
            i += 3;
            continue;
        }
        i += 1;
    }
    starts
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn groups_sps_and_idr() {
        let mut buf = vec![
            0, 0, 0, 1, 0x67, 0x42, 0, 0, 0, 1, 0x65, 0x88, 0, 0, 0, 1, 0x61, 0x11,
        ];
        let aus = take_complete_aus(&mut buf);
        assert_eq!(aus.len(), 1);
        let key = aus[0].windows(5).any(|w| w[0] == 0 && w[4] == 0x65);
        assert!(key);
        assert_eq!(buf, vec![0, 0, 0, 1, 0x61, 0x11]);
    }
}
