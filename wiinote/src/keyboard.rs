use std::error::Error;
use std::io;
use std::io::ErrorKind;
use uinput_tokio::event;
use uinput_tokio::event::keyboard;
use xwiimote::events::{Key, KeyState};

/// A result that may contain a `uinput` error value.
type UInputResult<T> = std::result::Result<T, Box<dyn Error>>;

/// The virtual device name to use for all events
/// originating from this application.
static DEV_NAME: &str = "Wiinote";

/// A virtual keyboard device.
pub struct Keyboard(uinput_tokio::Device);

impl Keyboard {
    /// Creates a new virtual keyboard device.
    pub async fn new() -> UInputResult<Self> {
        // Register certain keys for sending press and release events.
        let events = [
            event::Keyboard::Key(keyboard::Key::Up),
            event::Keyboard::Key(keyboard::Key::Down),
            event::Keyboard::Key(keyboard::Key::Left),
            event::Keyboard::Key(keyboard::Key::Right),
            event::Keyboard::Key(keyboard::Key::Enter),
            event::Keyboard::Misc(keyboard::Misc::VolumeUp),
            event::Keyboard::Key(keyboard::Key::Esc),
            event::Keyboard::Misc(keyboard::Misc::VolumeDown),
        ];
        let mut builder = uinput_tokio::default()?.name(DEV_NAME)?;
        for event in events {
            builder = builder.event(event)?;
        }
        builder.create().await.map(Self)
    }

    /// Presses or releases the key mapped to `button`, if any.
    /// Otherwise does nothing.
    pub async fn update(&mut self, button: &Key, state: &KeyState) -> UInputResult<()> {
        if let Some(key) = key_event(button) {
            match *state {
                KeyState::Down => self.0.press(&key).await?,
                KeyState::Up => self.0.release(&key).await?,
                KeyState::AutoRepeat => {} // leave the key pressed.
            };
            self.0.synchronize().await
        } else {
            // The button is not matched to any key, ignore.
            Ok(())
        }
    }
}

/// Converts a Wii Remote key into a keyboard event.
pub fn key_event(key: &Key) -> Option<event::Keyboard> {
    Some(match *key {
        Key::Up => event::Keyboard::Key(keyboard::Key::Up),
        Key::Down => event::Keyboard::Key(keyboard::Key::Down),
        Key::Left => event::Keyboard::Key(keyboard::Key::Left),
        Key::Right => event::Keyboard::Key(keyboard::Key::Right),
        Key::A => event::Keyboard::Key(keyboard::Key::Enter),
        Key::B => event::Keyboard::Key(keyboard::Key::Left),
        Key::Plus => event::Keyboard::Misc(keyboard::Misc::VolumeUp),
        Key::Home => event::Keyboard::Key(keyboard::Key::Esc),
        Key::Minus => event::Keyboard::Misc(keyboard::Misc::VolumeDown),
        _ => return None,
    })
}

/// Converts a boxed `uinput` error into an I/O error.
pub fn to_io_err(err: Box<dyn Error>) -> io::Error {
    // todo: the `uinput_tokio` crate doesn't specify the `Sized` trait
    //       for errors, so we cannot convert the error directly into
    //       an I/O error. See if we can retain the source information
    //       in some other way.
    io::Error::new(ErrorKind::Other, err.to_string())
}
