//! Golden frames: the frame set of `tools/golden.sh`, rendered in-process
//! through the library and compared cell by cell with the reference frames
//! committed as `tests/golden/<name>.frame`.
//!
//! A frame file is width and height as u32 little-endian, then one record
//! per cell, row-major: codepoint u32 little-endian, foreground r g b,
//! background r g b. Eleven 120x40 frames are about half a megabyte.
//!
//! The comparison is tolerant by default: a frame passes when at least
//! `GOLDEN_MIN_IDENTICAL` percent of cells are identical (default 98) and
//! the mean colour distance over all cells is at most `GOLDEN_MAX_DISTANCE`
//! (default 2.0; a cell's distance is the sum of absolute differences over
//! its six colour channels). `GOLDEN_STRICT=1` requires every cell to be
//! identical. Every run prints each frame's numbers, and a failure lists
//! the first ten differing cells so a shifted sprite can be told from a
//! colour tweak.
//!
//! `GOLDEN_RECORD=1` rewrites the reference frames (`make golden-record`).
//! `GOLDEN_DUMP=<dir>` also writes each rendered frame as a `.cells` text
//! dump, the format `tools/golden.sh` produces, for diffing by hand.

use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;

use roguemap::assets::Assets;
use roguemap::canvas::{Canvas, Cell, Rgb};
use roguemap::snapshot;

/// The snapshot geometry `tools/golden.sh` uses; a test checks the script
/// still says so.
const WIDTH: u16 = 120;
const HEIGHT: u16 = 40;

const DEFAULT_MIN_IDENTICAL: f64 = 98.0;
const DEFAULT_MAX_DISTANCE: f64 = 2.0;
/// How many differing cells a failure message lists per frame.
const LISTED_DIFFS: usize = 10;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn frame_dir() -> PathBuf {
    root().join("tests/golden")
}

fn frame_path(name: &str) -> PathBuf {
    frame_dir().join(format!("{name}.frame"))
}

/// One frame of the script: its name and `key=value` arguments.
struct Shot {
    name: String,
    args: Vec<String>,
}

/// The `shot NAME args...` lines of `tools/golden.sh`, in order.
fn shots() -> Vec<Shot> {
    let script = fs::read_to_string(root().join("tools/golden.sh")).expect("tools/golden.sh is readable");
    assert!(script.contains(&format!("--snap {WIDTH} {HEIGHT} ")), "tools/golden.sh no longer renders {WIDTH}x{HEIGHT} frames; update WIDTH and HEIGHT in tests/golden.rs");
    let shots: Vec<Shot> = script
        .lines()
        .filter_map(|l| l.strip_prefix("shot "))
        .map(|l| {
            let mut words = l.split_whitespace().map(str::to_string);
            let name = words.next().expect("a shot line names its frame");
            Shot { name, args: words.collect() }
        })
        .collect();
    assert!(!shots.is_empty(), "tools/golden.sh lists no `shot` lines");
    shots
}

// Frame files.

fn encode(cv: &Canvas) -> Vec<u8> {
    let mut v = Vec::with_capacity(8 + cv.cells.len() * 10);
    v.extend_from_slice(&(cv.w as u32).to_le_bytes());
    v.extend_from_slice(&(cv.h as u32).to_le_bytes());
    for c in &cv.cells {
        v.extend_from_slice(&(c.ch as u32).to_le_bytes());
        v.extend_from_slice(&[c.fg.0, c.fg.1, c.fg.2, c.bg.0, c.bg.1, c.bg.2]);
    }
    v
}

