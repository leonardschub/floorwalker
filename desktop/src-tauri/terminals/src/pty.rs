//! One process under a pseudo-terminal the crate owns: no window, a fixed size, and the
//! app's own Claude Code variables kept out of its environment.
//!
//! A reader thread hands the child's output to a callback as it comes. Hanging up wakes
//! that thread so it drops its handle, and drops this one: with the master closed, the
//! kernel sends the child, which leads its own session with the PTY as its controlling
//! terminal, SIGHUP; `claude attach` dies of it and leaves the session running (research
//! §4). A child that outlives the hang-up is killed after a grace, on a thread of its
//! own, so hanging up never blocks the caller.

use std::fs::File;
use std::io::{self, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// How long a hung-up child has to exit before it is killed.
const HANG_UP_GRACE: Duration = Duration::from_secs(2);

/// A child process on the slave side of a PTY whose master this holds.
pub struct Pty {
    master: File,
    /// Always there until the hang-up hands it to its reaper.
    child: Option<Child>,
    /// The write end of the pipe that wakes the reader thread to hang up.
    wake: File,
}

impl Pty {
    /// Runs `program args…` on a new PTY of `cols` × `rows`, as the leader of its own
    /// session with the PTY as its controlling terminal. `TERM` is `xterm-256color` and
    /// `COLORTERM` `truecolor` (an app started from the desktop has neither), and the
    /// app's own Claude Code variables are removed
    /// ([`agents::without_claude_code_variables`]). `output` gets each chunk the child
    /// writes, on a reader thread, until the child is gone or the PTY is hung up.
    pub fn spawn(
        program: &Path,
        args: &[&str],
        cols: u16,
        rows: u16,
        output: impl FnMut(&[u8]) + Send + 'static,
    ) -> io::Result<Pty> {
        let (master, slave) = open(cols, rows)?;
        let mut command = Command::new(program);
        command
            .args(args)
            .env("TERM", "xterm-256color")
            .env("COLORTERM", "truecolor")
            .stdin(Stdio::from(slave.try_clone()?))
            .stdout(Stdio::from(slave.try_clone()?))
            .stderr(Stdio::from(slave));
        agents::without_claude_code_variables(&mut command);
        // SAFETY: only async-signal-safe calls between fork and exec.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(io::Error::last_os_error());
                }
                // stdin is the slave by now: make it the controlling terminal.
                if libc::ioctl(0, libc::TIOCSCTTY, 0) == -1 {
                    return Err(io::Error::last_os_error());
                }
                // The app's threads may block or ignore signals; the child starts clean.
                let mut none: libc::sigset_t = std::mem::zeroed();
                libc::sigemptyset(&mut none);
                libc::sigprocmask(libc::SIG_SETMASK, &none, std::ptr::null_mut());
                for signal in [
                    libc::SIGHUP,
                    libc::SIGINT,
                    libc::SIGQUIT,
                    libc::SIGTERM,
                    libc::SIGPIPE,
                    libc::SIGCHLD,
                ] {
                    libc::signal(signal, libc::SIG_DFL);
                }
                Ok(())
            });
        }
        let mut child = command.spawn()?;
        let (wake_read, wake) = pipe()?;
        let reader = master.try_clone()?;
        let started = std::thread::Builder::new()
            .name("terminals-read".into())
            .spawn(move || read(reader, wake_read, output));
        if let Err(error) = started {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        Ok(Pty {
            master,
            child: Some(child),
            wake,
        })
    }

    /// The child's process id.
    pub fn pid(&self) -> u32 {
        self.child.as_ref().map_or(0, Child::id)
    }

    /// Writes `bytes` to the child's input.
    pub fn write(&self, bytes: &[u8]) -> io::Result<()> {
        (&self.master).write_all(bytes)
    }

    /// Sets the PTY's size; the child gets SIGWINCH.
    pub fn resize(&self, cols: u16, rows: u16) -> io::Result<()> {
        set_size(&self.master, cols, rows)
    }

    /// Whether the child has exited (and is reaped).
    pub fn exited(&mut self) -> bool {
        !matches!(self.child.as_mut().map(Child::try_wait), Some(Ok(None)))
    }

    /// Hangs up, as dropping it does.
    pub fn hang_up(self) {}
}

impl Drop for Pty {
    /// Hangs up: closes the master (this handle as the drop ends, and the reader thread's
    /// once it wakes) and reaps the child on a thread of its own, killing it after
    /// [`HANG_UP_GRACE`] if it outlives the hang-up.
    fn drop(&mut self) {
        let _ = self.wake.write_all(&[0]);
        let Some(mut child) = self.child.take() else {
            return;
        };
        let spawned = std::thread::Builder::new()
            .name("terminals-reap".into())
            .spawn(move || reap(&mut child));
        if let Err(error) = spawned {
            eprintln!("[terminals] could not start a reaper thread: {error}");
        }
    }
}

/// Waits for a hung-up child; kills it if it is still there after the grace.
fn reap(child: &mut Child) {
    let deadline = Instant::now() + HANG_UP_GRACE;
    loop {
        match child.try_wait() {
            Ok(Some(_)) | Err(_) => return,
            Ok(None) if Instant::now() >= deadline => {
                eprintln!(
                    "[terminals] pid {} outlived its hang-up, killing it",
                    child.id()
                );
                let _ = child.kill();
                let _ = child.wait();
                return;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

/// The reader thread: hands `master`'s output to `output` until the child is gone (the
/// read fails with EIO once no slave handle is open) or `wake` becomes readable.
fn read(mut master: File, wake: File, mut output: impl FnMut(&[u8])) {
    use std::io::Read;
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let mut fds = [
            libc::pollfd {
                fd: master.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: wake.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        // SAFETY: poll reads and writes the two pollfds above.
        let ready = unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) };
        if ready == -1 {
            if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return;
        }
        if fds[1].revents != 0 {
            return;
        }
        match master.read(&mut buffer) {
            Ok(0) => return,
            Ok(read) => output(&buffer[..read]),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => return,
        }
    }
}

/// A pipe whose ends close on exec: (read, write).
fn pipe() -> io::Result<(File, File)> {
    let mut fds = [-1; 2];
    // SAFETY: pipe2 writes two fds.
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } == -1 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: both fds were just opened and are owned here alone.
    Ok(unsafe { (File::from_raw_fd(fds[0]), File::from_raw_fd(fds[1])) })
}

/// Opens a PTY pair of `cols` × `rows`; both ends close on exec.
fn open(cols: u16, rows: u16) -> io::Result<(File, OwnedFd)> {
    let mut master = -1;
    let mut slave = -1;
    let size = winsize(cols, rows);
    // SAFETY: openpty writes two fds; the null name and termios are allowed.
    let opened = unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null(),
            &size,
        )
    };
    if opened == -1 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: both fds were just opened and are owned here alone.
    let (master, slave) = unsafe { (File::from_raw_fd(master), OwnedFd::from_raw_fd(slave)) };
    for fd in [master.as_raw_fd(), slave.as_raw_fd()] {
        // SAFETY: fcntl on fds owned here.
        unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) };
    }
    Ok((master, slave))
}

fn set_size(master: &File, cols: u16, rows: u16) -> io::Result<()> {
    let size = winsize(cols, rows);
    // SAFETY: TIOCSWINSZ reads a winsize from the pointer.
    if unsafe { libc::ioctl(master.as_raw_fd(), libc::TIOCSWINSZ, &size) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn winsize(cols: u16, rows: u16) -> libc::winsize {
    libc::winsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    }
}
