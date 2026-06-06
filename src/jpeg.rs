use crate::gps::GpsPoint;
use chrono::Timelike;
use std::error::Error;
use std::fs;
use std::path::Path;

pub type JpegResult<T> = Result<T, Box<dyn Error>>;

const APP1: u8 = 0xe1;
const SOS: u8 = 0xda;
const XMP_HEADER: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";

pub fn write_gps_exif(path: &Path, point: &GpsPoint) -> JpegResult<()> {
    let bytes = fs::read(path)?;
    let dimensions = jpeg_dimensions(&bytes)?;
    let updated = replace_metadata_app1s(
        &bytes,
        &build_exif_payload(point)?,
        &build_gpano_xmp_payload(dimensions),
    )?;
    fs::write(path, updated)?;
    Ok(())
}

fn replace_metadata_app1s(
    jpeg: &[u8],
    exif_payload: &[u8],
    xmp_payload: &[u8],
) -> JpegResult<Vec<u8>> {
    if jpeg.len() < 4 || jpeg[0] != 0xff || jpeg[1] != 0xd8 {
        return Err("not a JPEG file".into());
    }
    if exif_payload.len() + 2 > u16::MAX as usize {
        return Err("EXIF payload is too large for JPEG APP1".into());
    }
    if xmp_payload.len() + 2 > u16::MAX as usize {
        return Err("XMP payload is too large for JPEG APP1".into());
    }

    let mut out = Vec::with_capacity(jpeg.len() + exif_payload.len() + xmp_payload.len() + 8);
    out.extend_from_slice(&jpeg[..2]);
    write_app1_segment(&mut out, exif_payload);
    write_app1_segment(&mut out, xmp_payload);

    let mut pos = 2usize;
    while pos < jpeg.len() {
        if pos + 4 > jpeg.len() || jpeg[pos] != 0xff {
            out.extend_from_slice(&jpeg[pos..]);
            break;
        }

        let marker = jpeg[pos + 1];
        if marker == SOS {
            out.extend_from_slice(&jpeg[pos..]);
            break;
        }

        if is_standalone_marker(marker) {
            out.extend_from_slice(&jpeg[pos..pos + 2]);
            pos += 2;
            continue;
        }

        let len = u16::from_be_bytes([jpeg[pos + 2], jpeg[pos + 3]]) as usize;
        if len < 2 || pos + 2 + len > jpeg.len() {
            return Err("invalid JPEG segment length".into());
        }
        let payload_start = pos + 4;
        let payload_end = pos + 2 + len;
        let is_exif_app1 =
            marker == APP1 && jpeg[payload_start..payload_end].starts_with(b"Exif\0\0");
        let is_xmp_app1 =
            marker == APP1 && jpeg[payload_start..payload_end].starts_with(XMP_HEADER);
        if !is_exif_app1 && !is_xmp_app1 {
            out.extend_from_slice(&jpeg[pos..payload_end]);
        }
        pos = payload_end;
    }

    Ok(out)
}

fn write_app1_segment(out: &mut Vec<u8>, payload: &[u8]) {
    out.push(0xff);
    out.push(APP1);
    out.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
    out.extend_from_slice(payload);
}

fn is_standalone_marker(marker: u8) -> bool {
    marker == 0x01 || marker == 0xd8 || marker == 0xd9 || (0xd0..=0xd7).contains(&marker)
}

#[derive(Clone, Copy)]
struct JpegDimensions {
    width: u16,
    height: u16,
}

fn jpeg_dimensions(jpeg: &[u8]) -> JpegResult<JpegDimensions> {
    if jpeg.len() < 4 || jpeg[0] != 0xff || jpeg[1] != 0xd8 {
        return Err("not a JPEG file".into());
    }

    let mut pos = 2usize;
    while pos + 4 <= jpeg.len() && jpeg[pos] == 0xff {
        let marker = jpeg[pos + 1];
        if marker == SOS {
            break;
        }
        if is_standalone_marker(marker) {
            pos += 2;
            continue;
        }

        let len = u16::from_be_bytes([jpeg[pos + 2], jpeg[pos + 3]]) as usize;
        if len < 2 || pos + 2 + len > jpeg.len() {
            return Err("invalid JPEG segment length".into());
        }
        if is_sof_marker(marker) {
            let payload_start = pos + 4;
            if payload_start + 5 > jpeg.len() {
                return Err("short JPEG SOF segment".into());
            }
            let height = u16::from_be_bytes([jpeg[payload_start + 1], jpeg[payload_start + 2]]);
            let width = u16::from_be_bytes([jpeg[payload_start + 3], jpeg[payload_start + 4]]);
            return Ok(JpegDimensions { width, height });
        }
        pos += 2 + len;
    }

    Err("JPEG dimensions not found".into())
}

