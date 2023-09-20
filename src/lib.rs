//! This library provides a safe Rust interface to the [`xwiimote`][xwiimote]
//! userspace library.
//!
//! # Examples
//! Connect to the first Wii Remote found and print its battery level.
//! ```
//! use xwiimote::{Device, Monitor};
//! use futures_util::TryStreamExt;
//!
//! # tokio_test::block_on(async {
//! // A monitor enumerates the addresses of all connected Wii Remotes.
//! let mut monitor = Monitor::enumerate()?;
//! match monitor.try_next().await {
//!     Ok(Some(address)) => {
//!         // Connect to the Wii Remote specified by `address`.
//!         let device = Device::connect(&address)?;
//!         let level = device.battery()?;
//!         println!("the battery level is {}%", level);
//!     }
//!     Ok(None) => println!("found no connected device"),
//!     Err(e) => eprintln!("could not enumerate devices: {e}"),
//! };
//! # });
//! ```
//!
//! Print device addresses as new Wii Remotes are discovered.
//! ```
//! use xwiimote::{Device, Monitor};
//! use futures_util::TryStreamExt;
//!
//! # tokio_test::block_on(async {
//! let mut monitor = Monitor::discover()?;
//! while let Ok(Some(address)) = monitor.try_next().await {
//!     println!("found device at {address}");
//! }
//! # });
//!
//! ```
//!
//! [xwiimote]: https://github.com/xwiimote/xwiimote

use crate::events::{Event, EventStream};
use crate::reactor::{Interest, Reactor};
use bitflags::bitflags;
use futures_core::Stream;
use libc::{c_int, c_uint};
use num_derive::FromPrimitive;
use std::ffi::{CStr, CString, OsStr};
use std::os::fd::RawFd;
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;
use std::pin::Pin;
use std::ptr;
use std::task::{Context, Poll};
use std::time::{Duration, SystemTime};
use xwiimote_sys::{
    xwii_iface, xwii_iface_available, xwii_iface_close, xwii_iface_get_battery,
    xwii_iface_get_devtype, xwii_iface_get_extension, xwii_iface_get_led,
    xwii_iface_get_mp_normalization, xwii_iface_new, xwii_iface_open, xwii_iface_opened,
    xwii_iface_rumble, xwii_iface_set_led, xwii_iface_set_mp_normalization, xwii_iface_unref,
    xwii_iface_watch, xwii_monitor, xwii_monitor_get_fd, xwii_monitor_new, xwii_monitor_poll,
    xwii_monitor_unref, XWII_IFACE_WRITABLE,
};

pub mod events;
pub(crate) mod reactor;

// FFI and libc utilities.

/// Returns an error representing the last OS error which occurred,
/// if the given expression is `true`.
macro_rules! bail_if {
    ($e:expr) => {
        if $e {
            return Err(std::io::Error::last_os_error());
        }
    };
}

// Expose macro to all modules within crate.
pub(crate) use bail_if;

/// Deallocates a string which was created by the `xwiimote` library.
///
/// # Safety
/// `str` must point to valid memory allocated by the `xwiimote` library.
unsafe fn free_str(str: *const libc::c_char) {
    libc::free(str as *mut libc::c_void);
}

/// Converts a C string into a Rust [`String`].
fn to_rust_str(str: &CStr) -> String {
    str.to_string_lossy().into_owned()
}

/// The main result type used by this crate.
pub(crate) type Result<T> = std::io::Result<T>;

/// A Wii Remote device address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Address(PathBuf);

impl Address {
    /// Converts the path given as a C string into a device address.
    fn from_raw(path_str: &CStr) -> Self {
        let path_str = OsStr::from_bytes(path_str.to_bytes()).to_os_string();
        Self(PathBuf::from(path_str))
    }

    fn to_c_string(&self) -> CString {
        let slice = self.0.as_os_str().as_bytes();
        CString::new(slice).expect("path contains an internal null byte")
    }
}

impl From<PathBuf> for Address {
    /// Wraps the path to a Wii Remote HID device (typically under
    /// the `/sys/bus/hid/devices` directory) in an [`Address`].
    fn from(path: PathBuf) -> Self {
        Self(path)
    }
}

// Device monitoring (enumeration and discovery).

