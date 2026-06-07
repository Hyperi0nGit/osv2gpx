use std::env;
use std::io::{self, IsTerminal, Write};
#[cfg(windows)]
use std::mem;
#[cfg(windows)]
use std::os::raw::c_void;
use std::path::{Path, PathBuf};
#[cfg(windows)]
use std::process::Command;

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn CloseHandle(handle: *mut c_void) -> i32;
    fn CreateToolhelp32Snapshot(flags: u32, process_id: u32) -> *mut c_void;
    fn GetCurrentProcessId() -> u32;
    fn Process32FirstW(snapshot: *mut c_void, entry: *mut ProcessEntry32W) -> i32;
    fn Process32NextW(snapshot: *mut c_void, entry: *mut ProcessEntry32W) -> i32;
}

#[cfg(windows)]
const TH32CS_SNAPPROCESS: u32 = 0x00000002;
#[cfg(windows)]
const INVALID_HANDLE_VALUE: *mut c_void = !0_usize as *mut c_void;

#[cfg(windows)]
#[repr(C)]
struct ProcessEntry32W {
    size: u32,
    usage: u32,
    process_id: u32,
    default_heap_id: usize,
    module_id: u32,
    threads: u32,
    parent_process_id: u32,
    priority_class_base: i32,
    flags: u32,
    exe_file: [u16; 260],
}

fn main() {
    let exit_code = match run() {
        Ok(()) => 0,
        Err(AppError::Usage) => 2,
        Err(AppError::Other(err)) => {
            eprintln!("error: {}", err);
            1
        }
    };

    pause_if_needed();

    if exit_code != 0 {
        std::process::exit(exit_code);
    }
}

enum AppError {
    Usage,
    Other(Box<dyn std::error::Error>),
}

impl<E> From<E> for AppError
where
    E: Into<Box<dyn std::error::Error>>,
{
    fn from(err: E) -> Self {
        Self::Other(err.into())
    }
}

fn run() -> Result<(), AppError> {
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
            osv2gpx::set_mp4_creation_time_from_gpx(&gpx_path, &mp4_path)?;
            return Ok(());
        }
        if let Some((dir_path, gpx_path)) = split_dir_gpx_args(&paths[0], &paths[1]) {
            osv2gpx::geotag_jpegs_with_gpx(&dir_path, &gpx_path)?;
            return Ok(());
        }
    }

    if paths.is_empty() {
        print_usage();
        return Err(AppError::Usage);
    }

    for path in paths {
        if let Err(err) = osv2gpx::convert_osv_to_gpx(&path) {
            return Err(format!("{}: {}", path.display(), err).into());
        }
    }

    Ok(())
}

fn pause_if_needed() {
    if !should_pause_before_exit() {
        return;
    }

    eprintln!();
    eprint!("Press any key to exit...");
    let _ = io::stderr().flush();

    if pause_for_keypress() {
        return;
    }

    eprint!("Press Enter to exit...");
    let _ = io::stderr().flush();

    let mut input = String::new();
    let _ = io::stdin().read_line(&mut input);
}

#[cfg(windows)]
fn should_pause_before_exit() -> bool {
    let stdin_is_terminal = io::stdin().is_terminal();
    let parent_name = parent_process_name();
    let started_by_explorer = parent_name
        .as_deref()
        .map(|name| name.eq_ignore_ascii_case("explorer.exe"))
        .unwrap_or(false);

    if env::var_os("OSV2GPX_DEBUG_PAUSE").is_some() {
        eprintln!(
            "debug: stdin_is_terminal={stdin_is_terminal}, parent_process={}",
            parent_name.as_deref().unwrap_or("<unknown>")
        );
    }

    !stdin_is_terminal || started_by_explorer
}

#[cfg(not(windows))]
fn should_pause_before_exit() -> bool {
    !io::stdin().is_terminal()
}

#[cfg(windows)]
fn parent_process_name() -> Option<String> {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return None;
    }

    let current_process_id = unsafe { GetCurrentProcessId() };
    let parent_process_id = find_parent_process_id(snapshot, current_process_id);
    let parent_name =
        parent_process_id.and_then(|process_id| find_process_name(snapshot, process_id));

    unsafe {
        CloseHandle(snapshot);
    }

    parent_name
}

#[cfg(windows)]
fn find_parent_process_id(snapshot: *mut c_void, process_id: u32) -> Option<u32> {
    find_process_entry(snapshot, process_id).map(|entry| entry.parent_process_id)
}

#[cfg(windows)]
fn find_process_name(snapshot: *mut c_void, process_id: u32) -> Option<String> {
    find_process_entry(snapshot, process_id).map(|entry| exe_file_to_string(&entry.exe_file))
}

#[cfg(windows)]
fn find_process_entry(snapshot: *mut c_void, process_id: u32) -> Option<ProcessEntry32W> {
    let mut entry = ProcessEntry32W {
        size: mem::size_of::<ProcessEntry32W>() as u32,
        usage: 0,
        process_id: 0,
        default_heap_id: 0,
        module_id: 0,
        threads: 0,
        parent_process_id: 0,
        priority_class_base: 0,
        flags: 0,
        exe_file: [0; 260],
    };

    let mut has_entry = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
    while has_entry {
        if entry.process_id == process_id {
            return Some(entry);
        }
        has_entry = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
    }

    None
}

#[cfg(windows)]
fn exe_file_to_string(exe_file: &[u16; 260]) -> String {
    let len = exe_file
        .iter()
        .position(|code_unit| *code_unit == 0)
        .unwrap_or(exe_file.len());
    String::from_utf16_lossy(&exe_file[..len])
}

#[cfg(windows)]
fn pause_for_keypress() -> bool {
    Command::new("cmd")
        .args(["/C", "pause > nul"])
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(not(windows))]
fn pause_for_keypress() -> bool {
    false
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
