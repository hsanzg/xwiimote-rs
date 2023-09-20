use crate::{bail_if, Result};
use std::collections::HashMap;
use std::os::fd::RawFd;
use std::sync::Mutex;
use std::task::Waker;

/// A collection of tasks that are notified of the occurrence
/// of a particular asynchronous IO event.
///
/// A collection of asynchronous IO events to block on.
///
/// which upon their occurrence notify notifying its executor that it is ready to be run
///
/// interested in certain asynchronous IO events.
pub(crate) struct Reactor {
    ep_fd: RawFd,
    wakers: Mutex<HashMap<RawFd, Waker>>,
}

impl Reactor {
    pub fn new() -> Result<Self> {
        let ep_fd = unsafe { libc::epoll_create1(libc::EPOLL_CLOEXEC) };
        bail_if!(ep_fd == -1);
        Ok(Self {
            ep_fd,
            // todo: pre-allocate the hashmap.
            wakers: Mutex::default(),
        })
    }

    // Interests.

    fn ctl_interest(&self, op: libc::c_int, fd: RawFd, events: libc::c_int) -> Result<()> {
        todo!()
    }

    /// Expresses an interest in a particular kind of event on a file.
    pub fn add_interest(&self, fd: RawFd, events: libc::c_int) -> Result<()> {
        self.ctl_interest(libc::EPOLL_CTL_ADD, fd, events)
    }

    /// Removes the interest in a particular kind of event on a file.
    ///
    /// This also wakes the pending future, if set.
    pub fn remove_interest(&self, fd: RawFd, events: libc::c_int) -> Result<()> {
        self.ctl_interest(libc::EPOLL_CTL_DEL, fd, events)?;
        if let Some(waker) = self.wakers.lock().unwrap().remove(&fd) {
            waker.wake();
        }
        Ok(())
    }
}