/// Enumerates the addresses of connected Wii Remotes and optionally streams
/// device addresses as new devices are discovered. The same address may
/// be produced multiple times.
///
/// When discovery mode is disabled, the stream returns [`None`]
/// once the addresses of all connected devices have been produced.
///
/// A monitor should be dropped when no longer needed in order to avoid
/// needlessly polling the system for new devices.
pub struct Monitor {
    handle: *mut xwii_monitor,
    /// The file descriptor used by the monitor referenced by `handle`.
    /// Only present in discovery mode in order to monitor for hot-plug events.
    mon_fd: Option<RawFd>,
    /// Have we produced all the connected devices already?
    enumerated: bool,
}

impl Monitor {
    const HOTPLUG_EVENTS: c_int = libc::EPOLLIN | libc::EPOLLHUP | libc::EPOLLPRI;

    fn new(discover: bool) -> Result<Self> {
        // Create a monitor based on udevd events.
        let handle = unsafe { xwii_monitor_new(discover, false) };
        bail_if!(handle.is_null());

        Ok(Self {
            handle,
            mon_fd: discover.then(|| unsafe { xwii_monitor_get_fd(handle, false) }),
            enumerated: false,
        })
    }

    /// Creates a monitor that streams the addresses of all connected devices.
    pub fn enumerate() -> Result<Self> {
        Self::new(false)
    }

    /// Creates a monitor that first streams the addresses of all connected
    /// devices and then listens for hot-plugged devices, producing
    /// their addresses as they are found.
    pub fn discover() -> Result<Self> {
        Self::new(true)
    }
}

impl Stream for Monitor {
    type Item = Result<Address>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let raw_path = if self.enumerated {
            // At this point every connected device has already been produced.
            // If `self.mon_fd` is present, we should now discover new devices.
            // Otherwise the enumeration process is complete.
            let mon_fd = match self.mon_fd {
                Some(fd) => fd,
                None => return Poll::Ready(None),
            };

            let raw_path = unsafe { xwii_monitor_poll(self.handle) };
            if raw_path.is_null() {
                // No new device is available; arrange for `wake` to be called
                // once a new device is found.
                let interest = Interest::new(mon_fd, Self::HOTPLUG_EVENTS);
                Reactor::get().set_callback(interest, cx.waker().clone());
                return Poll::Pending;
            }
            raw_path
        } else {
            // Enumerate the next connected device, if any.
            // This process requires no blocking; read directly.
            let raw_path = unsafe { xwii_monitor_poll(self.handle) };
            if raw_path.is_null() {
                // We just read the first `null` device address;
                // the enumeration phase is complete.
                self.enumerated = true;
                return if let Some(mon_fd) = self.mon_fd {
                    // Listen for hot-plug events on the monitor descriptor.
                    let interest = Interest::new(mon_fd, Self::HOTPLUG_EVENTS);
                    Reactor::get().add_interest(&interest)?;
                    // Poll again to return the first discovered device.
                    self.poll_next(cx)
                } else {
                    Poll::Ready(None)
                };
            }
            raw_path
        };

        // Convert the raw path into an address and free the original string.
        let slice = unsafe { CStr::from_ptr(raw_path) };
        let address = Address::from_raw(slice);
        unsafe { free_str(raw_path) };
        Poll::Ready(Some(Ok(address)))
    }
}

impl Drop for Monitor {
    fn drop(&mut self) {
        if let Some(mon_fd) = self.mon_fd {
            let interest = Interest::new(mon_fd, Self::HOTPLUG_EVENTS);
            Reactor::get()
                .remove_interest(&interest)
                .expect("failed to remove interest for monitor fd");
        }
        // Decrements ref-count to zero. This closes `self.fd`, if set.
        unsafe { xwii_monitor_unref(self.handle) };
    }
}

// Device and interfaces

