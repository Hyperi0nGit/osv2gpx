use chrono::{DateTime, TimeZone, Utc};

#[derive(Clone, Debug)]
pub struct GpsPoint {
    pub lat: f64,
    pub lon: f64,
    pub abs_alt: f64,
    pub rel_alt: f64,
    pub time: DateTime<Utc>,
}

#[derive(Clone, Debug, Default)]
struct ProtoField {
    number: u64,
    wire: u64,
    bytes: Vec<u8>,
    uint: u64,
    fixed32: u32,
    fixed64: u64,
}

pub fn extract_gps_point(bytes: &[u8]) -> Option<GpsPoint> {
    let payload = unwrap_embedded_mp4(bytes);
    let fields = parse_proto_fields(&payload)?;
    find_gps_in_fields(&fields, 0)
}

fn find_gps_in_fields(fields: &[ProtoField], depth: usize) -> Option<GpsPoint> {
    if let Some(point) = gps_from_telemetry_message(fields) {
        return Some(point);
    }
    if depth >= 8 {
        return None;
    }
    for field in fields {
        if field.wire != 2 || field.bytes.is_empty() {
            continue;
        }
        let children = match parse_proto_fields(&field.bytes) {
            Some(children) => children,
            None => continue,
        };
        if let Some(point) = find_gps_in_fields(&children, depth + 1) {
            return Some(point);
        }
    }
    None
}

fn gps_from_telemetry_message(fields: &[ProtoField]) -> Option<GpsPoint> {
    let location = fields.iter().find(|field| field.number == 4)?;
    let rel_alt = fields.iter().find(|field| field.number == 5);
    if location.wire != 2 {
        return None;
    }

    let loc_fields = parse_proto_fields(&location.bytes)?;
    let lat_lon_field = loc_fields.iter().find(|field| field.number == 1)?;
    let abs_alt_field = loc_fields
        .iter()
        .find(|field| field.number == 2 && field.wire == 0)?;
    if lat_lon_field.wire != 2 {
        return None;
    }

    let coord_fields = parse_proto_fields(&lat_lon_field.bytes)?;
    let mut lat = None;
    let mut lon = None;
    for field in &coord_fields {
        if field.wire != 1 {
            continue;
        }
        match field.number {
            2 => lat = Some(f64::from_bits(field.fixed64)),
            3 => lon = Some(f64::from_bits(field.fixed64)),
            _ => {}
        }
    }

    let lat = lat?;
    let lon = lon?;
    if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
        return None;
    }

    let mut point = GpsPoint {
        lat,
        lon,
        abs_alt: abs_alt_field.uint as f64 / 1000.0,
        rel_alt: 0.0,
        time: Utc.timestamp_opt(0, 0).single().unwrap(),
    };

    if let Some(rel_alt) = rel_alt {
        if rel_alt.wire == 2 {
            if let Some(rel_fields) = parse_proto_fields(&rel_alt.bytes) {
                for field in rel_fields {
                    if field.number == 1 && field.wire == 5 {
                        point.rel_alt = f32::from_bits(field.fixed32) as f64 / 1000.0;
                    }
                }
            }
        }
    }

    Some(point)
}

fn parse_proto_fields(bytes: &[u8]) -> Option<Vec<ProtoField>> {
    let mut fields = Vec::new();
    let mut pos = 0usize;
    while pos < bytes.len() {
        let (key, read) = read_varint(&bytes[pos..])?;
        pos += read;
        let field_no = key >> 3;
        let wire = key & 7;
        if field_no == 0 {
            return None;
        }

        let mut field = ProtoField {
            number: field_no,
            wire,
            ..ProtoField::default()
        };

        match wire {
            0 => {
                let (value, read) = read_varint(&bytes[pos..])?;
                field.uint = value;
                pos += read;
            }
            1 => {
                if pos + 8 > bytes.len() {
                    return None;
                }
                field.fixed64 = u64::from_le_bytes(bytes[pos..pos + 8].try_into().ok()?);
                pos += 8;
            }
            2 => {
                let (len, read) = read_varint(&bytes[pos..])?;
                pos += read;
                let len = len as usize;
                if pos + len > bytes.len() {
                    return None;
                }
                field.bytes = bytes[pos..pos + len].to_vec();
                pos += len;
            }
            5 => {
                if pos + 4 > bytes.len() {
                    return None;
                }
                field.fixed32 = u32::from_le_bytes(bytes[pos..pos + 4].try_into().ok()?);
                pos += 4;
            }
            _ => return None,
        }
        fields.push(field);
    }
    Some(fields)
}

fn unwrap_embedded_mp4(bytes: &[u8]) -> Vec<u8> {
    if bytes.len() < 16 || &bytes[4..8] != b"ftyp" {
        return bytes.to_vec();
    }
    let mut offset = 0usize;
    while offset + 8 <= bytes.len() {
        let size32 = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap());
        let typ = &bytes[offset + 4..offset + 8];
        let mut header_size = 8usize;
        let mut size = size32 as usize;
        if size32 == 1 {
            if offset + 16 > bytes.len() {
                return bytes.to_vec();
            }
            size = u64::from_be_bytes(bytes[offset + 8..offset + 16].try_into().unwrap()) as usize;
            header_size = 16;
        } else if size32 == 0 {
            size = bytes.len() - offset;
        }
        if size < header_size || offset + size > bytes.len() {
            return bytes.to_vec();
        }
        if typ == b"mdat" {
            return bytes[offset + header_size..offset + size].to_vec();
        }
        offset += size;
    }
    bytes.to_vec()
}

fn read_varint(bytes: &[u8]) -> Option<(u64, usize)> {
    let mut value = 0u64;
    for (idx, byte) in bytes.iter().copied().take(10).enumerate() {
        value |= u64::from(byte & 0x7f) << (7 * idx);
        if byte < 0x80 {
            return Some((value, idx + 1));
        }
    }
    None
}
