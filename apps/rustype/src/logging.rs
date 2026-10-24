//! Write runtime messages to both stderr and a log file, so a Finder-launched
//! app (whose stderr is discarded) can still be diagnosed.

use std::path::PathBuf;

pub fn log_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join("Library/Logs/Rustype/rustype.log"))
}

macro_rules! log_line {
    ($($arg:tt)*) => {{
        let line = format!($($arg)*);
        eprintln!("{line}");
        if let Some(path) = $crate::logging::log_path() {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
            {
                use std::io::Write as _;
                let _ = writeln!(file, "{line}");
            }
        }
    }};
}