bitflags! {
    /// Represents the channels that can be opened on a [`Device`].
    ///
    /// The `xwiimote` library uses the term "interface" to refer
    /// to this concept.
    pub struct Channels: c_uint {
        // todo: improve docs
        /// Primary channel.
        const CORE = xwiimote_sys::XWII_IFACE_CORE;
        /// Accelerometer channel.
        const ACCELEROMETER = xwiimote_sys::XWII_IFACE_ACCEL;
        /// IR camera channel.
        const IR = xwiimote_sys::XWII_IFACE_IR;
        /// MotionPlus extension channel.
        const MOTION_PLUS = xwiimote_sys::XWII_IFACE_MOTION_PLUS;
        /// Nunchuk extension channel.
        const NUNCHUK = xwiimote_sys::XWII_IFACE_NUNCHUK;
        /// Classic controller channel.
        const CLASSIC_CONTROLLER = xwiimote_sys::XWII_IFACE_CLASSIC_CONTROLLER;
        /// Balance board channel.
        const BALANCE_BOARD = xwiimote_sys::XWII_IFACE_PRO_CONTROLLER;
        /// ProController channel.
        const PRO_CONTROLLER = xwiimote_sys::XWII_IFACE_DRUMS;
        /// Drums channel.
        const DRUMS = xwiimote_sys::XWII_IFACE_DRUMS;
        /// Guitar channel.
        const GUITAR = xwiimote_sys::XWII_IFACE_GUITAR;
    }
}

/// Motion Plus sensor normalization and calibration values.
///
/// The absolute offsets are subtracted from any Motion Plus
/// sensor data before they are returned in an event.
#[derive(Copy, Clone, Eq, PartialEq, Default, Debug)]
pub struct MotionPlusNormalization {
    /// Absolute x-axis offset.
    pub x: i32,
    /// Absolute y-axis offset.
    pub y: i32,
    /// Absolute z-axis offset
    pub z: i32,
    /// Calibration factor used to establish the zero-point of
    /// the Motion Plus sensor data depending on its output.
    pub factor: i32,
}

/// The Wii Remote LED lights.
#[repr(u32)]
#[derive(Copy, Clone, Debug, FromPrimitive)]
pub enum Led {
    /// The left-most light.
    One = xwiimote_sys::XWII_LED1,
    /// The mid-left light.
    Two = xwiimote_sys::XWII_LED2,
    /// The mid-right light.
    Three = xwiimote_sys::XWII_LED3,
    /// The right-most light.
    Four = xwiimote_sys::XWII_LED4,
}

/// A connected Wii Remote.
pub struct Device {
    handle: *mut xwii_iface,
    /// Is the core channel open in writable mode?
    ///
    /// Operations like toggling the rumble motor require this channel
    /// to be open in order to function.
    core_open: bool,
}

impl Device {
    /// Connects to the Wii Remote specified by `address`.
    pub fn connect(address: &Address) -> Result<Self> {
        let path = address.to_c_string();

        // Opening a device file immediately after being discovered results
        // in a "Transport is not connected" error. This delays the operation,
        // but it isn't ideal (since the delay is arbitrary).
        std::thread::sleep(Duration::from_millis(100));

        let mut handle = ptr::null_mut();
        let res_code = unsafe { xwii_iface_new(&mut handle, path.as_ptr()) };
        bail_if!(res_code != 0);

        // Watch the device for hot-plug events. Otherwise the `xwii_iface_dispatch`
        // function does not report events of type `XWII_EVENT_GONE`,
        // which we need in order to tell the reactor to remove interest
        // from the device file.
        let res_code = unsafe { xwii_iface_watch(handle, true) };
        bail_if!(res_code != 0);

        Ok(Self {
            handle,
            core_open: false,
        })
    }

    // Channels.

    /// Opens the given channels for communication.
    ///
    /// If a given channel is already open, it is ignored. If any channel
    /// fails to open, the function still tries to open the remaining
    /// requested channels and then returns the error.
    ///
    /// A channel may be closed automatically if an extension is unplugged
    /// or on error conditions.
    pub fn open(&mut self, channels: Channels, writable: bool) -> Result<()> {
        let mut ifaces = channels.bits();
        if writable {
            ifaces |= XWII_IFACE_WRITABLE;
        }
        let res_code = unsafe { xwii_iface_open(self.handle, ifaces) };
        bail_if!(res_code != 0);

        if channels.contains(Channels::CORE) && writable {
            self.core_open = true;
        }
        Ok(())
    }

    /// Open the [core channel](`Channels::CORE`) in writable mode,
    /// if not already open.
    fn ensure_core_open(&mut self) -> Result<()> {
        if !self.core_open {
            self.open(Channels::CORE, true)?
        }
        Ok(())
    }

    /// Closes the given channels.
    ///
    /// If a channel is already closed, it is ignored.
    pub fn close(&mut self, channels: Channels) -> Result<()> {
        if channels.contains(Channels::CORE) {
            self.core_open = false;
        }
        unsafe { xwii_iface_close(self.handle, channels.bits()) };
        Ok(())
    }

