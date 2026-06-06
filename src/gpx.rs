use crate::gps::GpsPoint;
use chrono::{DateTime, Utc};
use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};
use quick_xml::{Reader, Writer};
use std::error::Error;
use std::fs::File;
use std::io::{BufReader, Write};
use std::path::Path;

pub type GpxResult<T> = Result<T, Box<dyn Error>>;

pub fn write_gpx(writer: &mut impl Write, points: &[GpsPoint], name: &str) -> GpxResult<()> {
    let mut xml = Writer::new_with_indent(writer, b' ', 2);
    xml.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))?;

    let mut gpx = BytesStart::new("gpx");
    gpx.push_attribute(("version", "1.1"));
    gpx.push_attribute(("creator", "osv2gpx"));
    gpx.push_attribute(("xmlns", "http://www.topografix.com/GPX/1/1"));
    gpx.push_attribute(("xmlns:xsi", "http://www.w3.org/2001/XMLSchema-instance"));
    gpx.push_attribute((
        "xsi:schemaLocation",
        "http://www.topografix.com/GPX/1/1 http://www.topografix.com/GPX/1/1/gpx.xsd",
    ));
    xml.write_event(Event::Start(gpx))?;

    xml.write_event(Event::Start(BytesStart::new("trk")))?;
    write_text_element(&mut xml, "name", name)?;
    xml.write_event(Event::Start(BytesStart::new("trkseg")))?;

    for point in points {
        let lat = format!("{:.8}", point.lat);
        let lon = format!("{:.8}", point.lon);
        let mut track_point = BytesStart::new("trkpt");
        track_point.push_attribute(("lat", lat.as_str()));
        track_point.push_attribute(("lon", lon.as_str()));
        xml.write_event(Event::Start(track_point))?;

        write_text_element(&mut xml, "ele", &format!("{:.3}", point.abs_alt))?;
        write_text_element(
            &mut xml,
            "time",
            &point.time.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
        )?;
        xml.write_event(Event::End(BytesEnd::new("trkpt")))?;
    }

    xml.write_event(Event::End(BytesEnd::new("trkseg")))?;
    xml.write_event(Event::End(BytesEnd::new("trk")))?;
    xml.write_event(Event::End(BytesEnd::new("gpx")))?;
    xml.get_mut().write_all(b"\n")?;
    Ok(())
}

fn write_text_element<W: Write>(xml: &mut Writer<W>, name: &str, text: &str) -> GpxResult<()> {
    xml.write_event(Event::Start(BytesStart::new(name)))?;
    xml.write_event(Event::Text(BytesText::new(text)))?;
    xml.write_event(Event::End(BytesEnd::new(name)))?;
    Ok(())
}

pub fn first_gpx_time(path: &Path) -> GpxResult<DateTime<Utc>> {
    let file = File::open(path)?;
    let mut reader = Reader::from_reader(BufReader::new(file));
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Eof => break,
            Event::Start(start) if is_local_name(start.name().as_ref(), b"time") => {
                let value = read_element_text(&mut reader, b"time")?;
                let parsed = DateTime::parse_from_rfc3339(value.trim())
                    .map_err(|err| format!("invalid GPX time {:?}: {}", value, err))?;
                return Ok(parsed.with_timezone(&Utc));
            }
            _ => {}
        }
        buf.clear();
    }

    Err("no GPX time element found".into())
}

pub fn read_gpx_points(path: &Path) -> GpxResult<Vec<GpsPoint>> {
    let file = File::open(path)?;
    let mut reader = Reader::from_reader(BufReader::new(file));
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut points = Vec::new();

    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Eof => break,
            Event::Start(start) if is_local_name(start.name().as_ref(), b"trkpt") => {
                let mut lat = None;
                let mut lon = None;
                for attr in start.attributes() {
                    let attr = attr?;
                    let value = attr.decode_and_unescape_value(reader.decoder())?;
                    match attr.key.as_ref() {
                        b"lat" => lat = Some(value.parse::<f64>()?),
                        b"lon" => lon = Some(value.parse::<f64>()?),
                        _ => {}
                    }
                }
                let (abs_alt, time) = read_track_point_body(&mut reader)?;
                if let (Some(lat), Some(lon), Some(time)) = (lat, lon, time) {
                    points.push(GpsPoint {
                        lat,
                        lon,
                        abs_alt: abs_alt.unwrap_or(0.0),
                        rel_alt: 0.0,
                        time,
                    });
                }
            }
            _ => {}
        }
        buf.clear();
    }

    points.sort_by_key(|point| point.time);
    Ok(points)
}