fn decode(bytes: &[u8]) -> Result<Canvas, String> {
    let u32_at = |i: usize| -> Result<u32, String> {
        let b = bytes.get(i..i + 4).ok_or_else(|| format!("truncated at byte {i}"))?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    let (w, h) = (u32_at(0)?, u32_at(4)?);
    if w > u16::MAX as u32 || h > u16::MAX as u32 {
        return Err(format!("implausible size {w}x{h}"));
    }
    let n = (w * h) as usize;
    if bytes.len() != 8 + n * 10 {
        return Err(format!("{} bytes for a {w}x{h} frame; expected {}", bytes.len(), 8 + n * 10));
    }
    let mut cv = Canvas::new(w as u16, h as u16);
    for (i, cell) in cv.cells.iter_mut().enumerate() {
        let at = 8 + i * 10;
        let cp = u32_at(at)?;
        let ch = char::from_u32(cp).ok_or_else(|| format!("cell {i}: U+{cp:X} is not a character"))?;
        let b = &bytes[at + 4..at + 10];
        *cell = Cell { ch, fg: Rgb(b[0], b[1], b[2]), bg: Rgb(b[3], b[4], b[5]) };
    }
    Ok(cv)
}

// The comparator.

/// One cell that differs between the reference and the rendered frame.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Diff {
    x: i32,
    y: i32,
    expected: Cell,
    actual: Cell,
}

/// How close a rendered frame is to its reference.
#[derive(Clone, Debug, PartialEq)]
struct Report {
    /// Percent of cells with the same glyph and both colours.
    identical: f64,
    /// Percent of cells with the same glyph.
    glyph: f64,
    /// Mean over all cells of the six-channel colour distance.
    distance: f64,
    /// Every differing cell, row-major.
    diffs: Vec<Diff>,
}

fn channel_distance(a: Rgb, b: Rgb) -> u32 {
    a.0.abs_diff(b.0) as u32 + a.1.abs_diff(b.1) as u32 + a.2.abs_diff(b.2) as u32
}

/// Compare two frames of the same size cell by cell.
fn compare(expected: &Canvas, actual: &Canvas) -> Result<Report, String> {
    if (expected.w, expected.h) != (actual.w, actual.h) {
        return Err(format!("reference is {}x{} but the frame is {}x{}", expected.w, expected.h, actual.w, actual.h));
    }
    let n = expected.cells.len();
    let (mut identical, mut glyph, mut distance) = (0usize, 0usize, 0u64);
    let mut diffs = Vec::new();
    for (i, (e, a)) in expected.cells.iter().zip(actual.cells.iter()).enumerate() {
        if e == a {
            identical += 1;
            glyph += 1;
            continue;
        }
        if e.ch == a.ch {
            glyph += 1;
        }
        distance += (channel_distance(e.fg, a.fg) + channel_distance(e.bg, a.bg)) as u64;
        diffs.push(Diff { x: (i % expected.w as usize) as i32, y: (i / expected.w as usize) as i32, expected: *e, actual: *a });
    }
    let pct = |k: usize| if n == 0 { 100.0 } else { k as f64 * 100.0 / n as f64 };
    Ok(Report { identical: pct(identical), glyph: pct(glyph), distance: if n == 0 { 0.0 } else { distance as f64 / n as f64 }, diffs })
}

fn show_cell(c: &Cell) -> String {
    let glyph = if c.ch == ' ' { "' '".to_string() } else { format!("'{}'", c.ch) };
    format!("U+{:04X} {glyph} fg({},{},{}) bg({},{},{})", c.ch as u32, c.fg.0, c.fg.1, c.fg.2, c.bg.0, c.bg.1, c.bg.2)
}

impl Report {
    fn summary(&self) -> String {
        format!("identical {:.3}%  glyph {:.3}%  distance {:.4}  ({} cells differ)", self.identical, self.glyph, self.distance, self.diffs.len())
    }

    /// The first differing cells, one per line.
    fn listing(&self) -> String {
        let mut s = String::new();
        for d in self.diffs.iter().take(LISTED_DIFFS) {
            let _ = writeln!(s, "      ({}, {}): expected {}  actual {}", d.x, d.y, show_cell(&d.expected), show_cell(&d.actual));
        }
        if self.diffs.len() > LISTED_DIFFS {
            let _ = writeln!(s, "      ... and {} more", self.diffs.len() - LISTED_DIFFS);
        }
        s
    }
}

/// Pass thresholds from the environment.
struct Tolerance {
    min_identical: f64,
    max_distance: f64,
    strict: bool,
}

impl Tolerance {
    fn from_env() -> Tolerance {
        let num = |key: &str, default: f64| std::env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(default);
        let strict = std::env::var("GOLDEN_STRICT").is_ok_and(|v| v != "0" && !v.is_empty());
        Tolerance { min_identical: num("GOLDEN_MIN_IDENTICAL", DEFAULT_MIN_IDENTICAL), max_distance: num("GOLDEN_MAX_DISTANCE", DEFAULT_MAX_DISTANCE), strict }
    }

