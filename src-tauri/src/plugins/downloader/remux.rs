//! Rewrites YouTube's fragmented (DASH) MP4 audio as a regular MP4, what
//! ffmpeg's `-c copy` (yt-dlp's FixupM4a) does: the samples of every
//! `moof`/`mdat` fragment go into one `mdat`, described by a rebuilt sample
//! table, and `mvex`/`sidx` are dropped. Windows' Media Foundation opens the
//! fragmented file but plays nothing (it jumps to the end).
//!
//! Handles what YouTube serves: one track, `trun` boxes with per-sample or
//! default sizes and durations. Anything else is left as it is.

const CONTAINERS: &[&[u8; 4]] = &[b"moov", b"trak", b"mdia", b"minf", b"stbl", b"moof", b"traf", b"mvex"];

struct Mp4Box {
    kind: [u8; 4],
    /// The payload (after the header), or the children of a container
    payload: Vec<u8>,
    children: Option<Vec<Mp4Box>>,
}

impl Mp4Box {
    fn leaf(kind: &[u8; 4], payload: Vec<u8>) -> Self {
        Self { kind: *kind, payload, children: None }
    }

    fn child(&self, kind: &[u8; 4]) -> Option<&Mp4Box> {
        self.children.as_ref()?.iter().find(|child| &child.kind == kind)
    }

    fn child_mut(&mut self, kind: &[u8; 4]) -> Option<&mut Mp4Box> {
        self.children.as_mut()?.iter_mut().find(|child| &child.kind == kind)
    }

    fn write(&self, out: &mut Vec<u8>) {
        let start = out.len();
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&self.kind);
        match &self.children {
            Some(children) => children.iter().for_each(|child| child.write(out)),
            None => out.extend_from_slice(&self.payload),
        }
        let size = (out.len() - start) as u32;
        out[start..start + 4].copy_from_slice(&size.to_be_bytes());
    }
}

/// A box in the input: its kind, where it starts, and its payload range
struct Span {
    kind: [u8; 4],
    start: usize,
    payload: std::ops::Range<usize>,
}

fn spans(data: &[u8], range: std::ops::Range<usize>) -> Result<Vec<Span>, String> {
    let mut boxes = vec![];
    let mut offset = range.start;
    while offset + 8 <= range.end {
        let mut size = u32::from_be_bytes(data[offset..offset + 4].try_into().unwrap()) as usize;
        let kind: [u8; 4] = data[offset + 4..offset + 8].try_into().unwrap();
        let mut header = 8;
        if size == 1 {
            let large = data.get(offset + 8..offset + 16).ok_or("truncated box")?;
            size = u64::from_be_bytes(large.try_into().unwrap()) as usize;
            header = 16;
        } else if size == 0 {
            size = range.end - offset;
        }
        if size < header || offset + size > range.end {
            return Err(format!("bad {} box", String::from_utf8_lossy(&kind)));
        }
        boxes.push(Span { kind, start: offset, payload: offset + header..offset + size });
        offset += size;
    }
    Ok(boxes)
}

fn tree(data: &[u8], span: &Span) -> Result<Mp4Box, String> {
    if CONTAINERS.contains(&&span.kind) {
        let children = spans(data, span.payload.clone())?
            .iter()
            .map(|child| tree(data, child))
            .collect::<Result<_, _>>()?;
        Ok(Mp4Box { kind: span.kind, payload: vec![], children: Some(children) })
    } else {
        Ok(Mp4Box::leaf(&span.kind, data[span.payload.clone()].to_vec()))
    }
}

struct Reader<'a> {
    data: &'a [u8],
    offset: usize,
}

impl Reader<'_> {
    fn u32(&mut self) -> Result<u32, String> {
        let bytes = self.data.get(self.offset..self.offset + 4).ok_or("truncated box")?;
        self.offset += 4;
        Ok(u32::from_be_bytes(bytes.try_into().unwrap()))
    }

    fn u64(&mut self) -> Result<u64, String> {
        Ok((u64::from(self.u32()?) << 32) | u64::from(self.u32()?))
    }
}

/// Version and flags of a full box
fn full_header(payload: &[u8]) -> Result<(u8, u32), String> {
    let word = u32::from_be_bytes(payload.get(..4).ok_or("truncated box")?.try_into().unwrap());
    Ok(((word >> 24) as u8, word & 0x00ff_ffff))
}

