# xwiimote

[![Crates.io](https://img.shields.io/crates/v/xwiimote)](https://crates.io/crates/xwiimote)
[![docs.rs](https://img.shields.io/docsrs/xwiimote)](https://docs.rs/xwiimote)
[![Build status](actions/workflows/build.yml/badge.svg)](actions/)

Idiomatic Rust bindings to the [xwiimote](https://github.com/dvdhrm/xwiimote) user-space library.

## Usage

You will need the following dependencies to build and use the library:
- libudev >= 183
- libxwiimote >= 2-2 (optional; set `XWIIMOTE_SYS_STATIC=1` to build from source and link statically.)

The [wiinote](wiinote) application showcases the functionality provided by this library.

## License

[MIT](LICENSE) &copy; [Hugo Sanz González](https://hgsg.me)
