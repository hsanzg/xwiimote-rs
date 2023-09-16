# xwiimote

[![Crates.io](https://img.shields.io/crates/v/xwiimote)](https://crates.io/crates/xwiimote)
[![docs.rs](https://img.shields.io/docsrs/xwiimote)](https://docs.rs/xwiimote)
[![Build status](https://github.com/hsanzg/xwiimote/actions/workflows/build.yml/badge.svg)](https://github.com/hsanzg/xwiimote/actions/)

Idiomatic Rust bindings to the [xwiimote](https://github.com/dvdhrm/xwiimote) user-space library.

## Usage

You will need the following dependencies to build and use the library:
- libudev
- libxwiimote (optional; set `XWIIMOTE_SYS_STATIC=1` to build from source and link statically.)

## License

[MIT](LICENSE) &copy; [Hugo Sanz González](https://hgsg.me)
