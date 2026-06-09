# osv2gpx

`osv2gpx` is a tool for OSV files recorded by DJI Avata 360. It prepares the
GPX tracks, MP4 timing metadata, and JPG GPS metadata needed for Google Street
View uploads.

It is used to:

1. Extract GPS data from an OSV file and save it as a GPX file.
2. Add timestamp metadata to a DJI Studio exported MP4 using the GPX file.
3. Add GPS metadata to JPG files generated from the MP4 with ffmpeg at one
   frame per second, using the GPX file.

[繁體中文](README.zh-TW.md)

## Usage

### 1. Extract GPX from OSV

Generate `flight.gpx` from the original `flight.OSV`:

```powershell
osv2gpx flight.OSV
```

On Windows, you can also drag an OSV file onto `osv2gpx.exe`. The GPX file is
created in the same folder as the OSV file.

Generate one GPX file per OSV input:

```powershell
osv2gpx flight1.OSV flight2.OSV flight3.OSV
```

### 2. Add Time Metadata to DJI Studio MP4

Write the GPX first timestamp into the DJI Studio exported MP4:

```powershell
osv2gpx flight.mp4 flight.gpx
```

### 3. Add GPS Metadata to JPG Files

Generate one JPG per second from the exported MP4 and write GPS EXIF plus GPano
XMP to those JPG files:

```powershell
mkdir jpg-dir
ffmpeg -i flight.mp4 -vf fps=1 -q:v 2 jpg-dir\frame_%06d.jpg
osv2gpx jpg-dir flight.gpx
```

`osv2gpx` processes the generated JPG files by filename order. The first JPG
uses the first GPX time, the second JPG uses one second after that, and so on.
GPS positions are interpolated from the GPX track. The GPano XMP marks each JPG
as a full equirectangular panorama using the image's actual width and height.
The generated JPG files can be uploaded with the Google Street View Publish API,
for example by using [stviewpub](https://znbang.github.io/stviewpub/)
([project](https://github.com/znbang/stviewpub)).

## Output

The GPX output contains one track segment with `trkpt` entries:

```xml
<trkpt lat="24.79920562" lon="121.05540174">
  <ele>201.138</ele>
  <time>2026-05-27T09:23:16.647Z</time>
</trkpt>
```

Elevation uses absolute altitude in meters.

## Build

Install the stable [Rust toolchain](https://rustup.rs/), then build the release
binary:

```powershell
cargo build --release --locked
```

## Notes

- Use the original OSV file to generate the GPX track. DJI Studio exported MP4
  files do not preserve the DJI GPS metadata tracks.
- DJI Studio exported MP4 files also lack creation time metadata. If you already
  have a matching GPX file, `osv2gpx` can copy the GPX first timestamp into the
  MP4 creation time fields.
- GPX files contain latitude, longitude, absolute altitude, and timestamp data.
  JPG positions are interpolated from the GPX track.
- MP4 creation time fields are second-precision, while DJI SRT files may include
  millisecond timestamps.