    fn passes(&self, r: &Report) -> bool {
        if self.strict {
            r.diffs.is_empty()
        } else {
            r.identical >= self.min_identical && r.distance <= self.max_distance
        }
    }

    fn describe(&self) -> String {
        if self.strict {
            "strict: every cell identical".to_string()
        } else {
            format!("identical >= {:.1}% and distance <= {:.2}", self.min_identical, self.max_distance)
        }
    }
}

// The tests.

#[test]
fn golden_frames_match_their_references() {
    let record = std::env::var_os("GOLDEN_RECORD").is_some();
    let dump = std::env::var_os("GOLDEN_DUMP").map(PathBuf::from);
    if let Some(d) = &dump {
        fs::create_dir_all(d).expect("GOLDEN_DUMP directory");
    }
    let tol = Tolerance::from_env();
    let assets = Rc::new(Assets::load().expect("assets load"));
    let mut failures = String::new();
    let mut all_identical = true;
    let shots = shots();
    for s in &shots {
        let cv = snapshot::render(assets.clone(), WIDTH, HEIGHT, &s.args);
        if let Some(d) = &dump {
            fs::write(d.join(format!("{}.cells", s.name)), cv.dump_bytes()).expect("write frame dump");
        }
        let path = frame_path(&s.name);
        if record {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, encode(&cv)).expect("write reference frame");
            eprintln!("golden: recorded {:<10} {}x{}", s.name, cv.w, cv.h);
            continue;
        }
        let reference = match fs::read(&path) {
            Ok(bytes) => decode(&bytes).unwrap_or_else(|e| panic!("{}: {e}", path.display())),
            Err(_) => {
                eprintln!("golden: {:<10} no reference at {}", s.name, path.display());
                let _ = writeln!(failures, "  {}: no reference frame at {}", s.name, path.display());
                all_identical = false;
                continue;
            }
        };
        match compare(&reference, &cv) {
            Ok(r) => {
                let ok = tol.passes(&r);
                eprintln!("golden: {:<10} {}{}", s.name, r.summary(), if ok { "" } else { "  FAIL" });
                if !r.diffs.is_empty() {
                    all_identical = false;
                }
                if !ok {
                    let _ = write!(failures, "  {}: {}\n{}", s.name, r.summary(), r.listing());
                }
            }
            Err(e) => {
                eprintln!("golden: {:<10} {e}  FAIL", s.name);
                let _ = writeln!(failures, "  {}: {e}", s.name);
                all_identical = false;
            }
        }
    }
    if record {
        eprintln!("golden: recorded {} frames in {}", shots.len(), frame_dir().display());
        return;
    }
    assert!(failures.is_empty(), "golden frames differ from tests/golden ({}):\n{}If the change is intended, run `make golden-record` and say which frames changed and why in the commit.", tol.describe(), failures);
    if all_identical {
        eprintln!("golden: all frames identical");
    } else {
        eprintln!("golden: all frames within tolerance ({})", tol.describe());
    }
}

#[test]
fn every_reference_frame_has_a_shot() {
    let names: Vec<String> = shots().into_iter().map(|s| s.name).collect();
    for entry in fs::read_dir(frame_dir()).expect("tests/golden exists") {
        let p = entry.unwrap().path();
        if p.extension().is_some_and(|e| e == "frame") {
            let stem = p.file_stem().unwrap().to_string_lossy().to_string();
            assert!(names.contains(&stem), "{} has no shot in tools/golden.sh", p.display());
        }
    }
}

#[test]
fn frame_files_round_trip() {
    let mut cv = Canvas::new(7, 3);
    for (i, c) in cv.cells.iter_mut().enumerate() {
        *c = Cell { ch: char::from_u32(0x1FB00 + i as u32).unwrap(), fg: Rgb(i as u8, 200, 3), bg: Rgb(9, i as u8 * 10, 250) };
    }
    let bytes = encode(&cv);
    assert_eq!(bytes.len(), 8 + 21 * 10);
    let back = decode(&bytes).unwrap();
    assert_eq!((back.w, back.h), (7, 3));
    assert_eq!(back.cells, cv.cells);
    assert!(decode(&bytes[..bytes.len() - 1]).is_err(), "a truncated file is rejected");
}

