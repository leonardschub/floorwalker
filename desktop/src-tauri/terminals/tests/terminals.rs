//! The terminals crate against the fake attach (`fake_claude.py`), which stands in for
//! `claude` under the crate's real PTYs: spec "Testing Decisions", seam 2.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use terminals::{Leaving, LeftSession, PcImage, Terminals, PC_IMAGE_HEIGHT, PC_IMAGE_WIDTH};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// One test's fake `claude`: a wrapper script running `fake_claude.py` with its own
/// control directory.
struct Fake {
    dir: tempfile::TempDir,
}

impl Fake {
    fn new() -> Fake {
        let dir = tempfile::tempdir().expect("temp dir");
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fake_claude.py");
        let wrapper = dir.path().join("claude");
        std::fs::write(
            &wrapper,
            format!(
                "#!/bin/sh\nexec python3 {} --dir {} \"$@\"\n",
                script.display(),
                dir.path().display()
            ),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::create_dir(dir.path().join("sessions")).unwrap();
        Fake { dir }
    }

    fn claude(&self) -> PathBuf {
        self.dir.path().join("claude")
    }

    /// Terminals running this fake, reading its `sessions/` as Claude Code's.
    fn terminals(&self) -> Terminals {
        Terminals::new(self.claude(), self.dir.path().join("sessions"))
    }

    /// Gives each of `short_ids` a running background session: a session file, as
    /// Claude Code writes for each session's process, naming this test's own process.
    fn running(&self, short_ids: &[&str]) {
        let pid = std::process::id();
        let stat = std::fs::read_to_string("/proc/self/stat").unwrap();
        let start = stat
            .rsplit_once(')')
            .unwrap()
            .1
            .split_whitespace()
            .nth(19)
            .unwrap()
            .to_owned();
        for id in short_ids {
            std::fs::write(
                self.dir.path().join(format!("sessions/{id}.json")),
                format!(r#"{{"pid":{pid},"kind":"bg","jobId":"{id}","procStart":"{start}","status":"idle"}}"#),
            )
            .unwrap();
        }
    }

    /// Ends the session's process: its session file goes, as Claude Code removes it.
    fn stop(&self, short_id: &str) {
        std::fs::remove_file(self.dir.path().join(format!("sessions/{short_id}.json"))).unwrap();
    }

    /// What the fake attach of `short_id` draws as it starts.
    fn replay(&self, short_id: &str, fixture_name: &str) {
        std::fs::copy(
            fixture(fixture_name),
            self.dir.path().join(format!("{short_id}.replay")),
        )
        .unwrap();
    }

    /// Makes the fake attach of `short_id` draw `bytes` now.
    fn draw(&self, short_id: &str, bytes: &[u8]) {
        let name = format!("draw-{}", self.count_draws());
        let path = self.dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        self.command(short_id, &format!("write {}", path.display()));
    }

    fn count_draws(&self) -> usize {
        std::fs::read_dir(self.dir.path())
            .unwrap()
            .filter(|entry| {
                entry
                    .as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with("draw-")
            })
            .count()
    }

    fn command(&self, short_id: &str, line: &str) {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.dir.path().join(format!("{short_id}.cmd")))
            .unwrap();
        writeln!(file, "{line}").unwrap();
    }

    fn log(&self) -> Vec<String> {
        std::fs::read_to_string(self.dir.path().join("log"))
            .unwrap_or_default()
            .lines()
            .map(|line| {
                // Drop the pid: `start <id>`, `hangup <id>`, `exit <id>`.
                line.rsplit_once(' ')
                    .map_or(line, |(event, _pid)| event)
                    .to_owned()
            })
            .collect()
    }

    /// Every byte typed into the fake attach of `short_id` so far.
    fn typed(&self, short_id: &str) -> Vec<u8> {
        std::fs::read(self.dir.path().join(format!("{short_id}.input"))).unwrap_or_default()
    }

    fn count(&self, event: &str) -> usize {
        self.log().iter().filter(|line| *line == event).count()
    }
}

/// Polls `done` for up to 10 s.
fn eventually(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn the_live_set_opens_and_hangs_up_attaches() {
    let fake = Fake::new();
    fake.running(&["aaaa1111", "bbbb2222", "cccc3333"]);
    let mut terminals = fake.terminals();

    terminals.set_live(["aaaa1111", "bbbb2222"]);
    terminals.tick(Instant::now());
    eventually("both attaches to start", || {
        fake.count("start aaaa1111") == 1 && fake.count("start bbbb2222") == 1
    });
    assert!(terminals.is_attached("aaaa1111"));
    assert!(terminals.is_attached("bbbb2222"));

    terminals.set_live(["bbbb2222", "cccc3333"]);
    assert!(
        !terminals.is_attached("aaaa1111"),
        "a session leaving the set is hung up at once"
    );
    terminals.tick(Instant::now());
    eventually("aaaa1111 hung up and cccc3333 started", || {
        fake.count("hangup aaaa1111") == 1 && fake.count("start cccc3333") == 1
    });
    assert_eq!(
        fake.count("start bbbb2222"),
        1,
        "a session staying live keeps its attach"
    );
    assert_eq!(fake.count("hangup bbbb2222"), 0);
}

/// The colour of the pixel at `x`, `y` (RGB).
fn pixel(image: &PcImage, x: u32, y: u32) -> [u8; 3] {
    let at = ((y * PC_IMAGE_WIDTH + x) * 4) as usize;
    [image.rgba[at], image.rgba[at + 1], image.rgba[at + 2]]
}

#[test]
fn a_replayed_attach_fills_the_grid_and_paints_the_pc_image() {
    let fake = Fake::new();
    fake.replay("aaaa1111", "session.bytes");
    fake.running(&["aaaa1111"]);
    let mut terminals = fake.terminals();
    terminals.set_live(["aaaa1111"]);
    terminals.tick(Instant::now());
    eventually("the replay to reach the grid", || {
        terminals
            .screen_text("aaaa1111")
            .is_some_and(|text| text.contains("as10-probe-a"))
    });

    let text = terminals.screen_text("aaaa1111").unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 30);
    assert_eq!(lines[1], " ▐▛███▛█   Claude Code v2.1.303");
    assert_eq!(lines[3], " ▝▝   ▝▝   /tmp/claude-1000/bgprobe");
    // Claude Code puts a no-break space after the prompt sign.
    assert_eq!(lines[27], "❯\u{a0}Try \"edit <filepath> to...\"");
    assert!(lines[26].ends_with("── as10-probe-a ─"), "{}", lines[26]);
    assert_eq!(lines[26].chars().count(), 100);

    assert!(
        terminals.image("aaaa1111").is_none(),
        "nothing is painted before a tick"
    );
    terminals.tick(Instant::now() + Duration::from_secs(1));
    let image = terminals.image("aaaa1111").expect("painted on the tick");
    assert_eq!(
        image.rgba.len(),
        (PC_IMAGE_WIDTH * PC_IMAGE_HEIGHT * 4) as usize
    );
    assert_eq!((PC_IMAGE_WIDTH, PC_IMAGE_HEIGHT), (600, 360));
    // Cells are 6 × 12 px. Row 1, column 1 is `▐` in the logo's orange (`38;2;215;119;87`)
    // on the default black: its right half is orange, its left half black.
    assert_eq!(pixel(&image, 6 + 4, 12 + 6), [215, 119, 87]);
    assert_eq!(pixel(&image, 6 + 1, 12 + 6), [0, 0, 0]);
    // Row 1, columns 2–7 are on `48;2;0;0;0`, and `█` at column 3 is all orange.
    assert_eq!(pixel(&image, 3 * 6 + 3, 12 + 2), [215, 119, 87]);
    // An empty cell is black all over.
    let blank = (0..12).flat_map(|y| (0..6).map(move |x| (50 * 6 + x, 12 * 12 + y)));
    assert!(blank
        .into_iter()
        .all(|(x, y)| pixel(&image, x, y) == [0, 0, 0]));
    // Row 29 starts `  ⏵⏵ auto mode on`: JetBrains Mono has no `⏵`, so a stand-in shape
    // from the font is drawn rather than nothing.
    let play = (0..12).flat_map(|y| (2 * 6..3 * 6).map(move |x| (x, 29 * 12 + y)));
    assert!(
        play.into_iter()
            .any(|(x, y)| pixel(&image, x, y) != [0, 0, 0]),
        "⏵ is drawn"
    );
    // Row 3's path is grey text (`38;2;153;153;153`) on black: antialiased greys up to
    // that one, and nothing else.
    let path: Vec<[u8; 3]> = (0..12)
        .flat_map(|y| (12 * 6..24 * 6).map(move |x| (x, 3 * 12 + y)))
        .map(|(x, y)| pixel(&image, x, y))
        .collect();
    assert!(path.iter().all(|[r, g, b]| r == g && g == b && *r <= 153));
    assert!(path.iter().any(|[r, _, _]| *r >= 120), "the text shows");
}

#[test]
fn the_pc_image_is_repainted_only_when_the_grid_changed_and_at_most_twice_a_second() {
    let fake = Fake::new();
    fake.replay("aaaa1111", "session.bytes");
    fake.running(&["aaaa1111"]);
    let mut terminals = fake.terminals();
    let start = Instant::now();
    terminals.set_live(["aaaa1111"]);
    terminals.tick(start);
    eventually("the replay", || {
        terminals
            .screen_text("aaaa1111")
            .is_some_and(|text| text.contains("as10-probe-a"))
    });
    let start = start + Duration::from_secs(1);
    terminals.tick(start);
    let first = terminals.image("aaaa1111").unwrap();

    terminals.tick(start + Duration::from_millis(600));
    assert_eq!(
        terminals.image("aaaa1111").unwrap().serial,
        first.serial,
        "no output, no repaint"
    );

    // `hello` at row 15, column 10 (1-based).
    fake.draw("aaaa1111", b"\x1b[15;10Hhello");
    eventually("the new output", || {
        terminals.screen_text("aaaa1111").unwrap().contains("hello")
    });
    terminals.tick(start + Duration::from_millis(800));
    let second = terminals.image("aaaa1111").unwrap();
    assert_eq!(second.serial, first.serial + 1);
    let hello = |image: &PcImage| {
        (14 * 12..15 * 12)
            .flat_map(|y| (9 * 6..14 * 6).map(move |x| (x, y)))
            .filter(|(x, y)| pixel(image, *x, *y) != [0, 0, 0])
            .count()
    };
    assert_eq!(hello(&first), 0);
    assert!(hello(&second) > 20, "hello is painted");

    fake.draw("aaaa1111", b"\x1b[16;10Hworld");
    eventually("more output", || {
        terminals.screen_text("aaaa1111").unwrap().contains("world")
    });
    terminals.tick(start + Duration::from_millis(1100));
    assert_eq!(
        terminals.image("aaaa1111").unwrap().serial,
        second.serial,
        "300 ms after a paint is too soon"
    );
    terminals.tick(start + Duration::from_millis(1300));
    assert_eq!(
        terminals.image("aaaa1111").unwrap().serial,
        second.serial + 1,
        "500 ms after it is not"
    );
}

/// A live `aaaa1111` whose fake attach has drawn the captured session, painted at the
/// returned time.
fn live_session(fake: &Fake) -> (Terminals, Instant) {
    fake.replay("aaaa1111", "session.bytes");
    fake.running(&["aaaa1111"]);
    let mut terminals = fake.terminals();
    let start = Instant::now();
    terminals.set_live(["aaaa1111"]);
    terminals.tick(start);
    eventually("the replay", || {
        terminals
            .screen_text("aaaa1111")
            .is_some_and(|text| text.contains("as10-probe-a"))
    });
    let painted = start + Duration::from_secs(1);
    terminals.tick(painted);
    assert!(terminals.image("aaaa1111").is_some());
    (terminals, painted)
}

/// Ticks every 100 ms of made-up time from `from` until a tick reports an attach that
/// left its session; returns the report and the time of that tick.
fn tick_until_left(terminals: &mut Terminals, from: Instant) -> (Vec<LeftSession>, Instant) {
    let mut now = from;
    let mut left = Vec::new();
    eventually("an attach to leave its session", || {
        now += Duration::from_millis(100);
        left = terminals.tick(now);
        !left.is_empty()
    });
    (left, now)
}

#[test]
fn an_attach_retitled_claude_agents_has_left_the_session_and_is_attached_again() {
    let fake = Fake::new();
    let (mut terminals, painted) = live_session(&fake);
    let before = terminals.image("aaaa1111").unwrap();

    // Agent view by its title alone: the fake retitles itself and draws nothing.
    let nothing = fake.dir.path().join("nothing");
    std::fs::write(&nothing, b"").unwrap();
    fake.command("aaaa1111", &format!("agents {}", nothing.display()));
    let (left, at) = tick_until_left(&mut terminals, painted);

    assert_eq!(
        left,
        vec![LeftSession {
            short_id: "aaaa1111".into(),
            why: Leaving::AgentView
        }]
    );
    eventually("the hang-up", || fake.count("hangup aaaa1111") == 1);
    assert_eq!(
        terminals.image("aaaa1111").unwrap().serial,
        before.serial,
        "the last frame stays"
    );

    // Still live, so attached again, though not straight away.
    terminals.tick(at + Duration::from_millis(100));
    assert!(!terminals.is_attached("aaaa1111"));
    terminals.tick(at + Duration::from_secs(3));
    assert!(terminals.is_attached("aaaa1111"));
    eventually("the second attach", || fake.count("start aaaa1111") == 2);
}

#[test]
fn agent_view_text_on_the_grid_counts_as_leaving_and_is_never_painted() {
    let fake = Fake::new();
    let (mut terminals, painted) = live_session(&fake);
    let before = terminals.image("aaaa1111").unwrap();

    // Agent view drawn without the retitle: its text is the only sign.
    fake.command(
        "aaaa1111",
        &format!("write {}", fixture("agent-view.bytes").display()),
    );
    let (left, _) = tick_until_left(&mut terminals, painted);

    assert_eq!(
        left,
        vec![LeftSession {
            short_id: "aaaa1111".into(),
            why: Leaving::AgentView
        }]
    );
    assert_eq!(
        terminals.image("aaaa1111").unwrap().serial,
        before.serial,
        "agent view is never painted"
    );
    eventually("the hang-up", || fake.count("hangup aaaa1111") == 1);
}

#[test]
fn an_attach_that_exits_has_left_the_session_and_its_last_frame_stays() {
    let fake = Fake::new();
    let (mut terminals, painted) = live_session(&fake);
    let before = terminals.image("aaaa1111").unwrap();

    fake.command("aaaa1111", "exit");
    let (left, at) = tick_until_left(&mut terminals, painted);

    assert_eq!(
        left,
        vec![LeftSession {
            short_id: "aaaa1111".into(),
            why: Leaving::Exited
        }]
    );
    assert_eq!(
        terminals.image("aaaa1111").unwrap().serial,
        before.serial,
        "the blank main screen is not painted"
    );
    terminals.tick(at + Duration::from_secs(3));
    eventually("the second attach", || fake.count("start aaaa1111") == 2);
}

#[test]
fn a_grid_off_the_alternate_screen_is_not_painted() {
    // The attach draws its session on the alternate screen and leaves it first when it
    // exits or turns into agent view: the main screen between is blank.
    let fake = Fake::new();
    let (mut terminals, painted) = live_session(&fake);
    let before = terminals.image("aaaa1111").unwrap();

    fake.draw("aaaa1111", b"\x1b[?1049l");
    eventually("the output", || {
        terminals.screen_text("aaaa1111").unwrap().trim().is_empty()
    });
    assert!(terminals.tick(painted + Duration::from_secs(1)).is_empty());
    assert_eq!(terminals.image("aaaa1111").unwrap().serial, before.serial);

    fake.draw("aaaa1111", b"\x1b[?1049h\x1b[2J\x1b[Hback");
    eventually("the output", || {
        terminals.screen_text("aaaa1111").unwrap().contains("back")
    });
    terminals.tick(painted + Duration::from_secs(2));
    assert_eq!(
        terminals.image("aaaa1111").unwrap().serial,
        before.serial + 1
    );
}

#[test]
fn a_session_leaving_the_live_set_keeps_its_last_frame() {
    let fake = Fake::new();
    let (mut terminals, painted) = live_session(&fake);
    let last = terminals.image("aaaa1111").unwrap();

    terminals.set_live(Vec::<String>::new());
    terminals.tick(painted + Duration::from_secs(1));
    assert!(!terminals.is_attached("aaaa1111"));
    eventually("the hang-up", || fake.count("hangup aaaa1111") == 1);
    let frozen = terminals.image("aaaa1111").expect("the frozen frame");
    assert_eq!(frozen.serial, last.serial);
    assert_eq!(frozen.rgba, last.rgba);

    // Live again: the frozen frame shows until the new attach paints.
    terminals.set_live(["aaaa1111"]);
    terminals.tick(painted + Duration::from_secs(2));
    assert!(terminals.is_attached("aaaa1111"));
    assert_eq!(terminals.image("aaaa1111").unwrap().serial, last.serial);
    eventually("the replay", || {
        terminals
            .screen_text("aaaa1111")
            .is_some_and(|text| text.contains("as10-probe-a"))
    });
    terminals.tick(painted + Duration::from_secs(3));
    assert_eq!(terminals.image("aaaa1111").unwrap().serial, last.serial + 1);
}

#[test]
fn an_attach_runs_without_the_apps_claude_code_variables() {
    // The app may have been started from inside a Claude Code session.
    std::env::set_var("CLAUDECODE", "1");
    std::env::set_var("CLAUDE_CODE_ENTRYPOINT", "cli");
    std::env::set_var("CLAUDE_CODE_SESSION_ID", "the-apps-own");
    std::env::set_var("TERMINALS_TEST_KEPT", "kept");
    std::env::set_var("TERM", "dumb");
    let fake = Fake::new();
    fake.running(&["aaaa1111"]);
    let mut terminals = fake.terminals();
    terminals.set_live(["aaaa1111"]);
    terminals.tick(Instant::now());
    let env_file = fake.dir.path().join("aaaa1111.env");
    eventually("the environment", || {
        std::fs::read_to_string(&env_file).is_ok_and(|env| env.contains("TERM="))
    });

    let env = std::fs::read_to_string(&env_file).unwrap();
    let keys: Vec<&str> = env
        .lines()
        .filter_map(|line| line.split_once('=').map(|(key, _)| key))
        .collect();
    assert!(
        !keys
            .iter()
            .any(|key| *key == "CLAUDECODE" || key.starts_with("CLAUDE_CODE_")),
        "{keys:?}"
    );
    assert!(env.lines().any(|line| line == "TERMINALS_TEST_KEPT=kept"));
    assert!(env.lines().any(|line| line == "TERM=xterm-256color"));
    assert!(env.lines().any(|line| line == "COLORTERM=truecolor"));
}

#[test]
fn shutting_down_hangs_up_every_attach_for_good() {
    let fake = Fake::new();
    fake.running(&["aaaa1111", "bbbb2222"]);
    let mut terminals = fake.terminals();
    let start = Instant::now();
    terminals.set_live(["aaaa1111", "bbbb2222"]);
    terminals.tick(start);
    eventually("both attaches", || {
        fake.count("start aaaa1111") == 1 && fake.count("start bbbb2222") == 1
    });

    terminals.shutdown();
    eventually("both hang-ups", || {
        fake.count("hangup aaaa1111") == 1 && fake.count("hangup bbbb2222") == 1
    });
    terminals.tick(start + Duration::from_secs(5));
    assert!(!terminals.is_attached("aaaa1111"));
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(
        fake.count("start aaaa1111"),
        1,
        "nothing attaches after a shutdown"
    );
}

#[test]
fn dropping_the_terminals_hangs_up_every_attach() {
    let fake = Fake::new();
    fake.running(&["aaaa1111"]);
    let mut terminals = fake.terminals();
    terminals.set_live(["aaaa1111"]);
    terminals.tick(Instant::now());
    eventually("the attach", || fake.count("start aaaa1111") == 1);

    drop(terminals);
    eventually("the hang-up", || fake.count("hangup aaaa1111") == 1);
}

#[test]
fn a_live_session_without_a_running_process_is_never_attached() {
    // `claude attach` on a stopped or dormant session starts it again (ticket 10's real
    // run): a live set a few seconds stale must not wake it.
    let fake = Fake::new();
    let mut terminals = fake.terminals();
    let start = Instant::now();
    terminals.set_live(["aaaa1111"]);
    terminals.tick(start);
    assert!(!terminals.is_attached("aaaa1111"), "no session file");

    // A session file whose process is gone.
    std::fs::write(
        fake.dir.path().join("sessions/aaaa1111.json"),
        r#"{"pid":999999999,"kind":"bg","jobId":"aaaa1111","procStart":"1"}"#,
    )
    .unwrap();
    terminals.tick(start + Duration::from_secs(3));
    assert!(!terminals.is_attached("aaaa1111"), "a dead process");

    fake.running(&["aaaa1111"]);
    terminals.tick(start + Duration::from_secs(6));
    assert!(terminals.is_attached("aaaa1111"));
    eventually("the attach", || fake.count("start aaaa1111") == 1);
}

#[test]
fn an_attach_that_left_a_stopped_session_is_not_followed_by_another() {
    let fake = Fake::new();
    let (mut terminals, painted) = live_session(&fake);

    // The session is stopped (a kill, `claude stop`): its process goes and the attach exits.
    fake.stop("aaaa1111");
    fake.command("aaaa1111", "exit");
    let (left, at) = tick_until_left(&mut terminals, painted);
    assert_eq!(left[0].why, Leaving::Exited);

    terminals.tick(at + Duration::from_secs(3));
    terminals.tick(at + Duration::from_secs(6));
    assert!(!terminals.is_attached("aaaa1111"));
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(fake.count("start aaaa1111"), 1, "never attached again");
    assert!(
        terminals.image("aaaa1111").is_some(),
        "its last frame stays"
    );
}

/// Drains the seated stream for up to 10 s while ticking with the time from `from` as it
/// passes (the size retake waits real time for the attach), until `done` holds for what
/// came; returns it.
fn stream_until(
    terminals: &mut Terminals,
    from: Instant,
    mut done: impl FnMut(&[u8]) -> bool,
) -> Vec<u8> {
    let mut streamed = Vec::new();
    let began = Instant::now();
    eventually("the seated stream", || {
        terminals.tick(from + began.elapsed());
        streamed.extend(terminals.seated_output());
        done(&streamed)
    });
    streamed
}

fn holds(bytes: &[u8], part: &[u8]) -> bool {
    bytes.windows(part.len()).any(|window| window == part)
}

#[test]
fn sitting_in_retakes_the_size_and_streams_the_full_redraw() {
    let fake = Fake::new();
    let (mut terminals, painted) = live_session(&fake);
    assert!(
        terminals.seated_output().is_empty(),
        "nothing streams while nobody is seated"
    );

    terminals.seat(Some("aaaa1111"), painted);
    // 100 × 31, then back to 100 × 30: the attach redraws its whole screen, which fills
    // the fresh overlay (the fake clears and replays its frame on each size change).
    let streamed = stream_until(&mut terminals, painted, |bytes| {
        fake.count("winch aaaa1111 100x31") == 1
            && fake.count("winch aaaa1111 100x30") == 1
            // A clear and a redraw for each size change.
            && bytes.windows(4).filter(|w| w == b"\x1b[2J").count() >= 2
            && holds(bytes, "as10-probe-a".as_bytes())
    });
    assert!(holds(&streamed, b"\x1b[2J"), "a full clear");
    // The grid the PC is painted from stays 100 × 30.
    let text = terminals.screen_text("aaaa1111").unwrap();
    assert_eq!(text.lines().count(), 30);

    // What the session draws next streams as it comes.
    fake.draw("aaaa1111", b"typed on the session");
    stream_until(&mut terminals, painted + Duration::from_secs(5), |bytes| {
        holds(bytes, b"typed on the session")
    });

    // Leaving stops the stream.
    terminals.seat(None, painted + Duration::from_secs(20));
    fake.draw("aaaa1111", b"after leaving");
    eventually("the drawing to reach the grid", || {
        terminals
            .screen_text("aaaa1111")
            .is_some_and(|text| text.contains("after leaving"))
    });
    terminals.tick(painted + Duration::from_secs(21));
    assert!(terminals.seated_output().is_empty());
}

#[test]
fn only_the_seated_attach_streams() {
    let fake = Fake::new();
    fake.replay("aaaa1111", "session.bytes");
    fake.replay("bbbb2222", "session.bytes");
    fake.running(&["aaaa1111", "bbbb2222"]);
    let mut terminals = fake.terminals();
    let start = Instant::now();
    terminals.set_live(["aaaa1111", "bbbb2222"]);
    terminals.tick(start);
    eventually("both replays", || {
        ["aaaa1111", "bbbb2222"].iter().all(|id| {
            terminals
                .screen_text(id)
                .is_some_and(|text| text.contains("as10-probe-a"))
        })
    });

    terminals.seat(Some("bbbb2222"), start);
    stream_until(&mut terminals, start, |bytes| holds(bytes, b"\x1b[2J"));
    fake.draw("aaaa1111", b"from the other session");
    fake.draw("bbbb2222", b"from the seated session");
    let streamed = stream_until(&mut terminals, start + Duration::from_secs(5), |bytes| {
        holds(bytes, b"from the seated session")
    });
    assert!(!holds(&streamed, b"from the other session"));
    assert_eq!(
        fake.count("winch aaaa1111 100x31"),
        0,
        "the other keeps its size"
    );
}

#[test]
fn a_seat_taken_before_its_attach_starts_streams_the_attach_from_its_first_byte() {
    let fake = Fake::new();
    fake.replay("aaaa1111", "session.bytes");
    fake.running(&["aaaa1111"]);
    let mut terminals = fake.terminals();
    let start = Instant::now();
    // Sat in from afar: the seated session joins the live set as the seat is taken.
    terminals.set_live(["aaaa1111"]);
    terminals.seat(Some("aaaa1111"), start);
    let streamed = stream_until(&mut terminals, start, |bytes| {
        holds(bytes, "as10-probe-a".as_bytes())
    });
    // The attach's own first frame, whole: no retake is needed.
    assert!(
        holds(&streamed, b"\x1b[?1049h"),
        "from the alternate screen's start"
    );
    assert_eq!(fake.count("winch aaaa1111 100x31"), 0);
}

#[test]
fn input_reaches_only_the_seated_attach_and_is_ignored_while_nobody_is_seated() {
    let fake = Fake::new();
    fake.replay("aaaa1111", "session.bytes");
    fake.replay("bbbb2222", "session.bytes");
    fake.running(&["aaaa1111", "bbbb2222"]);
    let mut terminals = fake.terminals();
    let start = Instant::now();
    terminals.set_live(["aaaa1111", "bbbb2222"]);
    terminals.tick(start);
    // Both drew their frame, so both have put their terminals in raw mode (as the real
    // attach does): Ctrl+C is a byte to them, not a signal.
    eventually("both replays", || {
        ["aaaa1111", "bbbb2222"].iter().all(|id| {
            terminals
                .screen_text(id)
                .is_some_and(|text| text.contains("as10-probe-a"))
        })
    });

    assert!(
        !terminals.input(b"nobody is seated"),
        "nothing is written while nobody is seated"
    );

    terminals.seat(Some("bbbb2222"), start);
    // Keys as xterm.js encodes them: a prompt and Enter, Esc, Ctrl+C.
    assert!(terminals.input(b"hello\r"));
    assert!(terminals.input(b"\x1b"));
    assert!(terminals.input(b"\x03"));
    eventually("the typing to reach the seated attach", || {
        fake.typed("bbbb2222") == b"hello\r\x1b\x03"
    });

    // Leaving: the next key goes nowhere.
    terminals.seat(None, start + Duration::from_secs(1));
    assert!(!terminals.input(b"after leaving"));
    // A seat whose session has no attach yet takes nothing either.
    terminals.seat(Some("cccc3333"), start + Duration::from_secs(2));
    assert!(!terminals.input(b"to no attach"));

    std::thread::sleep(Duration::from_millis(300));
    assert!(fake.typed("aaaa1111").is_empty(), "never the other session");
    assert_eq!(fake.typed("bbbb2222"), b"hello\r\x1b\x03");
}

#[test]
fn agent_view_never_streams_to_the_overlay() {
    let fake = Fake::new();
    let (mut terminals, painted) = live_session(&fake);
    terminals.seat(Some("aaaa1111"), painted);
    stream_until(&mut terminals, painted, |bytes| {
        holds(bytes, "as10-probe-a".as_bytes())
    });

    // `/exit`, ← or a double Ctrl+C on an empty prompt: the attach leaves the alternate
    // screen and draws agent view, which the overlay must never show.
    fake.command(
        "aaaa1111",
        &format!("agents {}", fixture("agent-view.bytes").display()),
    );
    let mut streamed = Vec::new();
    let began = Instant::now();
    let mut left = Vec::new();
    eventually("the seated attach to leave its session", || {
        left = terminals.tick(painted + Duration::from_secs(5) + began.elapsed());
        streamed.extend(terminals.seated_output());
        !left.is_empty()
    });
    assert_eq!(left[0].short_id, "aaaa1111");
    assert!(
        !holds(&streamed, b"describe a task for a new session"),
        "agent view's prompt never streams"
    );
    assert!(!holds(&streamed, b"claude agents"), "nor its title");
}

#[test]
fn typing_stops_once_the_seated_attach_leaves_the_alternate_screen() {
    let fake = Fake::new();
    let (mut terminals, painted) = live_session(&fake);
    terminals.seat(Some("aaaa1111"), painted);
    assert!(terminals.input(b"before"));
    eventually("the typing to reach the seated attach", || {
        fake.typed("aaaa1111") == b"before"
    });

    // `/exit`, ← or a double Ctrl+C on an empty prompt: the attach leaves the alternate
    // screen and draws agent view (which enters it again). Keys typed before the next
    // check notices must never reach agent view, which can dispatch to or delete
    // sessions.
    fake.command(
        "aaaa1111",
        &format!("write {}", fixture("agent-view.bytes").display()),
    );
    eventually("agent view to reach the grid", || {
        terminals
            .screen_text("aaaa1111")
            .is_some_and(|text| text.contains("describe a task for a new session"))
    });
    assert!(!terminals.input(b"\x7f"), "no key reaches agent view");
    // Nor once the seat is taken again on the same attach.
    terminals.seat(None, painted);
    terminals.seat(Some("aaaa1111"), painted);
    assert!(!terminals.input(b"\r"), "nor after sitting in again");

    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(fake.typed("aaaa1111"), b"before");
}
