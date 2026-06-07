use chrono::{DateTime, Duration, TimeZone, Timelike, Utc};
use std::error::Error;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};

pub type Mp4Result<T> = Result<T, Box<dyn Error>>;

#[derive(Clone, Debug)]
pub struct BoxHeader {
    pub typ: String,
    pub header_size: u64,
    pub size: u64,
    pub start: u64,
    pub end: u64,
}

#[derive(Clone, Debug, Default)]
pub struct Track {
    pub id: u32,
    pub handler: String,
    pub name: String,
    pub sample_entry: String,
    pub timescale: u32,
    pub sample_count: usize,
    pub sizes: Vec<u32>,
    pub chunk_offsets: Vec<u64>,
    pub stsc: Vec<StscEntry>,
    pub stts: Vec<SttsEntry>,
}

#[derive(Clone, Debug)]
pub struct StscEntry {
    pub first_chunk: u32,
    pub samples_per_chunk: u32,
    pub sample_description_index: u32,
}

#[derive(Clone, Debug)]
pub struct SttsEntry {
    pub sample_count: u32,
    pub sample_delta: u32,
}

#[derive(Clone, Debug)]
pub struct SampleRef {
    pub index: usize,
    pub offset: u64,
    pub size: u32,
    pub time: f64,
}

pub fn parse_tracks(file: &mut File) -> Mp4Result<Vec<Track>> {
    let size = reader_size(file)?;
    let mut tracks = Vec::new();
    for top in read_boxes(file, 0, size) {
        if top.typ != "moov" {
            continue;
        }
        for child in read_boxes(file, top.start + top.header_size, top.end) {
            if child.typ != "trak" {
                continue;
            }
            tracks.push(parse_track(file, &child)?);
        }
    }
    Ok(tracks)
}

pub fn parse_movie_creation_time(file: &mut File) -> Mp4Result<DateTime<Utc>> {
    let size = reader_size(file)?;
    for top in read_boxes(file, 0, size) {
        if top.typ != "moov" {
            continue;
        }
        for child in read_boxes(file, top.start + top.header_size, top.end) {
            if child.typ != "mvhd" {
                continue;
            }
            let mut bytes = read_at(file, child.start + child.header_size, 20)?;
            let seconds = if bytes[0] == 1 {
                bytes = read_at(file, child.start + child.header_size, 28)?;
                u64::from_be_bytes(bytes[4..12].try_into()?)
            } else {
                u32::from_be_bytes(bytes[4..8].try_into()?) as u64
            };
            return Ok(quicktime_epoch() + Duration::seconds(seconds as i64));
        }
    }
    Err("mvhd creation time not found".into())
}

pub fn select_track(tracks: &[Track], id: u32) -> Mp4Result<&Track> {
    if id != 0 {
        return tracks
            .iter()
            .find(|track| track.id == id)
            .ok_or_else(|| format!("track id {} not found", id).into());
    }
    tracks
        .iter()
        .find(|track| track.sample_entry == "djmd" || track.handler == "meta")
        .ok_or_else(|| "no djmd/meta track found".into())
}

pub fn samples_for_track(track: &Track) -> Mp4Result<Vec<SampleRef>> {
    if track.sizes.is_empty() {
        return Err("track has no sample sizes".into());
    }
    if track.chunk_offsets.is_empty() {
        return Err("track has no chunk offsets".into());
    }
    if track.stsc.is_empty() {
        return Err("track has no sample-to-chunk table".into());
    }

    let mut stsc = track.stsc.clone();
    stsc.sort_by_key(|entry| entry.first_chunk);

    let times = sample_times(track);
    let mut refs = Vec::with_capacity(track.sizes.len());
    let mut sample_index = 0usize;

    for (chunk_idx, chunk_offset) in track.chunk_offsets.iter().copied().enumerate() {
        let chunk_no = (chunk_idx + 1) as u32;
        let mut entry = &stsc[0];
        for candidate in &stsc {
            if candidate.first_chunk <= chunk_no {
                entry = candidate;
            }
        }

        let mut offset = chunk_offset;
        for _ in 0..entry.samples_per_chunk {
            if sample_index >= track.sizes.len() {
                break;
            }
            let size = track.sizes[sample_index];
            let sample_time = times.get(sample_index).copied().unwrap_or(0.0);
            refs.push(SampleRef {
                index: sample_index + 1,
                offset,
                size,
                time: sample_time,
            });
            offset += size as u64;
            sample_index += 1;
        }
    }

    if refs.len() != track.sizes.len() {
        return Err(format!(
            "built {} sample refs for {} sample sizes",
            refs.len(),
            track.sizes.len()
        )
        .into());
    }
    Ok(refs)
}

