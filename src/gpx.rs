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
