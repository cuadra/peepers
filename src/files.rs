//! Building the file list from CLI arguments.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};

pub const EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "jpe", "jfif", "png", "apng", "gif", "webp", "bmp", "tif", "tiff", "ico", "pnm",
    "pbm", "pgm", "ppm", "pam", "qoi", "tga", "hdr", "exr",
];

pub fn is_image(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .map(|e| EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// Turn arguments into `(list, start index)`.
///
/// * one file  → every image in its directory, starting at that file
///   (this is what Yazi / file managers pass)
/// * a directory → every image in it
/// * several paths → exactly those (plus contents of any directories)
pub fn collect(args: &[PathBuf]) -> (Vec<PathBuf>, usize) {
    let args: Vec<PathBuf> = if args.is_empty() { vec![PathBuf::from(".")] } else { args.to_vec() };

    if args.len() == 1 {
        let a = absolute(&args[0]);
        if a.is_dir() {
            return (list_dir(&a), 0);
        }
        let dir = a.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
        let mut list = list_dir(&dir);
        let idx = match list.iter().position(|p| *p == a) {
            Some(i) => i,
            None => {
                // Unknown extension or odd path: still show it.
                list.insert(0, a);
                0
            }
        };
        return (list, idx);
    }

    let mut out = Vec::new();
    for a in &args {
        let a = absolute(a);
        if a.is_dir() {
            out.extend(list_dir(&a));
        } else if a.exists() {
            out.push(a);
        }
    }
    (out, 0)
}

fn absolute(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

fn list_dir(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| is_image(p) && p.is_file())
                .collect()
        })
        .unwrap_or_default();
    v.sort_by(|a, b| natural_cmp(&a.to_string_lossy(), &b.to_string_lossy()));
    v
}

/// "img2" < "img10", case-insensitive.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, _) => return Ordering::Less,
            (_, None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let take = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut s = String::new();
                    while let Some(c) = it.peek().copied().filter(char::is_ascii_digit) {
                        s.push(c);
                        it.next();
                    }
                    s
                };
                let (na, nb) = (take(&mut a), take(&mut b));
                let (ta, tb) = (na.trim_start_matches('0'), nb.trim_start_matches('0'));
                let o = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb)).then_with(|| na.len().cmp(&nb.len()));
                if o != Ordering::Equal {
                    return o;
                }
            }
            (Some(x), Some(y)) => {
                let o = x.to_lowercase().cmp(y.to_lowercase());
                if o != Ordering::Equal {
                    return o;
                }
                a.next();
                b.next();
            }
        }
    }
}

/// `~/Pictures/2026/October` style directory label for the info panel.
pub fn pretty_dir(p: &Path) -> String {
    let dir = p.parent().unwrap_or(p);
    if let Some(home) = dirs::home_dir() {
        if let Ok(rest) = dir.strip_prefix(&home) {
            return if rest.as_os_str().is_empty() { "~".into() } else { format!("~/{}", rest.display()) };
        }
    }
    dir.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn natural() {
        let mut v = vec!["img10.jpg", "IMG2.jpg", "img1.jpg", "img02.jpg", "a.png"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, ["a.png", "img1.jpg", "IMG2.jpg", "img02.jpg", "img10.jpg"]);
    }
}
