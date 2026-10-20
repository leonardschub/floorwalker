//! Checks `claude attach` under the crate's PTY against real background sessions (the
//! spec's "Prove first" 1–4), for when a Claude Code update may have moved things:
//!
//! - `attach_probe draw <id>`: the attach draws at 100 × 30 and takes a key (types `x`,
//!   then erases it), then hangs up.
//! - `attach_probe retitle <id>`: ← on the empty prompt opens agent view; prints the
//!   attach's `/proc` cmdline and comm before and after, then hangs up.
//! - `attach_probe retake <id>`: a second attach holds 80 × 24; the first resizes to
//!   100 × 31 and back to 100 × 30 and prints whether it got a clear and a redraw.
//! - `attach_probe memory <id>…`: one attach per id (repeat ids for more), waits, prints
//!   each attach's PSS and RSS and this process's own.
//!
//! - `attach_probe capture <id> <file>`: saves what the attach drew in its first 8 s to
//!   `<file>`, and what it drew after ← on the empty prompt (agent view) to
//!   `<file>.agents`: the fake attach's fixtures.
//!
//! - `attach_probe paint <id> <png>`: the crate's own [`Terminals`] keeps `<id>` live
//!   for 40 s, ticking every 50 ms; saves the PC image to `<png>` after 10 s and again
//!   (`<png>.last.png`) at the end, and prints every attach that left its session.
//!
//! Run it with `CLAUDECODE` and `CLAUDE_CODE_*` set or not: the PTY scrubs them.
//! `cargo run -p terminals --example attach_probe -- draw <id>`

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use alacritty_terminal::event::VoidListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config, Term};
use alacritty_terminal::vte::ansi::Processor;
use std::time::Instant;

use terminals::pty::Pty;
use terminals::{Terminals, PC_IMAGE_HEIGHT, PC_IMAGE_WIDTH};

fn save_png(rgba: &[u8], path: &str) {
    let file = std::fs::File::create(path).expect("create the PNG");
    let mut encoder = png::Encoder::new(file, PC_IMAGE_WIDTH, PC_IMAGE_HEIGHT);
    encoder.set_color(png::ColorType::Rgba);
    encoder
        .write_header()
        .unwrap()
        .write_image_data(rgba)
        .unwrap();
}

struct Attach {
    pty: Pty,
    term: Arc<Mutex<(Term<VoidListener>, Processor)>>,
    raw: Arc<Mutex<Vec<u8>>>,
}

fn attach(claude: &Path, id: &str, cols: u16, rows: u16) -> Attach {
    let size = TermSize::new(cols as usize, rows as usize);
    let term = Arc::new(Mutex::new((
        Term::new(Config::default(), &size, VoidListener),
        Processor::new(),
    )));
    let raw = Arc::new(Mutex::new(Vec::new()));
    let (fed, logged) = (term.clone(), raw.clone());
    let pty = Pty::spawn(claude, &["attach", id], cols, rows, move |bytes| {
        let mut guard = fed.lock().unwrap();
        let (term, processor) = &mut *guard;
        processor.advance(term, bytes);
        logged.lock().unwrap().extend_from_slice(bytes);
    })
    .expect("spawn claude attach");
    Attach { pty, term, raw }
}

fn screen(attach: &Attach) -> String {
    let guard = attach.term.lock().unwrap();
    let grid = guard.0.grid();
    let mut text = String::new();
    for line in 0..grid.screen_lines() {
        let row: String = (0..grid.columns())
            .map(|col| grid[Line(line as i32)][Column(col)].c)
            .collect();
        text.push('|');
        text.push_str(row.trim_end());
        text.push('\n');
    }
    text
}

fn proc_title(pid: u32) -> String {
    let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
    let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default();
    format!(
        "cmdline={:?} comm={:?}",
        String::from_utf8_lossy(&cmdline),
        comm.trim_end()
    )
}

fn children(pid: u32) -> String {
    std::fs::read_to_string(format!("/proc/{pid}/task/{pid}/children")).unwrap_or_default()
}

fn memory(pid: u32) -> (u64, u64) {
    let text = std::fs::read_to_string(format!("/proc/{pid}/smaps_rollup")).unwrap_or_default();
    let field = |name: &str| {
        text.lines()
            .find(|line| line.starts_with(name))
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|kb| kb.parse::<u64>().ok())
            .unwrap_or(0)
    };
    (field("Pss:") / 1024, field("Rss:") / 1024)
}

