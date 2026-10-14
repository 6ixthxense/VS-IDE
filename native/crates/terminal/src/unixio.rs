//! Interruptible PTY I/O on Unix.
//!
//! The reader and writer poll the master fd together with a "wake" pipe. Dropping/firing the
//! wake guard makes both return immediately, so closing a session never leaks its threads even
//! when a background process keeps the slave side of the PTY open (no EOF would ever arrive).

use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::sync::Arc;

use parking_lot::Mutex;
use portable_pty::MasterPty;

/// Closing the write end of the pipe wakes every poller.
pub(crate) struct WakeGuard(Mutex<Option<OwnedFd>>);

impl WakeGuard {
    pub(crate) fn fire(&self) {
        self.0.lock().take();
    }
}

struct Wake(OwnedFd);

fn cvt(r: libc::c_int) -> io::Result<libc::c_int> {
    if r < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(r)
    }
}

fn dup_cloexec(fd: RawFd) -> io::Result<OwnedFd> {
    // SAFETY: fcntl(F_DUPFD_CLOEXEC) returns a fresh fd that we take ownership of.
    let n = cvt(unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 0) })?;
    Ok(unsafe { OwnedFd::from_raw_fd(n) })
}

fn set_cloexec(fd: RawFd) -> io::Result<()> {
    // SAFETY: plain fcntl calls on an fd we own.
    let flags = cvt(unsafe { libc::fcntl(fd, libc::F_GETFD) })?;
    cvt(unsafe { libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) })?;
    Ok(())
}

fn set_nonblocking(fd: RawFd) -> io::Result<()> {
    // SAFETY: plain fcntl calls on an fd we own.
    let flags = cvt(unsafe { libc::fcntl(fd, libc::F_GETFL) })?;
    cvt(unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) })?;
    Ok(())
}

/// Wait until `fd` has `events` or the wake pipe fires. Returns `Ok(None)` when woken.
fn poll_wake(fd: RawFd, events: libc::c_short, wake: &Wake) -> io::Result<Option<libc::c_short>> {
    loop {
        let mut fds = [
            libc::pollfd { fd, events, revents: 0 },
            libc::pollfd { fd: wake.0.as_raw_fd(), events: libc::POLLIN, revents: 0 },
        ];
        // SAFETY: `fds` is a valid array of two pollfd structs.
        let r = unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) };
        if r < 0 {
            let e = io::Error::last_os_error();
            if e.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(e);
        }
        if fds[1].revents != 0 {
            return Ok(None);
        }
        if fds[0].revents != 0 {
            return Ok(Some(fds[0].revents));
        }
    }
}

struct PtyReader {
    fd: OwnedFd,
    wake: Arc<Wake>,
}

impl Read for PtyReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            if poll_wake(self.fd.as_raw_fd(), libc::POLLIN, &self.wake)?.is_none() {
                return Ok(0);
            }
            // SAFETY: buf is valid for buf.len() bytes.
            let n = unsafe { libc::read(self.fd.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
            if n >= 0 {
                return Ok(n as usize);
            }
            let e = io::Error::last_os_error();
            match e.kind() {
                io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock => continue,
                _ => {
                    // Linux reports EIO once every slave fd is closed: that is EOF.
                    if e.raw_os_error() == Some(libc::EIO) {
                        return Ok(0);
                    }
                    return Err(e);
                }
            }
        }
    }
}

struct PtyWriter {
    fd: OwnedFd,
    wake: Arc<Wake>,
}

impl Write for PtyWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        loop {
            // SAFETY: buf is valid for buf.len() bytes.
            let n = unsafe { libc::write(self.fd.as_raw_fd(), buf.as_ptr().cast(), buf.len()) };
            if n >= 0 {
                return Ok(n as usize);
            }
            let e = io::Error::last_os_error();
            match e.kind() {
                io::ErrorKind::Interrupted => continue,
                io::ErrorKind::WouldBlock => {
                    // Child is not reading: wait for room (or for the session to close).
                    if poll_wake(self.fd.as_raw_fd(), libc::POLLOUT, &self.wake)?.is_none() {
                        return Err(io::Error::new(io::ErrorKind::BrokenPipe, "terminal closed"));
                    }
                }
                _ => return Err(e),
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Reader, writer and the guard that interrupts both.
pub(crate) type PtyIo = (Box<dyn Read + Send>, Box<dyn Write + Send>, WakeGuard);

/// Build reader/writer over (dups of) the master fd plus the wake guard.
pub(crate) fn open(
    master: &dyn MasterPty,
) -> io::Result<PtyIo> {
    let raw = master
        .as_raw_fd()
        .ok_or_else(|| io::Error::other("pty master has no file descriptor"))?;
    let r = dup_cloexec(raw)?;
    let w = dup_cloexec(raw)?;
    // O_NONBLOCK lives on the open file description, shared by all dups.
    set_nonblocking(r.as_raw_fd())?;

    let mut fds = [0 as libc::c_int; 2];
    // SAFETY: fds has room for two ints.
    cvt(unsafe { libc::pipe(fds.as_mut_ptr()) })?;
    // SAFETY: pipe() returned two fresh fds we now own.
    let (rd, wr) = unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
    set_cloexec(rd.as_raw_fd())?;
    set_cloexec(wr.as_raw_fd())?;
    let wake = Arc::new(Wake(rd));
    Ok((
        Box::new(PtyReader { fd: r, wake: wake.clone() }),
        Box::new(PtyWriter { fd: w, wake }),
        WakeGuard(Mutex::new(Some(wr))),
    ))
}
