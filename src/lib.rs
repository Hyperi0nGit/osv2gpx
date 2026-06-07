pub mod gps;
pub mod gpx;
pub mod jpeg;
pub mod mp4;

use chrono::Duration;
use std::error::Error;
use std::fs::{self, File};
use std::path::{Path, PathBuf};

pub type AppResult<T> = Result<T, Box<dyn Error>>;

pub fn convert_osv_to_gpx(path: &Path) -> AppResult<()> {
    let mut file = File::open(path)?;
    let tracks = mp4::parse_tracks(&mut file)?;
    let creation_time = mp4::parse_movie_creation_time(&mut file)?;
    let track = mp4::select_track(&tracks, 0)?.clone();
    let refs = mp4::samples_for_track(&track)?;

    let mut points = Vec::with_capacity(refs.len());
    for sample in refs {
        let payload = mp4::read_at(&mut file, sample.offset, sample.size as usize)?;
        if let Some(mut point) = gps::extract_gps_point(&payload) {
            let nanos = (sample.time * 1_000_000_000.0) as i64;
            point.time = creation_time + Duration::nanoseconds(nanos);
            points.push(point);
        }
    }

    if points.is_empty() {
        return Err("no GPS points found in OSV protobuf metadata".into());
    }

    let output_path = output_gpx_path(path);
    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut out = File::create(&output_path)?;
    gpx::write_gpx(&mut out, &points, &name)?;
    eprintln!(
        "wrote {} GPX points to {}",
        points.len(),
        output_path.display()
    );
    Ok(())
}

pub fn set_mp4_creation_time_from_gpx(gpx_path: &Path, mp4_path: &Path) -> AppResult<()> {
    let time = gpx::first_gpx_time(gpx_path)?;
    let mut file = File::options().read(true).write(true).open(mp4_path)?;
    let updated = mp4::write_mp4_creation_time(&mut file, time)?;
    eprintln!(
        "set {} creation_time to {} from {} ({} fields updated)",
        mp4_path.display(),
        time.format("%Y-%m-%dT%H:%M:%SZ"),
        gpx_path.display(),
        updated
    );
    Ok(())
}

pub fn geotag_jpegs_with_gpx(dir_path: &Path, gpx_path: &Path) -> AppResult<()> {
    let points = gpx::read_gpx_points(gpx_path)?;
    if points.is_empty() {
        return Err("no GPX track points found".into());
    }

    let mut jpg_paths = Vec::new();
    for entry in fs::read_dir(dir_path)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() && is_jpeg_path(&path) {
            jpg_paths.push(path);
        }
    }
    jpg_paths.sort_by_key(|path| path.file_name().map(|name| name.to_os_string()));

    if jpg_paths.is_empty() {
        return Err(format!("no JPG files found in {}", dir_path.display()).into());
    }

    let start = points[0].time;
    for (idx, jpg_path) in jpg_paths.iter().enumerate() {
        let image_time = start + Duration::seconds(idx as i64);
        let point = gpx::interpolate_gps_point(&points, image_time).ok_or_else(|| {
            format!(
                "{} time {} is outside GPX range",
                jpg_path.display(),
                image_time.format("%Y-%m-%dT%H:%M:%SZ")
            )
        })?;
        jpeg::write_gps_exif(jpg_path, &point)?;
    }

    eprintln!(
        "wrote GPS EXIF and GPano XMP to {} JPG files in {} from {}",
        jpg_paths.len(),
        dir_path.display(),
        gpx_path.display()
    );
    Ok(())
}

fn output_gpx_path(path: &Path) -> PathBuf {
    let mut out = path.to_path_buf();
    out.set_extension("gpx");
    out
}

fn is_jpeg_path(path: &Path) -> bool {
    matches!(
        path.extension()
            .map(|ext| ext.to_string_lossy().to_lowercase()),
        Some(ext) if ext == "jpg" || ext == "jpeg"
    )
}