fn wait(seconds: u64) {
    std::thread::sleep(Duration::from_secs(seconds));
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let home = PathBuf::from(std::env::var_os("HOME").expect("HOME"));
    let claude = home.join(".local/bin/claude");
    match args.first().map(String::as_str) {
        Some("draw") => {
            let a = attach(&claude, &args[1], 100, 30);
            wait(8);
            println!(
                "pid {} {}\n{}",
                a.pty.pid(),
                proc_title(a.pty.pid()),
                screen(&a)
            );
            a.pty.write(b"x").unwrap();
            wait(2);
            println!("after typing x:\n{}", screen(&a));
            a.pty.write(b"\x7f").unwrap();
            wait(2);
            println!("after backspace:\n{}", screen(&a));
            a.pty.hang_up();
            wait(3);
            println!("hung up");
        }
        Some("retitle") => {
            let mut a = attach(&claude, &args[1], 100, 30);
            wait(8);
            let pid = a.pty.pid();
            println!("before: {} children=[{}]", proc_title(pid), children(pid));
            let environ = std::fs::read(format!("/proc/{pid}/environ")).unwrap_or_default();
            let claude_vars: Vec<String> = environ
                .split(|byte| *byte == 0)
                .map(|entry| {
                    String::from_utf8_lossy(entry)
                        .split('=')
                        .next()
                        .unwrap_or("")
                        .to_owned()
                })
                .filter(|key| key.starts_with("CLAUDE") || key == "TERM" || key == "COLORTERM")
                .collect();
            println!("attach's CLAUDE*/TERM variables: {claude_vars:?}");
            a.pty.write(b"\x1b[D").unwrap();
            wait(3);
            println!("after <-: {} children=[{}]", proc_title(pid), children(pid));
            for child in children(pid).split_whitespace() {
                println!("  child {child}: {}", proc_title(child.parse().unwrap()));
            }
            println!("{}", screen(&a));
            println!("exited: {}", a.pty.exited());
            a.pty.hang_up();
            wait(3);
            println!(
                "hung up; attach pid alive: {}",
                Path::new(&format!("/proc/{pid}")).exists()
            );
        }
        Some("retake") => {
            let a = attach(&claude, &args[1], 100, 30);
            wait(8);
            let b = attach(&claude, &args[1], 80, 24);
            wait(8);
            println!("B attached at 80 x 24; A's screen now:\n{}", screen(&a));
            let mark = a.raw.lock().unwrap().len();
            a.pty.resize(100, 31).unwrap();
            wait(1);
            a.pty.resize(100, 30).unwrap();
            wait(3);
            let after = a.raw.lock().unwrap()[mark..].to_vec();
            let clears = after.windows(4).filter(|w| w == b"\x1b[2J").count();
            println!(
                "after the retake: {} bytes, {} clears (ESC[2J)\n{}",
                after.len(),
                clears,
                screen(&a)
            );
            b.pty.hang_up();
            a.pty.hang_up();
            wait(3);
        }
        Some("memory") => {
            let before = memory(std::process::id());
            let attaches: Vec<Attach> = args[1..]
                .iter()
                .map(|id| attach(&claude, id, 100, 30))
                .collect();
            wait(25);
            let (mut pss, mut rss) = (0, 0);
            for (id, a) in args[1..].iter().zip(&attaches) {
                let (p, r) = memory(a.pty.pid());
                let kids: Vec<u32> = children(a.pty.pid())
                    .split_whitespace()
                    .filter_map(|c| c.parse().ok())
                    .collect();
                println!(
                    "attach {id} pid {}: PSS {p} MB, RSS {r} MB, children {kids:?}",
                    a.pty.pid()
                );
                pss += p;
                rss += r;
            }
            let own = memory(std::process::id());
            println!("attaches total: PSS {pss} MB, RSS {rss} MB");
            println!(
                "this process: PSS {} -> {} MB, RSS {} -> {} MB",
                before.0, own.0, before.1, own.1
            );
            for a in attaches {
                a.pty.hang_up();
            }
            wait(3);
        }
        Some("capture") => {
            let a = attach(&claude, &args[1], 100, 30);
            wait(8);
            let mark = a.raw.lock().unwrap().len();
            std::fs::write(&args[2], &a.raw.lock().unwrap()[..mark]).unwrap();
            a.pty.write(b"\x1b[D").unwrap();
            wait(3);
            std::fs::write(
                format!("{}.agents", args[2]),
                &a.raw.lock().unwrap()[mark..],
            )
            .unwrap();
            println!("{}", screen(&a));
            a.pty.hang_up();
            wait(3);
        }
        Some("paint") => {
            let mut terminals = Terminals::new(&claude, home.join(".claude/sessions"));
            terminals.set_live([args[1].as_str()]);
            let start = Instant::now();
            let mut saved = false;
            while start.elapsed() < Duration::from_secs(40) {
                for left in terminals.tick(Instant::now()) {
                    println!("{:>5.1} s: {left:?}", start.elapsed().as_secs_f32());
                }
                if !saved && start.elapsed() >= Duration::from_secs(10) {
                    let image = terminals.image(&args[1]).expect("painted within 10 s");
                    println!(
                        "{:>5.1} s: saved paint {}",
                        start.elapsed().as_secs_f32(),
                        image.serial
                    );
                    save_png(&image.rgba, &args[2]);
                    saved = true;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            if let Some(image) = terminals.image(&args[1]) {
                println!(
                    "end: paint {}, attached: {}",
                    image.serial,
                    terminals.is_attached(&args[1])
                );
                save_png(&image.rgba, &format!("{}.last.png", args[2]));
            }
            terminals.shutdown();
            wait(3);
        }
        _ => eprintln!("usage: attach_probe draw|retitle|retake|capture|paint <id> | memory <id>…"),
    }
}
