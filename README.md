# RMox

A family of Rust crates for interfacing with the reMarkable 2.

Currently includes crates for drawing to the display and getting input from the included peripherals (including the Type Folio).

Currently under active development. The API is not finalized.

## Usage

There is currently a `testing` crate which is used for testing whatever I am currently working on.
In order to run that, I suggest the following workflow:

1. Install a launcher capable of running `.draft` files.
2. Create a `.draft` file for a dummy application that just runs `sleep inf`:

```ini
name=dummy
desc=Do nothing
call=sleep inf
```

3. Run that `.draft` file from your launcher.
4. Use the following command to build, copy, and run the binary: `cross build --release --target armv7-unknown-linux-gnueabihf && rsync -vzh $TARGET_DIR/armv7-unknown-linux-gnueabihf/release/testing root@10.11.99.1:/home/root/tempbin && ssh root@10.11.99.1 /home/root/tempbin`.

## License

AGPL-3.0-or-later
