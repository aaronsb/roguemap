//! roguemap-edit: the asset editor binary (ADR-003). Argument parsing and
//! the terminal loop; the editor itself is `roguemap::editor`.
//!
//! `roguemap-edit DIR` edits the tables in DIR; `--export DIR` writes the
//! embedded set there first. With no argument `ROGUEMAP_ASSETS` names the
//! directory. An embedded set has nowhere to be written back to, so the
//! editor refuses to run on it. `--snap W H OUT [key=value...]` renders one
//! editor screen headless.

use std::io;
use std::path::Path;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};

use roguemap::assets::Assets;
use roguemap::editor::{self, keys, Editor};
use roguemap::terminal;

const USAGE: &str = "usage: roguemap-edit DIR                 edit the asset tables in DIR
       roguemap-edit --export DIR        write the embedded set to DIR, then edit it
       roguemap-edit --snap W H OUT [key=value...]
                                         render one editor screen headless; keys:
                                         dir table row biome season tod glyphs tier pattern deg pane
With no DIR, ROGUEMAP_ASSETS names the directory.";

fn fail(msg: &str, code: i32) -> ! {
    eprintln!("roguemap-edit: {msg}");
    std::process::exit(code);
}

/// The terminal loop: event, action, apply, draw, present.
fn edit(dir: &Path) -> io::Result<()> {
    // Load before touching the terminal so a bad file is reported on a
    // plain console.
    let mut ed = match Editor::open(dir, 80, 25) {
        Ok(ed) => ed,
        Err(e) => fail(&format!("cannot load {}: {e}", dir.display()), 1),
    };
    let mut term = terminal::Terminal::new()?;
    ed.resize(term.width(), term.height());
    let start = Instant::now();
    let frame = Duration::from_millis(40);
    loop {
        let now = Instant::now();
        ed.draw(term.canvas(), start.elapsed().as_secs_f32());
        term.present()?;
        while event::poll(frame.saturating_sub(now.elapsed()))? {
            match event::read()? {
                Event::Key(k) if k.kind == KeyEventKind::Press => {
                    if k.code == KeyCode::Char('c') && k.modifiers.contains(KeyModifiers::CONTROL) {
                        return Ok(());
                    }
                    if let Some(a) = keys::lookup(ed.mode, k.code) {
                        ed.apply(a);
                    }
                    if ed.quit {
                        return Ok(());
                    }
                }
                Event::Resize(w, h) => {
                    term.resize(w, h);
                    ed.resize(w as i32, h as i32);
                }
                _ => {}
            }
        }
    }
}

/// `--snap W H OUT [key=value...]`: the set from `dir=`, else
/// `ROGUEMAP_ASSETS`, else the embedded one (a snapshot writes nothing back).
fn snap(args: &[String]) -> io::Result<()> {
    if args.len() < 3 {
        fail(USAGE, 2);
    }
    let w: u16 = args[0].parse().unwrap_or(80);
    let h: u16 = args[1].parse().unwrap_or(25);
    let a = roguemap::snapshot::SnapArgs::parse(&args[3..]);
    let assets = match a.text("dir") {
        Some(d) => Assets::from_dir(Path::new(d)),
        None => Assets::load(),
    };
    let assets = assets.unwrap_or_else(|e| fail(&format!("cannot load assets: {e}"), 1));
    let cv = editor::snapshot(assets, w, h, &args[3..]).unwrap_or_else(|e| fail(&e, 2));
    cv.dump(&args[2])
}

fn main() -> io::Result<()> {
    let argv: Vec<String> = std::env::args().collect();
    match argv.get(1).map(|s| s.as_str()) {
        Some("--snap") => snap(&argv[2..]),
        Some("--export") => {
            let Some(dir) = argv.get(2) else { fail(USAGE, 2) };
            let dir = Path::new(dir);
            let embedded = Assets::embedded().unwrap_or_else(|e| fail(&format!("embedded assets do not load: {e}"), 1));
            embedded.export(dir)?;
            eprintln!("roguemap-edit: wrote {} files to {}", embedded.files().len(), dir.display());
            edit(dir)
        }
        Some("--help") | Some("-h") => {
            println!("{USAGE}");
            Ok(())
        }
        Some(dir) if !dir.starts_with('-') => edit(Path::new(dir)),
        Some(_) => fail(USAGE, 2),
        None => match std::env::var_os("ROGUEMAP_ASSETS") {
            Some(dir) => edit(Path::new(&dir)),
            None => fail("no asset directory to edit: the embedded set has nowhere to be written back to.\nRun `roguemap-edit --export DIR` to seed a directory, `roguemap-edit DIR` to edit one, or set ROGUEMAP_ASSETS.", 2),
        },
    }
}