/// Writes a duration field of `mvhd`, `tkhd` or `mdhd` at the offsets
/// their version uses
fn set_duration(payload: &mut [u8], v0_offset: usize, v1_offset: usize, duration: u64) -> Result<(), String> {
    let (version, _) = full_header(payload)?;
    if version == 1 {
        payload.get_mut(v1_offset..v1_offset + 8).ok_or("truncated box")?.copy_from_slice(&duration.to_be_bytes());
    } else {
        let duration = u32::try_from(duration).map_err(|_| "duration too long")?;
        payload.get_mut(v0_offset..v0_offset + 4).ok_or("truncated box")?.copy_from_slice(&duration.to_be_bytes());
    }
    Ok(())
}

fn timescale(payload: &[u8], v0_offset: usize, v1_offset: usize) -> Result<u64, String> {
    let (version, _) = full_header(payload)?;
    let offset = if version == 1 { v1_offset } else { v0_offset };
    let mut reader = Reader { data: payload, offset };
    Ok(u64::from(reader.u32()?))
}

fn full_box(kind: &[u8; 4], body: &[u32]) -> Mp4Box {
    let mut payload = vec![0; 4];
    body.iter().for_each(|value| payload.extend_from_slice(&value.to_be_bytes()));
    Mp4Box::leaf(kind, payload)
}