fn is_sof_marker(marker: u8) -> bool {
    matches!(
        marker,
        0xc0 | 0xc1 | 0xc2 | 0xc3 | 0xc5 | 0xc6 | 0xc7 | 0xc9 | 0xca | 0xcb | 0xcd | 0xce | 0xcf
    )
}

fn build_exif_payload(point: &GpsPoint) -> JpegResult<Vec<u8>> {
    let mut tiff = Vec::new();
    tiff.extend_from_slice(b"MM");
    push_u16(&mut tiff, 42);
    push_u32(&mut tiff, 8);

    let ifd0_offset = 8usize;
    let ifd0_len = 2 + 2 * 12 + 4;
    let exif_ifd_offset = ifd0_offset + ifd0_len;
    let exif_ifd_len = 2 + 1 * 12 + 4;
    let gps_ifd_offset = exif_ifd_offset + exif_ifd_len + 20;
    let gps_ifd_len = 2 + 9 * 12 + 4;
    let lat_data_offset = gps_ifd_offset + gps_ifd_len;
    let lon_data_offset = lat_data_offset + 24;
    let alt_data_offset = lon_data_offset + 24;
    let gps_time_offset = alt_data_offset + 8;
    let gps_date_offset = gps_time_offset + 24;

    push_u16(&mut tiff, 2);
    push_ifd_entry(
        &mut tiff,
        0x8769,
        4,
        1,
        Value::Offset(exif_ifd_offset as u32),
    );
    push_ifd_entry(
        &mut tiff,
        0x8825,
        4,
        1,
        Value::Offset(gps_ifd_offset as u32),
    );
    push_u32(&mut tiff, 0);

    let date_time = point.time.format("%Y:%m:%d %H:%M:%S").to_string();
    push_u16(&mut tiff, 1);
    push_ifd_entry(
        &mut tiff,
        0x9003,
        2,
        20,
        Value::Offset((exif_ifd_offset + exif_ifd_len) as u32),
    );
    push_u32(&mut tiff, 0);
    push_ascii(&mut tiff, &date_time, 20)?;

    let lat_ref = if point.lat < 0.0 { b"S\0" } else { b"N\0" };
    let lon_ref = if point.lon < 0.0 { b"W\0" } else { b"E\0" };
    push_u16(&mut tiff, 9);
    push_ifd_entry(&mut tiff, 0x0000, 1, 4, Value::Inline([2, 3, 0, 0]));
    push_ifd_entry(
        &mut tiff,
        0x0001,
        2,
        2,
        Value::Inline([lat_ref[0], 0, 0, 0]),
    );
    push_ifd_entry(
        &mut tiff,
        0x0002,
        5,
        3,
        Value::Offset(lat_data_offset as u32),
    );
    push_ifd_entry(
        &mut tiff,
        0x0003,
        2,
        2,
        Value::Inline([lon_ref[0], 0, 0, 0]),
    );
    push_ifd_entry(
        &mut tiff,
        0x0004,
        5,
        3,
        Value::Offset(lon_data_offset as u32),
    );
    push_ifd_entry(
        &mut tiff,
        0x0005,
        1,
        1,
        Value::Inline([if point.abs_alt < 0.0 { 1 } else { 0 }, 0, 0, 0]),
    );
    push_ifd_entry(
        &mut tiff,
        0x0006,
        5,
        1,
        Value::Offset(alt_data_offset as u32),
    );
    push_ifd_entry(
        &mut tiff,
        0x0007,
        5,
        3,
        Value::Offset(gps_time_offset as u32),
    );
    push_ifd_entry(
        &mut tiff,
        0x001d,
        2,
        11,
        Value::Offset(gps_date_offset as u32),
    );
    push_u32(&mut tiff, 0);

    push_dms_rationals(&mut tiff, point.lat.abs());
    push_dms_rationals(&mut tiff, point.lon.abs());
    push_rational(
        &mut tiff,
        (point.abs_alt.abs() * 1000.0).round() as u32,
        1000,
    );
    push_rational(&mut tiff, point.time.hour(), 1);
    push_rational(&mut tiff, point.time.minute(), 1);
    push_rational(
        &mut tiff,
        point.time.second() * 1000 + point.time.timestamp_subsec_millis(),
        1000,
    );
    push_ascii(&mut tiff, &point.time.format("%Y:%m:%d").to_string(), 11)?;

    let mut payload = Vec::with_capacity(tiff.len() + 6);
    payload.extend_from_slice(b"Exif\0\0");
    payload.extend_from_slice(&tiff);
    Ok(payload)
}