    /// Lists the currently open channels.
    pub fn get_open(&self) -> Channels {
        Channels::from_bits(unsafe { xwii_iface_opened(self.handle) }).unwrap()
    }

    /// Lists the channels that can be opened, including those
    /// that are already open.
    ///
    /// A channel can become available as a result of an extension being plugged
    /// to the device. Dually, it becomes unavailable when the extension
    /// is disconnected.
    pub fn available(&self) -> Channels {
        Channels::from_bits(unsafe { xwii_iface_available(self.handle) }).unwrap()
    }

    // Events.

    /// Returns an stream that produces events received from the device,
    /// including the time at which the kernel generated them.
    ///
    /// Most event types are received only if the appropriate channels
    /// are open. See [`Event`] for details.
    pub fn events(&self) -> Result<impl Stream<Item = Result<(Event, SystemTime)>> + '_> {
        EventStream::new(self)
    }

    // Out-of-band actions (which don't require any channel open to work).

    /// Reads the current state of an LED light.
    pub fn led(&self, light: Led) -> Result<bool> {
        let mut enabled = false;
        let res_code = unsafe { xwii_iface_get_led(self.handle, light as c_uint, &mut enabled) };
        bail_if!(res_code != 0);
        Ok(enabled)
    }

    /// Changes the state of an LED light.
    pub fn set_led(&mut self, light: Led, enabled: bool) -> Result<()> {
        let res_code = unsafe { xwii_iface_set_led(self.handle, light as c_uint, enabled) };
        bail_if!(res_code != 0);
        Ok(())
    }

    /// Reads the current battery level.
    ///
    /// # Returns
    /// The battery level as a percentage from 0 to 100%, where 100%
    /// means the battery is fully charged.
    pub fn battery(&self) -> Result<u8> {
        let mut level = 0;
        let res_code = unsafe { xwii_iface_get_battery(self.handle, &mut level) };
        bail_if!(res_code != 0);
        Ok(level)
    }

    /// Returns the device type identifier.
    pub fn kind(&self) -> Result<String> {
        let mut raw_kind = ptr::null_mut();
        let res_code = unsafe { xwii_iface_get_devtype(self.handle, &mut raw_kind) };
        bail_if!(res_code != 0);

        let kind = to_rust_str(unsafe { CStr::from_ptr(raw_kind) });
        unsafe { free_str(raw_kind) };
        Ok(kind)
    }

    /// Returns the current extension type identifier.
    pub fn extension(&self) -> Result<String> {
        let mut raw_ext_kind = ptr::null_mut();
        let res_code = unsafe { xwii_iface_get_extension(self.handle, &mut raw_ext_kind) };
        bail_if!(res_code != 0);

        let ext_kind = to_rust_str(unsafe { CStr::from_ptr(raw_ext_kind) });
        unsafe { free_str(raw_ext_kind) };
        Ok(ext_kind)
    }

    /// Toggles the rumble motor.
    ///
    /// If the [core channel][core] is closed, it is opened in writable mode.
    ///
    /// [core]: `Channels::CORE`
    pub fn set_rumble(&mut self, enabled: bool) -> Result<()> {
        self.ensure_core_open()?;
        let res_code = unsafe { xwii_iface_rumble(self.handle, enabled) };
        bail_if!(res_code != 0); // the channel might have been closed by the kernel
        Ok(())
    }

    // Motion Plus sensor normalization

    /// Reads the Motion Plus sensor normalization values.
    pub fn mp_normalization(&self) -> Result<MotionPlusNormalization> {
        let mut values = MotionPlusNormalization::default();
        unsafe {
            xwii_iface_get_mp_normalization(
                self.handle,
                &mut values.x,
                &mut values.y,
                &mut values.z,
                &mut values.factor,
            )
        };
        Ok(values)
    }

    /// Updates the Motion Plus sensor normalization values.
    pub fn set_mp_normalization(&mut self, values: &MotionPlusNormalization) -> Result<()> {
        unsafe {
            xwii_iface_set_mp_normalization(
                self.handle,
                values.x,
                values.y,
                values.z,
                values.factor,
            )
        };
        Ok(())
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        // Decrements ref-count to zero. This destroys the device.
        unsafe { xwii_iface_unref(self.handle) };
    }
}
