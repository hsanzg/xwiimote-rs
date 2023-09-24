# wiinote

[![Build status](https://github.com/hsanzg/xwiimote-rs/actions/workflows/build.yml/badge.svg)](https://github.com/hsanzg/xwiimote-rs/actions/)

Use a [Wii Remote](https://en.wikipedia.org/wiki/Wii_Remote) as a slide clicker.

This application also serves as an example for using the [Rust bindings](https://crates.io/crates/xwiimote)
to the [xwiimote](https://github.com/dvdhrm/xwiimote) user-space library.

## Usage

You will need the following dependencies to build and use wiinote:
- Rust >= 1.61
- libudev >= 183
- libxwiimote >= 2-2 (optional; set `XWIIMOTE_SYS_STATIC=1` to build from source and link statically.)
- libdbus-1-dev >= 1.12.20

## Setup

Enable the `uinput` kernel module to create a virtual keyboard device,
and emit key events corresponding to button presses on a Wii Remote:
```bash
modprobe uinput
```

Enable the Wii Remote kernel driver, which is used to communicate
with a Wii Remote and its extensions (such as a Nunchuck):
```bash
modprobe hid-wiimote
```

Pair and connect to a Wii Remote as with any other Bluetooth device;
see the [ArchWiki article](https://wiki.archlinux.org/title/XWiimote#Connect_the_Wii_Remote)
for details.

Allow the current user to access the `/dev/uinput` device file:
```bash
groupadd -f uinput
gpasswd -a $USER uinput
cat >/etc/udev/rules.d/40-input.rules <<EOL
KERNEL=="uinput", SUBSYSTEM=="misc", GROUP="uinput", MODE="0660"
EOL
```

Reload the `udev` rules:
```bash
udevadm control --reload-rules && udevadm trigger
```

Finally, run the application with `./wiinote`.
By default the program exits if no connected Wii Remote is found,
but this behavior can be changed via the `--discover` flag.
The output of `./wiinote --help` contains further information
on automatic device discovery.

## License

[MIT](LICENSE) &copy; [Hugo Sanz González](https://hgsg.me)