pub fn write_mp4_creation_time(file: &mut File, time: DateTime<Utc>) -> Mp4Result<usize> {
    let size = reader_size(file)?;
    let seconds = quicktime_seconds(time)?;
    let updated = patch_mp4_time_boxes(file, 0, size, seconds)?;
    if updated == 0 {
        return Err("no mvhd/tkhd/mdhd time fields found".into());
    }
    Ok(updated)
}

pub fn read_at(file: &mut File, offset: u64, len: usize) -> Mp4Result<Vec<u8>> {
    let mut bytes = vec![0; len];
    file.seek(SeekFrom::Start(offset))?;
    file.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn parse_track(file: &mut File, trak: &BoxHeader) -> Mp4Result<Track> {
    let mut track = Track::default();
    for box_header in read_boxes(file, trak.start + trak.header_size, trak.end) {
        match box_header.typ.as_str() {
            "tkhd" => track.id = parse_tkhd(file, &box_header)?,
            "mdia" => parse_mdia(file, &box_header, &mut track)?,
            _ => {}
        }
    }
    track.sample_count = track.sizes.len();
    Ok(track)
}

fn parse_mdia(file: &mut File, mdia: &BoxHeader, track: &mut Track) -> Mp4Result<()> {
    for box_header in read_boxes(file, mdia.start + mdia.header_size, mdia.end) {
        match box_header.typ.as_str() {
            "mdhd" => track.timescale = parse_mdhd(file, &box_header)?,
            "hdlr" => {
                let (handler, name) = parse_hdlr(file, &box_header)?;
                track.handler = handler;
                track.name = name;
            }
            "minf" => parse_minf(file, &box_header, track)?,
            _ => {}
        }
    }
    Ok(())
}

fn parse_minf(file: &mut File, minf: &BoxHeader, track: &mut Track) -> Mp4Result<()> {
    for box_header in read_boxes(file, minf.start + minf.header_size, minf.end) {
        if box_header.typ != "stbl" {
            continue;
        }
        for stbl in read_boxes(
            file,
            box_header.start + box_header.header_size,
            box_header.end,
        ) {
            match stbl.typ.as_str() {
                "stsd" => track.sample_entry = parse_stsd(file, &stbl)?,
                "stsz" => track.sizes = parse_stsz(file, &stbl)?,
                "stco" => track.chunk_offsets = parse_stco(file, &stbl)?,
                "co64" => track.chunk_offsets = parse_co64(file, &stbl)?,
                "stsc" => track.stsc = parse_stsc(file, &stbl)?,
                "stts" => track.stts = parse_stts(file, &stbl)?,
                _ => {}
            }
        }
    }
    Ok(())
}

fn read_boxes(file: &mut File, start: u64, end: u64) -> Vec<BoxHeader> {
    let mut boxes = Vec::new();
    let mut offset = start;
    while offset + 8 <= end {
        let bytes = match read_at(file, offset, 16) {
            Ok(bytes) => bytes,
            Err(_) => break,
        };
        let size32 = u32::from_be_bytes(bytes[0..4].try_into().unwrap());
        let typ = String::from_utf8_lossy(&bytes[4..8]).into_owned();
        let mut header_size = 8u64;
        let mut size = size32 as u64;
        if size32 == 1 {
            size = u64::from_be_bytes(bytes[8..16].try_into().unwrap());
            header_size = 16;
        } else if size32 == 0 {
            size = end - offset;
        }
        if size < header_size || offset.saturating_add(size) > end {
            break;
        }
        boxes.push(BoxHeader {
            typ,
            header_size,
            size,
            start: offset,
            end: offset + size,
        });
        offset += size;
    }
    boxes
}

fn parse_tkhd(file: &mut File, box_header: &BoxHeader) -> Mp4Result<u32> {
    let mut bytes = read_at(file, box_header.start + box_header.header_size, 24)?;
    if bytes[0] == 1 {
        bytes = read_at(file, box_header.start + box_header.header_size, 36)?;
        return Ok(u32::from_be_bytes(bytes[20..24].try_into()?));
    }
    Ok(u32::from_be_bytes(bytes[12..16].try_into()?))
}

fn parse_mdhd(file: &mut File, box_header: &BoxHeader) -> Mp4Result<u32> {
    let mut bytes = read_at(file, box_header.start + box_header.header_size, 24)?;
    if bytes[0] == 1 {
        bytes = read_at(file, box_header.start + box_header.header_size, 32)?;
        return Ok(u32::from_be_bytes(bytes[20..24].try_into()?));
    }
    Ok(u32::from_be_bytes(bytes[12..16].try_into()?))
}

fn parse_hdlr(file: &mut File, box_header: &BoxHeader) -> Mp4Result<(String, String)> {
    let len = (box_header.size - box_header.header_size) as usize;
    let bytes = read_at(file, box_header.start + box_header.header_size, len)?;
    if bytes.len() < 24 {
        return Err("short hdlr".into());
    }
    let handler = String::from_utf8_lossy(&bytes[8..12]).into_owned();
    let name = if bytes.len() > 24 {
        String::from_utf8_lossy(&bytes[24..])
            .trim_end_matches('\0')
            .to_string()
    } else {
        String::new()
    };
    Ok((handler, name))
}

fn parse_stsd(file: &mut File, box_header: &BoxHeader) -> Mp4Result<String> {
    let bytes = read_at(file, box_header.start + box_header.header_size, 16)?;
    if u32::from_be_bytes(bytes[4..8].try_into()?) == 0 {
        return Ok(String::new());
    }
    Ok(String::from_utf8_lossy(&bytes[12..16]).into_owned())
}

fn parse_stsz(file: &mut File, box_header: &BoxHeader) -> Mp4Result<Vec<u32>> {
    let header = read_at(file, box_header.start + box_header.header_size, 12)?;
    let sample_size = u32::from_be_bytes(header[4..8].try_into()?);
    let count = u32::from_be_bytes(header[8..12].try_into()?) as usize;
    let mut sizes = vec![sample_size; count];
    if sample_size != 0 {
        return Ok(sizes);
    }
    let bytes = read_at(
        file,
        box_header.start + box_header.header_size + 12,
        count * 4,
    )?;
    for (idx, size) in sizes.iter_mut().enumerate() {
        let base = idx * 4;
        *size = u32::from_be_bytes(bytes[base..base + 4].try_into()?);
    }
    Ok(sizes)
}

fn parse_stco(file: &mut File, box_header: &BoxHeader) -> Mp4Result<Vec<u64>> {
    let header = read_at(file, box_header.start + box_header.header_size, 8)?;
    let count = u32::from_be_bytes(header[4..8].try_into()?) as usize;
    let bytes = read_at(
        file,
        box_header.start + box_header.header_size + 8,
        count * 4,
    )?;
    let mut out = vec![0; count];
    for (idx, value) in out.iter_mut().enumerate() {
        let base = idx * 4;
        *value = u32::from_be_bytes(bytes[base..base + 4].try_into()?) as u64;
    }
    Ok(out)
}

fn parse_co64(file: &mut File, box_header: &BoxHeader) -> Mp4Result<Vec<u64>> {
    let header = read_at(file, box_header.start + box_header.header_size, 8)?;
    let count = u32::from_be_bytes(header[4..8].try_into()?) as usize;
    let bytes = read_at(
        file,
        box_header.start + box_header.header_size + 8,
        count * 8,
    )?;
    let mut out = vec![0; count];
    for (idx, value) in out.iter_mut().enumerate() {
        let base = idx * 8;
        *value = u64::from_be_bytes(bytes[base..base + 8].try_into()?);
    }
    Ok(out)
}

fn parse_stsc(file: &mut File, box_header: &BoxHeader) -> Mp4Result<Vec<StscEntry>> {
    let header = read_at(file, box_header.start + box_header.header_size, 8)?;
    let count = u32::from_be_bytes(header[4..8].try_into()?) as usize;
    let bytes = read_at(
        file,
        box_header.start + box_header.header_size + 8,
        count * 12,
    )?;
    let mut out = Vec::with_capacity(count);
    for idx in 0..count {
        let base = idx * 12;
        out.push(StscEntry {
            first_chunk: u32::from_be_bytes(bytes[base..base + 4].try_into()?),
            samples_per_chunk: u32::from_be_bytes(bytes[base + 4..base + 8].try_into()?),
            sample_description_index: u32::from_be_bytes(bytes[base + 8..base + 12].try_into()?),
        });
    }
    Ok(out)
}

fn parse_stts(file: &mut File, box_header: &BoxHeader) -> Mp4Result<Vec<SttsEntry>> {
    let header = read_at(file, box_header.start + box_header.header_size, 8)?;
    let count = u32::from_be_bytes(header[4..8].try_into()?) as usize;
    let bytes = read_at(
        file,
        box_header.start + box_header.header_size + 8,
        count * 8,
    )?;
    let mut out = Vec::with_capacity(count);
    for idx in 0..count {
        let base = idx * 8;
        out.push(SttsEntry {
            sample_count: u32::from_be_bytes(bytes[base..base + 4].try_into()?),
            sample_delta: u32::from_be_bytes(bytes[base + 4..base + 8].try_into()?),
        });
    }
    Ok(out)
}

fn sample_times(track: &Track) -> Vec<f64> {
    let mut times = vec![0.0; track.sizes.len()];
    if track.timescale == 0 {
        return times;
    }
    let mut ticks = 0u64;
    let mut idx = 0usize;
    for entry in &track.stts {
        for _ in 0..entry.sample_count {
            if idx >= times.len() {
                break;
            }
            times[idx] = ticks as f64 / track.timescale as f64;
            ticks += entry.sample_delta as u64;
            idx += 1;
        }
    }
    times
}

fn patch_mp4_time_boxes(file: &mut File, start: u64, end: u64, seconds: u64) -> Mp4Result<usize> {
    let mut updated = 0usize;
    for box_header in read_boxes(file, start, end) {
        match box_header.typ.as_str() {
            "mvhd" | "tkhd" | "mdhd" => {
                updated += patch_full_box_times(file, &box_header, seconds)?
            }
            "moov" | "trak" | "mdia" => {
                updated += patch_mp4_time_boxes(
                    file,
                    box_header.start + box_header.header_size,
                    box_header.end,
                    seconds,
                )?;
            }
            _ => {}
        }
    }
    Ok(updated)
}

fn patch_full_box_times(file: &mut File, box_header: &BoxHeader, seconds: u64) -> Mp4Result<usize> {
    let bytes = read_at(file, box_header.start + box_header.header_size, 1)?;
    let base = box_header.start + box_header.header_size;
    if bytes[0] == 1 {
        write_u64_at(file, base + 4, seconds)?;
        write_u64_at(file, base + 12, seconds)?;
        return Ok(2);
    }
    if seconds > u32::MAX as u64 {
        return Err(format!(
            "{} uses 32-bit time fields but {} exceeds uint32",
            box_header.typ, seconds
        )
        .into());
    }
    write_u32_at(file, base + 4, seconds as u32)?;
    write_u32_at(file, base + 8, seconds as u32)?;
    Ok(2)
}

fn quicktime_seconds(time: DateTime<Utc>) -> Mp4Result<u64> {
    let epoch = quicktime_epoch();
    let time = time
        .with_nanosecond(0)
        .ok_or("failed to truncate time to seconds")?;
    if time < epoch {
        return Err(format!(
            "time {} is before QuickTime epoch",
            time.format("%Y-%m-%dT%H:%M:%SZ")
        )
        .into());
    }
    Ok((time - epoch).num_seconds() as u64)
}

fn write_u32_at(file: &mut File, offset: u64, value: u32) -> Mp4Result<()> {
    file.seek(SeekFrom::Start(offset))?;
    file.write_all(&value.to_be_bytes())?;
    Ok(())
}

fn write_u64_at(file: &mut File, offset: u64, value: u64) -> Mp4Result<()> {
    file.seek(SeekFrom::Start(offset))?;
    file.write_all(&value.to_be_bytes())?;
    Ok(())
}

fn reader_size(file: &File) -> Mp4Result<u64> {
    Ok(file.metadata()?.len())
}

fn quicktime_epoch() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(1904, 1, 1, 0, 0, 0).single().unwrap()
}
