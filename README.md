# uvview: view-only UltraViewer client for Linux (Rust)

Shows a Windows PC's screen over UltraViewer. **View only**: no mouse, keyboard,
chat, clipboard or audio. It is a Rust port of the screen path of MacViewer's
`Sources/UltraViewerProtocol`: registration, partner lookup, login, the TLS relay
transport and the zstd/RGB555/JPEG screen decoder.

It is not an official UltraViewer product (same disclaimer as MacViewer). Only
connect to computers you're allowed to access.

## Build

You need Rust 1.82 or later and a C compiler (the zstd library is built from source).
No X11 or Wayland development packages are needed: the window libraries load
them when the program starts.

```sh
cd linux-viewer-rs
cargo test
cargo build --release   # target/release/uvview
```

## Use

```sh
./uvview --id 123456789                       # asks for the password, opens a window
UV_PASSWORD=1234 ./uvview --id 123456789 --snapshot screen.png   # save one frame, no window
```

The window scales the remote screen to fit, and its title shows the connection
status. When you close the window, uvview tells Windows the session ended. The
install identity is stored in `~/.config/macviewer-linux/registration.json`. The
Go viewer uses the same file.

## Limits

- Relay only: MacViewer's direct LAN connection is not ported.
- Windows UltraViewer 6.0.0 or later, 16-bit colour mode (as MacViewer).
- Passwords must be ASCII (as MacViewer).
- Login reports a fixed 1920x1080 viewer screen.
- Scaling is nearest-neighbour on the CPU.
