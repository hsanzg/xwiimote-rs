# wiinote

[![Build status](https://github.com/hsanzg/xwiimote/actions/workflows/build.yml/badge.svg)](https://github.com/hsanzg/xwiimote/actions/)

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

todo.

## License

[MIT](LICENSE) &copy; [Hugo Sanz González](https://hgsg.me)
