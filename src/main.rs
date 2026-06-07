use std::env;
use std::path::{Path, PathBuf};

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {}", err);
        std::process::exit(1);
    }
}

fn run() -> osv2gpx::AppResult<()> {
    let mut paths = Vec::new();

    for arg in env::args().skip(1) {
        match arg.as_str() {
            "-h" | "--help" => {
                print_usage();
                return Ok(());
            }
            _ if arg.starts_with('-') => return Err(format!("unknown flag: {}", arg).into()),
            _ => paths.push(PathBuf::from(arg)),
        }
    }

    if paths.len() == 2 {
        if let Some((gpx_path, mp4_path)) = split_gpx_mp4_args(&paths[0], &paths[1]) {
            return osv2gpx::set_mp4_creation_time_from_gpx(&gpx_path, &mp4_path);
        }
        if let Some((dir_path, gpx_path)) = split_dir_gpx_args(&paths[0], &paths[1]) {
            return osv2gpx::geotag_jpegs_with_gpx(&dir_path, &gpx_path);
        }
    }

    if paths.is_empty() {
        print_usage();
        std::process::exit(2);
    }

    for path in paths {
        if let Err(err) = osv2gpx::convert_osv_to_gpx(&path) {
            return Err(format!("{}: {}", path.display(), err).into());
        }
    }

    Ok(())
}

fn print_usage() {
    eprintln!("usage:");
    eprintln!("  osv2gpx flight.OSV");
    eprintln!("      Extract GPS from a DJI OSV file and write flight.gpx next to it.");
    eprintln!();
    eprintln!("  osv2gpx flight1.OSV flight2.OSV");
    eprintln!("      Convert each OSV file to a sibling GPX file.");
    eprintln!();
    eprintln!("  osv2gpx video.mp4 track.gpx");
    eprintln!("      Set the MP4 creation time to the first timestamp in the GPX.");
    eprintln!();
    eprintln!("  osv2gpx jpg-dir track.gpx");
    eprintln!("      Geotag one-JPG-per-second frames in filename order using GPX points.");
    eprintln!();
    eprintln!("ffmpeg one-JPG-per-second example:");
    eprintln!("  mkdir jpg-dir");
    eprintln!("  ffmpeg -i flight.mp4 -vf fps=1 -q:v 2 jpg-dir\\frame_%06d.jpg");
    eprintln!("  osv2gpx jpg-dir flight.gpx");
}

fn split_dir_gpx_args(a: &Path, b: &Path) -> Option<(PathBuf, PathBuf)> {
    let a_ext = lower_ext(a);
    let b_ext = lower_ext(b);
    match (a.is_dir(), a_ext.as_str(), b.is_dir(), b_ext.as_str()) {
        (true, _, false, "gpx") => Some((a.to_path_buf(), b.to_path_buf())),
        (false, "gpx", true, _) => Some((b.to_path_buf(), a.to_path_buf())),
        _ => None,
    }
}

fn split_gpx_mp4_args(a: &Path, b: &Path) -> Option<(PathBuf, PathBuf)> {
    let a_ext = lower_ext(a);
    let b_ext = lower_ext(b);
    match (a_ext.as_str(), b_ext.as_str()) {
        ("gpx", "mp4") => Some((a.to_path_buf(), b.to_path_buf())),
        ("mp4", "gpx") => Some((b.to_path_buf(), a.to_path_buf())),
        _ => None,
    }
}

fn lower_ext(path: &Path) -> String {
    path.extension()
        .map(|ext| ext.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}
