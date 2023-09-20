use crate::{bail_if, Result};
use libc::epoll_event;
use libc::{c_int, c_uint};
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::hash::Hash;
use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd, OwnedFd, RawFd};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::Waker;
use std::thread;

/// Describes the events a task wants to be notified of.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct Interest {
    /// The source of events.
    fd: RawFd,
    /// A bit field containing the types of the relevant events;
    /// see [`libc::EPOLLIN`], [`libc::EPOLLHUP`], etc.
    events: c_int,
}

impl Interest {
    /// Creates a new interest description.
    pub fn new<F: IntoRawFd>(fd: F, events: c_int) -> Self {
        Self {
            fd: fd.into_raw_fd(),
            events,
        }
    }
}

impl From<&Interest> for epoll_event {
    fn from(interest: &Interest) -> Self {
        epoll_event {
            // Enable edge-triggered mechanism, since the interested task
            // is expected to read all available data from `fd`.
            events: (interest.events | libc::EPOLLET) as c_uint,
            u64: interest.fd.try_into().unwrap(), // `fd` is valid
        }
    }
}

impl From<&epoll_event> for Interest {
    fn from(event: &epoll_event) -> Self {
        Self {
            fd: event.u64.try_into().unwrap(),
            events: event.events.try_into().unwrap(),
        }
    }
}

/// A buffer of readiness events polled from an epoll descriptor.
type Events = Vec<epoll_event>;

/// An event loop that blocks on asynchronous IO events and
/// notifies interested tasks of their occurrence.
pub struct Reactor {
    /// The epoll file descriptor.
    ep_fd: OwnedFd,
    /// The handles for waking up the interested tasks.
    wakers: Mutex<HashMap<Interest, Waker>>,
}

impl Reactor {
    /// Returns a reference to the global event loop.
    pub fn get() -> &'static Self {
        static REACTOR: Lazy<Reactor> = Lazy::new(|| {
            // Start the event loop in a separate thread.
            thread::spawn(|| {
                Reactor::get().run().expect("event loop failed");
            });
            Reactor::new().expect("failed to create global event loop")
        });
        &REACTOR
    }

    /// Creates a new event loop.
    fn new() -> Result<Self> {
        let ep_fd = unsafe { libc::epoll_create1(libc::EPOLL_CLOEXEC) };
        bail_if!(ep_fd == -1);
        Ok(Self {
            ep_fd: unsafe { OwnedFd::from_raw_fd(ep_fd) },
            // todo: pre-allocate the hashmap.
            wakers: Mutex::default(),
        })
    }

    /// Executes the event loop.
    fn run(&self) -> Result<()> {
        let term = Arc::new(AtomicBool::new(false));
        signal_hook::flag::register(libc::SIGTERM, Arc::clone(&term))?;

        // Poll for events until the process is terminated.
        // Reuse the readiness event buffer across `wake_ready` calls.
        let mut events = Events::with_capacity(16);
        while !term.load(Ordering::Relaxed) {
            self.wake_ready(&mut events)?;
        }
        Ok(())
    }

    /// Blocks until one or more events occur, and wakes the tasks
    /// that expressed interest in them.
    fn wake_ready(&self, events: &mut Events) -> Result<()> {
        events.clear();
        let n_ready = unsafe {
            libc::epoll_wait(
                self.ep_fd.as_raw_fd(),
                events.as_mut_ptr(),
                events.capacity() as c_int,
                -1, // todo: set reasonable timeout
            )
        };
        bail_if!(n_ready == -1);

        // SAFETY: `epoll_wait` ensures `n_ready` events are assigned.
        unsafe { events.set_len(n_ready as usize) };

        // Notify all interested tasks.
        let mut wakers = self.wakers.lock().unwrap();
        for event in events.iter() {
            let interest = event.into();
            if let Some(waker) = wakers.remove(&interest) {
                waker.wake();
            }
        }
        Ok(())
    }

    // Interests.

    fn ctl_interest(&self, op: c_int, interest: &Interest) -> Result<()> {
        let fd = interest.fd.as_raw_fd();
        let mut event = interest.into();
        let res_code = unsafe { libc::epoll_ctl(self.ep_fd.as_raw_fd(), op, fd, &mut event) };
        bail_if!(res_code == -1);
        Ok(())
    }

    /// Expresses an interest in a particular kind of event on a file.
    pub(crate) fn add_interest(&self, interest: &Interest) -> Result<()> {
        self.ctl_interest(libc::EPOLL_CTL_ADD, interest)
    }

    /// Removes the interest in a particular kind of event on a file.
    ///
    /// This also wakes the pending future, if set.
    pub(crate) fn remove_interest(&self, interest: &Interest) -> Result<()> {
        self.ctl_interest(libc::EPOLL_CTL_DEL, interest)?;
        if let Some(waker) = self.wakers.lock().unwrap().remove(interest) {
            waker.wake();
        }
        Ok(())
    }

    /// Stores the task waker to be called once an IO event that matches
    /// the given interest description occurs.
    ///
    /// The associated future is expected to read all available data
    /// from `interest.fd` once waken up. Otherwise the event loop
    /// may block indefinitely.
    pub(crate) fn set_callback(&self, interest: Interest, waker: Waker) {
        self.wakers.lock().unwrap().insert(interest, waker);
    }
}

#[cfg(test)]
mod tests {
    use crate::reactor::{Interest, Reactor};
    use crate::{bail_if, Result};
    use libc::c_int;
    use std::fs::File;
    use std::future::Future;
    use std::io::Write;
    use std::os::fd::{AsRawFd, OwnedFd};
    use std::pin::Pin;
    use std::task::{Context, Poll};

    #[test]
    fn double_interest_fails() -> Result<()> {
        let reactor = Reactor::new()?;
        let interest = Interest::new(0, libc::EPOLLIN);
        reactor.add_interest(&interest)?;

        assert!(reactor.add_interest(&interest).is_err());
        Ok(())
    }

    #[test]
    fn event_wakes_task() -> Result<()> {
        // Create a pipe whose read end we will poll on.
        let mut fds: Vec<OwnedFd> = Vec::with_capacity(2);
        let res_code = unsafe { libc::pipe2(fds.as_mut_ptr() as *mut c_int, libc::O_CLOEXEC) };
        bail_if!(res_code != 0);
        unsafe { fds.set_len(2) };

        // Record interest in the read end of the pipe.
        let interest = Interest::new(fds[0].as_raw_fd(), libc::EPOLLIN);
        Reactor::get().add_interest(&interest)?;

        struct ReaderFuture {
            first_try: bool,
            interest: Interest,
            file: File,
        }
        impl Future for ReaderFuture {
            type Output = ();

            fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
                if self.first_try {
                    // Ask the reactor to wake us up for the second try.
                    self.first_try = false;
                    Reactor::get().set_callback(self.interest.clone(), cx.waker().clone());

                    // Write to pipe in order to generate an epoll event.
                    self.file
                        .write_all(b"Hello world!")
                        .expect("failed to write to pipe");

                    Poll::Pending
                } else {
                    // Second try, we're done.
                    Poll::Ready(())
                }
            }
        }

        // Wait for the future to complete after two tries.
        futures_executor::block_on(ReaderFuture {
            first_try: true,
            interest,
            file: File::from(fds.remove(1)),
        });
        Ok(())
    }
}