/// A blank 120x40 frame with a three-cell figure at (`x0`, 20).
fn figure_at(x0: i32) -> Canvas {
    let mut cv = Canvas::new(WIDTH, HEIGHT);
    for c in cv.cells.iter_mut() {
        *c = Cell { ch: ' ', fg: Rgb(0, 0, 0), bg: Rgb(40, 60, 30) };
    }
    for (i, ch) in ['(', '@', ')'].into_iter().enumerate() {
        cv.put(x0 + i as i32, 20, ch, Rgb(240, 214, 176), Rgb(52, 74, 150));
    }
    cv
}

#[test]
fn comparator_scores_identical_frames_perfectly() {
    let a = figure_at(50);
    let r = compare(&a, &a).unwrap();
    assert_eq!(r, Report { identical: 100.0, glyph: 100.0, distance: 0.0, diffs: vec![] });
    assert!(Tolerance { min_identical: 100.0, max_distance: 0.0, strict: true }.passes(&r));
    let wrong_size = Canvas::new(WIDTH, HEIGHT - 1);
    assert!(compare(&a, &wrong_size).is_err());
}

#[test]
fn comparator_scores_one_changed_cell_in_4800_as_expected() {
    let a = figure_at(50);
    let mut b = figure_at(50);
    // Only the background of one cell moves, by 12 on one channel.
    b.put(3, 7, ' ', Rgb(0, 0, 0), Rgb(52, 60, 30));
    let r = compare(&a, &b).unwrap();
    let cells = (WIDTH as f64) * (HEIGHT as f64);
    assert!((r.identical - 4799.0 * 100.0 / cells).abs() < 1e-9, "{}", r.identical);
    assert_eq!(r.glyph, 100.0, "the glyph did not change");
    assert!((r.distance - 12.0 / cells).abs() < 1e-12, "{}", r.distance);
    assert_eq!(r.diffs.len(), 1);
    assert_eq!((r.diffs[0].x, r.diffs[0].y), (3, 7));
    let default = Tolerance { min_identical: DEFAULT_MIN_IDENTICAL, max_distance: DEFAULT_MAX_DISTANCE, strict: false };
    assert!(default.passes(&r), "one colour tweak is within the default tolerance");
    assert!(!Tolerance { strict: true, ..default }.passes(&r), "but not in strict mode");
    // A glyph change in the same cell also counts against the glyph score.
    b.put(3, 7, '#', Rgb(0, 0, 0), Rgb(40, 60, 30));
    let r = compare(&a, &b).unwrap();
    assert!((r.glyph - 4799.0 * 100.0 / cells).abs() < 1e-9);
    assert_eq!(r.distance, 0.0, "colours are back to the reference");
}

#[test]
fn comparator_reports_a_shifted_sprite_by_its_differing_cells() {
    let a = figure_at(50);
    let b = figure_at(51);
    let r = compare(&a, &b).unwrap();
    // Three cells of the figure and the one it vacated.
    let where_: Vec<(i32, i32)> = r.diffs.iter().map(|d| (d.x, d.y)).collect();
    assert_eq!(where_, vec![(50, 20), (51, 20), (52, 20), (53, 20)]);
    assert_eq!((r.diffs[0].expected.ch, r.diffs[0].actual.ch), ('(', ' '), "the vacated cell");
    assert_eq!((r.diffs[1].expected.ch, r.diffs[1].actual.ch), ('@', '('), "the figure moved right");
    assert_eq!((r.diffs[3].expected.ch, r.diffs[3].actual.ch), (' ', ')'));
    let listing = r.listing();
    assert!(listing.contains("(50, 20): expected U+0028 '(' fg(240,214,176) bg(52,74,150)  actual U+0020 ' ' fg(0,0,0) bg(40,60,30)"), "{listing}");
    assert_eq!(listing.lines().count(), 4, "every differing cell is listed when there are ten or fewer");
    assert!(r.summary().starts_with("identical 99.917%  glyph 99.917%"), "{}", r.summary());
}
