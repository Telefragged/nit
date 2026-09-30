//! Ends the process when its parent process exits.
//!
//! Claude Code does not stop a hook's process when the session ends, so a
//! watch that a hook starts ends itself when the session's process exits.

use anyhow::{Result, anyhow};
use rustix::io::Errno;
use rustix::process::{Pid, getppid};

/// Exits the process with status 0 once its parent exits.
///
/// # Errors
///
/// When the process has no parent, or when opening the pidfd or the
/// kqueue fails.
pub(crate) fn exit_with_parent() -> Result<()> {
    let parent = getppid().ok_or_else(|| anyhow!("the process has no parent"))?;
    let exited = on_exit(parent)?;
    // The parent can exit before `on_exit` opens its handle. The kernel
    // then gives this process a new parent, and the handle can refer to a
    // process that no longer exists.
    if getppid() != Some(parent) {
        std::process::exit(0);
    }
    std::thread::spawn(move || {
        exited();
        std::process::exit(0);
    });
    Ok(())
}

/// Returns a function that blocks until `pid` exits.
fn on_exit(pid: Pid) -> rustix::io::Result<impl FnOnce() + Send> {
    cfg_select! {
        target_os = "linux" => {
            use rustix::event::{PollFd, PollFlags, poll};
            let fd = rustix::process::pidfd_open(pid, rustix::process::PidfdFlags::empty())?;
            Ok(move || {
                let mut fds = [PollFd::new(&fd, PollFlags::IN)];
                while poll(&mut fds, None) == Err(Errno::INTR) {}
            })
        }
        target_os = "macos" => {
            use rustix::event::kqueue::{Event, EventFilter, EventFlags, ProcessEvents, kevent, kqueue};
            let queue = kqueue()?;
            let filter = EventFilter::Proc {
                pid,
                flags: ProcessEvents::EXIT,
            };
            let exit = Event::new(filter, EventFlags::ADD, std::ptr::null_mut());
            // SAFETY: the only event in the queue is for a process, not for a
            // file descriptor, so no descriptor has to outlive the queue.
            unsafe { kevent(&queue, &[exit], &mut Vec::new(), None) }?;
            Ok(move || {
                let mut events = Vec::with_capacity(1);
                // SAFETY: as above, the queue contains only the process event.
                while unsafe { kevent(&queue, &[], &mut events, None) } == Err(Errno::INTR) {}
            })
        }
    }
}
