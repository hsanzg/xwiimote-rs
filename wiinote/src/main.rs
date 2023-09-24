use crate::keyboard::{to_io_err, Keyboard};
use clap::Parser;
use futures_util::TryStreamExt;
use num_traits::cast::FromPrimitive;
use std::path::PathBuf;
use std::time::Duration;
use tokio::time::MissedTickBehavior;
use xwiimote::events::{Event, Key};
use xwiimote::{Address, Channels, Device, Led, Monitor, Result};

mod keyboard;

#[derive(Debug, Parser)]
#[command(version, author, about, long_about = None)]
struct Args {
    /// Search and connect to a Wii Remote placed in discoverable mode
    /// after failing to locate an already plugged-in Wii Remote.
    ///
    /// If the connection to the device is dropped, the program restarts
    /// the discovery session until a new Wii Remote is found.
    ///
    /// When not set, the program exits if no plugged-in Wii Remote
    /// is found.
    #[arg(short, long)]
    discover: bool,
    /// Connect to the Wii Remote identified by a `sysfs` device directory,
    /// which is typically of the form `/sys/bus/hid/devices/[dev]`.
    ///
    /// If not present, connect to the first Wii Remote found;
    /// see the `--discover` option for details.
    #[arg(value_hint = clap::ValueHint::DirPath, value_parser = parse_address)]
    address: Option<Address>,
}

/// Converts a path into a device address.
fn parse_address(input: &str) -> Result<Address> {
    Ok(Address::from(PathBuf::from(input)))
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args = Args::parse();
    let mut keyboard = Keyboard::new().await.map_err(to_io_err)?;
    if let Some(address) = args.address {
        // Connect to the device specified by the given address.
        connect(&address, &mut keyboard).await?;
    } else {
        // Enumerate devices and connect to the first one found.
        while let Some(address) = find_device(args.discover).await? {
            connect(&address, &mut keyboard).await?;
            // The previous device has disconnected gracefully; restart
            // the enumeration process to find a new device address.
        }
        // A device monitor produces `None` only if discovery mode
        // is disabled, and consequently so does `find_device`.
        eprintln!("No connected devices found");
    }
    Ok(())
}

/// Finds the address of a connected device.
///
/// If `discover` is true and no device is found, blocks until
/// a new device is hot-plugged. Otherwise returns `Ok(None)`.
async fn find_device(discover: bool) -> Result<Option<Address>> {
    let mut monitor = if discover {
        println!("Discovering devices");
        Monitor::discover()
    } else {
        println!("Enumerating connected devices");
        Monitor::enumerate()
    }?;
    monitor.try_next().await
}

/// Initiates the connection to the device specified by `address`.
///
/// # Returns
/// On success, the function blocks until the device is disconnected gracefully,
/// returning `Ok(())`. Otherwise an error is raised.
async fn connect(address: &Address, keyboard: &mut Keyboard) -> Result<()> {
    let mut device = Device::connect(address)?;
    let name = device.kind()?;

    device.open(Channels::CORE, true)?;
    println!("Device connected: {name}");

    handle(&mut device, keyboard).await?;
    println!("Device disconnected: {name}");
    Ok(())
}

/// The metrics that can be displayed in a [`LightsDisplay`].
#[derive(Debug, Copy, Clone)]
enum LightsMetric {
    /// Display the battery level.
    Battery,
    /// Display the connection strength level.
    Connection,
}

/// The set of lights in a Wii Remote, used as a display.
struct LightsDisplay<'d> {
    /// The device whose lights are being controlled.
    device: &'d Device,
    /// The metric to display.
    metric: LightsMetric,
    /// An interval that ticks whenever the display needs to be updated.
    interval: tokio::time::Interval,
}

impl<'d> LightsDisplay<'d> {
    /// Creates a wrapper for the display of a Wii Remote.
    pub fn new(device: &'d Device) -> Self {
        let mut interval = tokio::time::interval(Duration::from_secs(20));
        interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
        Self {
            device,
            // The connection strength is probably high immediately
            // after pairing; display the battery level by default.
            metric: LightsMetric::Battery,
            interval,
        }
    }

    /// Completes when the device display should be updated.
    pub async fn tick(&mut self) -> tokio::time::Instant {
        self.interval.tick().await
    }

    /// Updates the device lights according to the current metric.
    pub async fn update(&self) -> Result<()> {
        let level = match self.metric {
            LightsMetric::Battery => self.device.battery()?,
            LightsMetric::Connection => {
                // Technically RSSI is a measure of the received intensity
                // rather than connection quality. This is good enough for
                // the Wii Remote. The scale goes from -80 to 0, where 0
                // represents the greatest signal strength.
                let rssi = 0i8; // todo
                !((rssi as i16 * 100 / -80) as u8)
            }
        };

        // `level` is a value from 0 to 100 (inclusive).
        let last_ix = 1 + level / 30; // 1..=4
        for ix in 1..=4 {
            let light = Led::from_u8(ix).unwrap();
            self.device.set_led(light, ix <= last_ix)?;
        }
        Ok(())
    }

    /// Updates the displayed metric.
    pub async fn set_metric(&mut self, metric: LightsMetric) -> Result<()> {
        self.metric = metric;
        self.update().await
    }
}

/// Processes the connection to a Wii Remote.
///
/// # Returns
/// If the device is disconnected gracefully, returns `Ok(())`.
/// Otherwise an error is raised.
async fn handle(device: &mut Device, keyboard: &mut Keyboard) -> Result<()> {
    let mut event_stream = device.events()?;
    let mut display = LightsDisplay::new(device);

    loop {
        // Wait for the next event, which is either an event
        // emitted by the device or a display update request.
        let maybe_event = tokio::select! {
            res = event_stream.try_next() => res?,
            _ = display.tick() => {
                display.update().await?;
                continue;
            }
        };

        let (event, _time) = match maybe_event {
            Some(event) => event,
            None => return Ok(()), // connection closed
        };

        if let Event::Key(key, state) = event {
            match key {
                Key::One => display.set_metric(LightsMetric::Battery).await,
                Key::Two => display.set_metric(LightsMetric::Connection).await,
                // If the remote key is mapped to a regular keyboard key,
                // send a press or release event via the `uinput` API.
                _ => keyboard.update(&key, &state).await.map_err(to_io_err),
            }?;
        }
    }
}
