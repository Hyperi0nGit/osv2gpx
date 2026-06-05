use std::env;
use std::path::{Path, PathBuf};

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {}", err);
        std::process::exit(1);
    }
}

fn run() -> osv2gpx::AppResult<()> {
    let mut track_id = 0u32;
    let mut paths = Vec::new();
    let mut args = env::args().skip(1);

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print_usage();
                return Ok(());
            }
            "-track" => {
                let value = args.next().ok_or("-track requires a value")?;
                track_id = value
                    .parse::<u32>()
                    .map_err(|err| format!("invalid -track value {:?}: {}", value, err))?;
            }
            _ if arg.starts_with("-track=") => {
                let value = arg.trim_start_matches("-track=");
                track_id = value
                    .parse::<u32>()
                    .map_err(|err| format!("invalid -track value {:?}: {}", value, err))?;
            }
            _ if arg.starts_with('-') => return Err(format!("unknown flag: {}", arg).into()),
            _ => paths.push(PathBuf::from(arg)),
        }
    }

    if paths.len() == 2 {
        if let Some((gpx_path, mp4_path)) = split_gpx_mp4_args(&paths[0], &paths[1]) {
            return osv2gpx::set_mp4_creation_time_from_gpx(&gpx_path, &mp4_path);
        }
    }

    if paths.is_empty() {
        print_usage();
        std::process::exit(2);
    }

    for path in paths {
        if let Err(err) = osv2gpx::convert_osv_to_gpx(&path, track_id) {
            return Err(format!("{}: {}", path.display(), err).into());
        }
    }

    Ok(())
}

fn print_usage() {
    eprintln!("usage:");
    eprintln!("  osv2gpx [flags] flight.OSV               write flight.gpx next to the OSV");
    eprintln!("  osv2gpx [flags] flight1.OSV flight2.OSV  write one .gpx file for each OSV");
    eprintln!(
        "  osv2gpx video.mp4 track.gpx              set MP4 creation time from first GPX time"
    );
    eprintln!();
    eprintln!("options:");
    eprintln!("  -track uint");
    eprintln!("        metadata track id to use; defaults to first djmd track");
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