/// Returns the regular MP4, or `None` when the file is not fragmented.
pub fn remux(data: &[u8]) -> Result<Option<Vec<u8>>, String> {
    let top = spans(data, 0..data.len())?;
    if !top.iter().any(|span| &span.kind == b"moof") {
        return Ok(None);
    }
    let moov_span = top.iter().find(|span| &span.kind == b"moov").ok_or("no moov box")?;
    let mut moov = tree(data, moov_span)?;
    let tracks = moov.children.as_ref().map_or(0, |children| children.iter().filter(|c| &c.kind == b"trak").count());
    if tracks != 1 {
        return Err(format!("{tracks} tracks"));
    }

    // trex defaults, overridden by tfhd, overridden by trun
    let (mut default_duration, mut default_size) = (0u32, 0u32);
    if let Some(trex) = moov.child(b"mvex").and_then(|mvex| mvex.child(b"trex")) {
        let mut reader = Reader { data: &trex.payload, offset: 12 };
        default_duration = reader.u32()?;
        default_size = reader.u32()?;
    }

    // Every sample, in order: where it is and how long it lasts
    let mut samples: Vec<(std::ops::Range<usize>, u32)> = vec![];
    for moof in top.iter().filter(|span| &span.kind == b"moof") {
        for traf in spans(data, moof.payload.clone())?.iter().filter(|span| &span.kind == b"traf") {
            let children = spans(data, traf.payload.clone())?;
            let tfhd = children.iter().find(|span| &span.kind == b"tfhd").ok_or("no tfhd box")?;
            let (_, flags) = full_header(&data[tfhd.payload.clone()])?;
            let mut reader = Reader { data: &data[tfhd.payload.clone()], offset: 8 };
            let base = if flags & 0x1 != 0 { reader.u64()? as usize } else { moof.start };
            if flags & 0x2 != 0 {
                reader.u32()?;
            }
            let duration = if flags & 0x8 != 0 { reader.u32()? } else { default_duration };
            let size = if flags & 0x10 != 0 { reader.u32()? } else { default_size };

            let mut position = base;
            for trun in children.iter().filter(|span| &span.kind == b"trun") {
                let payload = &data[trun.payload.clone()];
                let (_, flags) = full_header(payload)?;
                let mut reader = Reader { data: payload, offset: 4 };
                let count = reader.u32()?;
                if flags & 0x1 != 0 {
                    position = base.wrapping_add_signed(reader.u32()? as i32 as isize);
                }
                if flags & 0x4 != 0 {
                    reader.u32()?;
                }
                for _ in 0..count {
                    let sample_duration = if flags & 0x100 != 0 { reader.u32()? } else { duration };
                    let sample_size = if flags & 0x200 != 0 { reader.u32()? } else { size } as usize;
                    if flags & 0x400 != 0 {
                        reader.u32()?;
                    }
                    if flags & 0x800 != 0 {
                        reader.u32()?;
                    }
                    if position + sample_size > data.len() {
                        return Err("sample outside the file".into());
                    }
                    samples.push((position..position + sample_size, sample_duration));
                    position += sample_size;
                }
            }
        }
    }
    if samples.is_empty() {
        return Err("no samples".into());
    }

    // The sample table: durations run-length encoded, one chunk
    let mut stts: Vec<(u32, u32)> = vec![];
    for (_, duration) in &samples {
        match stts.last_mut() {
            Some((count, last)) if last == duration => *count += 1,
            _ => stts.push((1, *duration)),
        }
    }
    let media_duration: u64 = samples.iter().map(|(_, duration)| u64::from(*duration)).sum();
    let mut stts_body = vec![stts.len() as u32];
    stts.iter().for_each(|(count, duration)| stts_body.extend([*count, *duration]));
    let mut stsz_body = vec![0, samples.len() as u32];
    stsz_body.extend(samples.iter().map(|(range, _)| range.len() as u32));
    let sample_count = samples.len() as u32;

    let movie_timescale = timescale(&moov.child(b"mvhd").ok_or("no mvhd box")?.payload, 12, 20)?;
    moov.children.as_mut().unwrap().retain(|child| &child.kind != b"mvex");
    let trak = moov.child_mut(b"trak").unwrap();
    let mdia = trak.child_mut(b"mdia").ok_or("no mdia box")?;
    let mdhd = mdia.child_mut(b"mdhd").ok_or("no mdhd box")?;
    let media_timescale = timescale(&mdhd.payload, 12, 20)?;
    set_duration(&mut mdhd.payload, 16, 24, media_duration)?;
    let stbl = mdia
        .child_mut(b"minf")
        .and_then(|minf| minf.child_mut(b"stbl"))
        .ok_or("no stbl box")?;
    let stbl_children = stbl.children.as_mut().unwrap();
    stbl_children.retain(|child| !matches!(&child.kind, b"stts" | b"stsc" | b"stsz" | b"stco" | b"co64" | b"stss" | b"ctts"));
    stbl_children.push(full_box(b"stts", &stts_body));
    stbl_children.push(full_box(b"stsc", &[1, 1, sample_count, 1]));
    stbl_children.push(full_box(b"stsz", &stsz_body));
    stbl_children.push(full_box(b"stco", &[1, 0]));

    let movie_duration = media_duration * movie_timescale / media_timescale.max(1);
    set_duration(&mut moov.child_mut(b"mvhd").unwrap().payload, 16, 24, movie_duration)?;
    let trak = moov.child_mut(b"trak").unwrap();
    set_duration(&mut trak.child_mut(b"tkhd").ok_or("no tkhd box")?.payload, 20, 28, movie_duration)?;

    let mut ftyp = Mp4Box::leaf(b"ftyp", b"M4A ".to_vec());
    ftyp.payload.extend_from_slice(&0x200u32.to_be_bytes());
    ftyp.payload.extend_from_slice(b"M4A isomiso2mp41");

    // The chunk offset depends on the moov size, which it doesn't change
    let mut head = vec![];
    ftyp.write(&mut head);
    moov.write(&mut head);
    let mdat_payload: usize = samples.iter().map(|(range, _)| range.len()).sum();
    let large = mdat_payload + 8 > u32::MAX as usize;
    let chunk_offset = u32::try_from(head.len() + if large { 16 } else { 8 }).map_err(|_| "moov too large")?;
    let stbl = moov
        .child_mut(b"trak")
        .and_then(|trak| trak.child_mut(b"mdia"))
        .and_then(|mdia| mdia.child_mut(b"minf"))
        .and_then(|minf| minf.child_mut(b"stbl"))
        .unwrap();
    *stbl.child_mut(b"stco").unwrap() = full_box(b"stco", &[1, chunk_offset]);

    let mut out = Vec::with_capacity(head.len() + mdat_payload + 16);
    ftyp.write(&mut out);
    moov.write(&mut out);
    if large {
        out.extend_from_slice(&1u32.to_be_bytes());
        out.extend_from_slice(b"mdat");
        out.extend_from_slice(&(mdat_payload as u64 + 16).to_be_bytes());
    } else {
        out.extend_from_slice(&(mdat_payload as u32 + 8).to_be_bytes());
        out.extend_from_slice(b"mdat");
    }
    samples.iter().for_each(|(range, _)| out.extend_from_slice(&data[range.clone()]));
    Ok(Some(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boxed(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut out = ((payload.len() + 8) as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(payload);
        out
    }

    fn words(values: &[u32]) -> Vec<u8> {
        values.iter().flat_map(|value| value.to_be_bytes()).collect()
    }

    /// A fragment like YouTube's: default-base-is-moof, per-sample sizes
    fn fragment(sizes: &[u32], fill: u8) -> Vec<u8> {
        let tfhd = boxed(b"tfhd", &words(&[0x0002_002a, 1, 1, 1024, 0]));
        let trun_len = 8 + 12 + 4 * sizes.len();
        let traf_len = 8 + tfhd.len() + trun_len;
        let moof_len = 8 + traf_len;
        let mut trun = words(&[0x0000_0201, sizes.len() as u32, (moof_len + 8) as u32]);
        trun.extend(words(sizes));
        let traf = boxed(b"traf", &[tfhd, boxed(b"trun", &trun)].concat());
        let mut out = boxed(b"moof", &traf);
        let total: u32 = sizes.iter().sum();
        out.extend(boxed(b"mdat", &vec![fill; total as usize]));
        out
    }

    #[test]
    fn joins_the_fragments_into_one_sample_table() {
        let mvhd = words(&[0, 0, 0, 44100, 0, 0x0001_0000]);
        let tkhd = words(&[0, 0, 0, 1, 0, 0, 0]);
        let mdhd = words(&[0, 0, 0, 44100, 0, 0]);
        let stbl = [
            boxed(b"stsd", &words(&[0, 0])),
            boxed(b"stts", &words(&[0, 0])),
            boxed(b"stsc", &words(&[0, 0])),
            boxed(b"stco", &words(&[0, 0])),
            boxed(b"stsz", &words(&[0, 0, 0])),
        ]
        .concat();
        let minf = boxed(b"minf", &boxed(b"stbl", &stbl));
        let mdia = boxed(b"mdia", &[boxed(b"mdhd", &mdhd), minf].concat());
        let trak = boxed(b"trak", &[boxed(b"tkhd", &tkhd), mdia].concat());
        let mvex = boxed(b"mvex", &boxed(b"trex", &words(&[0, 1, 1, 0, 0, 0])));
        let moov = boxed(b"moov", &[boxed(b"mvhd", &mvhd), mvex, trak].concat());
        let input = [
            boxed(b"ftyp", b"dash\0\0\0\0iso6mp41"),
            moov,
            boxed(b"sidx", &[0; 12]),
            fragment(&[3, 4], 0xaa),
            fragment(&[5], 0xbb),
        ]
        .concat();

        let output = remux(&input).unwrap().unwrap();
        let top = spans(&output, 0..output.len()).unwrap();
        let kinds: Vec<&[u8; 4]> = top.iter().map(|span| &span.kind).collect();
        assert_eq!(kinds, [b"ftyp", b"moov", b"mdat"]);
        let mdat = &output[top[2].payload.clone()];
        assert_eq!(mdat, [[0xaa; 7].as_slice(), &[0xbb; 5]].concat());

        let moov = tree(&output, &top[1]).unwrap();
        assert!(moov.child(b"mvex").is_none());
        let stbl = moov.child(b"trak").unwrap().child(b"mdia").unwrap().child(b"minf").unwrap().child(b"stbl").unwrap();
        assert_eq!(stbl.child(b"stts").unwrap().payload, words(&[0, 1, 3, 1024]));
        assert_eq!(stbl.child(b"stsz").unwrap().payload, words(&[0, 0, 3, 3, 4, 5]));
        assert_eq!(stbl.child(b"stsc").unwrap().payload, words(&[0, 1, 1, 3, 1]));
        assert_eq!(stbl.child(b"stco").unwrap().payload, words(&[0, 1, top[2].payload.start as u32]));
        let mdhd = &moov.child(b"trak").unwrap().child(b"mdia").unwrap().child(b"mdhd").unwrap().payload;
        assert_eq!(&mdhd[16..20], &3072u32.to_be_bytes());
    }

    #[test]
    fn leaves_regular_files_alone() {
        let input = [boxed(b"ftyp", b"M4A \0\0\0\0"), boxed(b"moov", &[]), boxed(b"mdat", &[1, 2])].concat();
        assert!(remux(&input).unwrap().is_none());
    }
}
