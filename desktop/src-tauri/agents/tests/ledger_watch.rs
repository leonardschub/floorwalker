//! The "ledger grew" signal (spec "The data contract": an open statistics view asks
//! again when Rust signals that the ledger grew), driven through its public seam: a
//! temporary `~/.opencodex` whose ledger the test appends to, short timings, and the
//! signals the sink receives. It watches only while a statistics view keeps asking.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use agents::{LedgerTiming, LedgerWatch};

const TIMING: LedgerTiming = LedgerTiming {
    look_every: Duration::from_millis(20),
    lease: Duration::from_millis(300),
};

fn grow(opencodex: &Path) {
    let mut ledger = OpenOptions::new()
        .create(true)
        .append(true)
        .open(opencodex.join("usage.jsonl"))
        .unwrap();
    ledger.write_all(b"{}\n").unwrap();
}

fn watch(opencodex: &Path) -> (LedgerWatch, mpsc::Receiver<()>) {
    fs::create_dir_all(opencodex).unwrap();
    grow(opencodex);
    let (send, grew) = mpsc::channel();
    let watch = LedgerWatch::new(opencodex, TIMING, move || {
        let _ = send.send(());
    });
    (watch, grew)
}

#[test]
fn nothing_is_watched_before_a_statistics_view_asks() {
    let home = tempfile::tempdir().unwrap();
    let opencodex = home.path().join(".opencodex");
    let (_watch, grew) = watch(&opencodex);

    grow(&opencodex);

    assert!(grew.recv_timeout(Duration::from_millis(200)).is_err());
}

#[test]
fn while_views_keep_asking_growth_is_signalled() {
    let home = tempfile::tempdir().unwrap();
    let opencodex = home.path().join(".opencodex");
    let (watch, grew) = watch(&opencodex);
    watch.renew();
    std::thread::sleep(Duration::from_millis(60));

    grow(&opencodex);

    grew.recv_timeout(Duration::from_millis(250))
        .expect("the growth is signalled");
}

#[test]
fn once_no_view_has_asked_for_a_lease_it_stops_until_one_asks_again() {
    let home = tempfile::tempdir().unwrap();
    let opencodex = home.path().join(".opencodex");
    let (watch, grew) = watch(&opencodex);
    watch.renew();
    std::thread::sleep(TIMING.lease + Duration::from_millis(150));

    grow(&opencodex);
    assert!(
        grew.recv_timeout(Duration::from_millis(200)).is_err(),
        "no signal after the lease ran out"
    );

    watch.renew();
    std::thread::sleep(Duration::from_millis(60));
    grow(&opencodex);
    grew.recv_timeout(Duration::from_millis(250))
        .expect("asking again watches again");
}
