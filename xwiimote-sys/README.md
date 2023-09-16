# xwiimote-sys

[![Crates.io](https://img.shields.io/crates/v/xwiimote-sys)](https://crates.io/crates/xwiimote-sys)
[![docs.rs](https://img.shields.io/docsrs/xwiimote-sys)](https://docs.rs/xwiimote-sys)

Rust FFI bindings to the [xwiimote](https://github.com/dvdhrm/xwiimote) user-space library.

The [xwiimote](https://crates.io/crates/xwiimote) crate provides higher-level,
more idiomatic bindings to the same library.

## Usage

You will need the following dependencies to create the bindings:
- libudev
- libxwiimote (optional; set `XWIIMOTE_SYS_STATIC=1` to build from source and link statically.)

## License

[MIT](LICENSE) &copy; [Hugo Sanz González](https://hgsg.me)