fn build_gpano_xmp_payload(dimensions: JpegDimensions) -> Vec<u8> {
    let width = dimensions.width;
    let height = dimensions.height;
    let packet = format!(
        r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
  <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
    <rdf:Description rdf:about=""
      xmlns:GPano="http://ns.google.com/photos/1.0/panorama/"
      GPano:ProjectionType="equirectangular"
      GPano:UsePanoramaViewer="True"
      GPano:CroppedAreaImageWidthPixels="{width}"
      GPano:CroppedAreaImageHeightPixels="{height}"
      GPano:FullPanoWidthPixels="{width}"
      GPano:FullPanoHeightPixels="{height}"
      GPano:CroppedAreaLeftPixels="0"
      GPano:CroppedAreaTopPixels="0"/>
  </rdf:RDF>
</x:xmpmeta>
<?xpacket end="w"?>"#
    );

    let mut payload = Vec::with_capacity(XMP_HEADER.len() + packet.len());
    payload.extend_from_slice(XMP_HEADER);
    payload.extend_from_slice(packet.as_bytes());
    payload
}

enum Value {
    Inline([u8; 4]),
    Offset(u32),
}

fn push_ifd_entry(out: &mut Vec<u8>, tag: u16, typ: u16, count: u32, value: Value) {
    push_u16(out, tag);
    push_u16(out, typ);
    push_u32(out, count);
    match value {
        Value::Inline(bytes) => out.extend_from_slice(&bytes),
        Value::Offset(offset) => push_u32(out, offset),
    }
}

fn push_ascii(out: &mut Vec<u8>, value: &str, count: usize) -> JpegResult<()> {
    let bytes = value.as_bytes();
    if bytes.len() + 1 != count {
        return Err(format!(
            "ASCII EXIF value {:?} does not match count {}",
            value, count
        )
        .into());
    }
    out.extend_from_slice(bytes);
    out.push(0);
    Ok(())
}

fn push_dms_rationals(out: &mut Vec<u8>, degrees_decimal: f64) {
    let degrees = degrees_decimal.floor();
    let minutes_decimal = (degrees_decimal - degrees) * 60.0;
    let minutes = minutes_decimal.floor();
    let seconds = (minutes_decimal - minutes) * 60.0;
    push_rational(out, degrees as u32, 1);
    push_rational(out, minutes as u32, 1);
    push_rational(out, (seconds * 10_000_000.0).round() as u32, 10_000_000);
}

fn push_rational(out: &mut Vec<u8>, numerator: u32, denominator: u32) {
    push_u32(out, numerator);
    push_u32(out, denominator);
}

fn push_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_be_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gps::GpsPoint;
    use chrono::{TimeZone, Utc};

    #[test]
    fn replaces_existing_exif_and_xmp_app1() {
        let point = GpsPoint {
            lat: 24.5,
            lon: 121.5,
            abs_alt: 100.0,
            rel_alt: 0.0,
            time: Utc.with_ymd_and_hms(2026, 5, 27, 9, 23, 16).unwrap(),
        };
        let exif_payload = build_exif_payload(&point).unwrap();
        let xmp_payload = build_gpano_xmp_payload(JpegDimensions {
            width: 8192,
            height: 4096,
        });
        let old_exif = b"Exif\0\0old";
        let old_xmp = b"http://ns.adobe.com/xap/1.0/\0old";
        let mut jpeg = vec![0xff, 0xd8, 0xff, APP1, 0, (old_exif.len() + 2) as u8];
        jpeg.extend_from_slice(old_exif);
        jpeg.extend_from_slice(&[0xff, APP1, 0, (old_xmp.len() + 2) as u8]);
        jpeg.extend_from_slice(old_xmp);
        jpeg.extend_from_slice(&[0xff, SOS, 0, 2, 1, 2, 3]);

        let updated = replace_metadata_app1s(&jpeg, &exif_payload, &xmp_payload).unwrap();
        let exif_count = updated
            .windows(b"Exif\0\0".len())
            .filter(|window| *window == b"Exif\0\0")
            .count();
        let xmp_count = updated
            .windows(XMP_HEADER.len())
            .filter(|window| *window == XMP_HEADER)
            .count();

        assert_eq!(exif_count, 1);
        assert_eq!(xmp_count, 1);
        assert!(updated[6..].starts_with(&exif_payload));
        assert!(
            updated
                .windows(xmp_payload.len())
                .any(|window| window == xmp_payload)
        );
    }

    #[test]
    fn reads_jpeg_dimensions_from_sof() {
        let jpeg = [
            0xff, 0xd8, 0xff, 0xe0, 0x00, 0x02, 0xff, 0xc0, 0x00, 0x08, 0x08, 0x10, 0x00, 0x20,
            0x00, 0xff, SOS, 0x00, 0x02,
        ];

        let dimensions = jpeg_dimensions(&jpeg).unwrap();

        assert_eq!(dimensions.width, 8192);
        assert_eq!(dimensions.height, 4096);
    }
}
