# RMox

A family of Rust crates providing various functionality for the reMarkable 2.

Currently includes crates for drawing to the display and getting input from the included peripherals (including the Type Folio), as well as a protocol between a Window Manager and its clients and an implementation of a WM providing that protocol.

Currently under active development. The API is not finalized.

## Usage

There are currently `wm` and `test-app` binaries.
In order to run them, I suggest the following workflow:

1. Install a launcher capable of running `.draft` files.
2. Create a `.draft` file for a dummy application that just runs `sleep inf`:

```ini
name=dummy
desc=Do nothing
call=sleep inf
```

3. Run that `.draft` file from your launcher.
4. To run the WM, use `./run-wm --control-socket /tmp/rmox.sock`.
5. To run the test app, use `RMOX_SOCKET=/tmp/rmox.sock ./run-test-app`.

## License

AGPL-3.0-or-later