pub fn interpolate_gps_point(points: &[GpsPoint], time: DateTime<Utc>) -> Option<GpsPoint> {
    if points.is_empty() {
        return None;
    }
    if time < points.first()?.time || time > points.last()?.time {
        return None;
    }

    match points.binary_search_by_key(&time, |point| point.time) {
        Ok(idx) => return Some(points[idx].clone()),
        Err(idx) if idx == 0 || idx >= points.len() => return None,
        Err(idx) => {
            let before = &points[idx - 1];
            let after = &points[idx];
            let total = after.time.signed_duration_since(before.time);
            let elapsed = time.signed_duration_since(before.time);
            let total_ns = total.num_nanoseconds()? as f64;
            if total_ns == 0.0 {
                return Some(before.clone());
            }
            let ratio = elapsed.num_nanoseconds()? as f64 / total_ns;
            Some(GpsPoint {
                lat: lerp(before.lat, after.lat, ratio),
                lon: lerp(before.lon, after.lon, ratio),
                abs_alt: lerp(before.abs_alt, after.abs_alt, ratio),
                rel_alt: lerp(before.rel_alt, after.rel_alt, ratio),
                time,
            })
        }
    }
}

fn read_track_point_body<R: std::io::BufRead>(
    reader: &mut Reader<R>,
) -> GpxResult<(Option<f64>, Option<DateTime<Utc>>)> {
    let mut buf = Vec::new();
    let mut ele = None;
    let mut time = None;
    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Start(start) if is_local_name(start.name().as_ref(), b"ele") => {
                let value = read_element_text(reader, b"ele")?;
                ele = Some(value.trim().parse::<f64>()?);
            }
            Event::Start(start) if is_local_name(start.name().as_ref(), b"time") => {
                let value = read_element_text(reader, b"time")?;
                let parsed = DateTime::parse_from_rfc3339(value.trim())
                    .map_err(|err| format!("invalid GPX time {:?}: {}", value, err))?;
                time = Some(parsed.with_timezone(&Utc));
            }
            Event::End(end) if is_local_name(end.name().as_ref(), b"trkpt") => {
                return Ok((ele, time));
            }
            Event::Eof => return Err("unexpected EOF while reading GPX track point".into()),
            _ => {}
        }
        buf.clear();
    }
}

fn lerp(a: f64, b: f64, ratio: f64) -> f64 {
    a + (b - a) * ratio
}

fn read_element_text<R: std::io::BufRead>(
    reader: &mut Reader<R>,
    end_name: &[u8],
) -> GpxResult<String> {
    let mut buf = Vec::new();
    let mut text = String::new();
    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Text(event) => text.push_str(&event.decode()?),
            Event::CData(event) => text.push_str(&event.decode()?),
            Event::End(end) if is_local_name(end.name().as_ref(), end_name) => return Ok(text),
            Event::Eof => return Err("unexpected EOF while reading GPX time".into()),
            _ => {}
        }
        buf.clear();
    }
}

fn is_local_name(name: &[u8], local: &[u8]) -> bool {
    name == local || name.rsplit(|byte| *byte == b':').next() == Some(local)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn interpolates_between_gpx_points() {
        let t0 = Utc.with_ymd_and_hms(2026, 5, 27, 9, 23, 16).unwrap();
        let t1 = Utc.with_ymd_and_hms(2026, 5, 27, 9, 23, 18).unwrap();
        let points = vec![
            GpsPoint {
                lat: 24.0,
                lon: 121.0,
                abs_alt: 100.0,
                rel_alt: 0.0,
                time: t0,
            },
            GpsPoint {
                lat: 26.0,
                lon: 123.0,
                abs_alt: 200.0,
                rel_alt: 0.0,
                time: t1,
            },
        ];

        let point = interpolate_gps_point(&points, t0 + chrono::Duration::seconds(1)).unwrap();
        assert_eq!(point.lat, 25.0);
        assert_eq!(point.lon, 122.0);
        assert_eq!(point.abs_alt, 150.0);
    }
}
