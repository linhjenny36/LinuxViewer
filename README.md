# uvview: UltraViewer client for Linux (Rust)

Shows a Windows PC's screen over UltraViewer, lets you control it with the mouse
and keyboard, and can play the PC's sound. It's a Rust port of parts of MacViewer's
`Sources/UltraViewerProtocol`: registration, partner lookup, login, the TLS relay
transport, the zstd/RGB555/JPEG screen decoder, mouse and keyboard messages, and
G.722 computer audio.

It isn't an official UltraViewer product (same disclaimer as MacViewer). Only
connect to computers you're allowed to access. The Windows user sees every
session and can switch it to view-only or end it.

## Build

You need Rust 1.82 or later and a C compiler: the zstd library and the G.722
decoder are built from source. You don't need X11 or Wayland development
packages, because the window libraries load them when the program starts.

```sh
cargo test
cargo build --release   # target/release/uvview
```

## Use

```sh
./uvview --id 123456789                  # asks for the password, opens a window
./uvview --id 123456789 --sound          # also plays the Windows computer's audio
UV_PASSWORD=1234 ./uvview --id 123456789 --snapshot screen.png   # save one frame, no window
```

- **Mouse:** move, left/right/middle click, drag and the scroll wheel are sent
  to Windows while the pointer is over the window. If the Windows user switches
  the session to view-only, input stops and the window title says so.
- **Keyboard:** keys are sent as physical US-layout keys while the window has
  focus. To type Vietnamese, turn on the input method on the Windows PC (for
  example Unikey); it composes the text there. Needs Windows UltraViewer 6.2.13
  or later. Shortcuts your Linux desktop grabs first (such as Super or Alt+Tab)
  stay on Linux.
- **Sound:** `--sound` plays the audio through `pacat` (PulseAudio or PipeWire),
  `pw-play` or `aplay`, whichever is installed first.
- The window scales the remote screen to fit, and its title shows the connection
  status. When you close the window, uvview tells Windows the session ended.
- The install identity is stored in `~/.config/macviewer-linux/registration.json`.

## Limits

- US keyboard layout only. CapsLock is assumed off when you connect; press it
  again if letters come out in the wrong case.
- No chat, clipboard or file transfer.
- Relay only: MacViewer's direct LAN connection is not ported.
- Windows UltraViewer 6.0.0 or later, 16-bit colour mode (as MacViewer).
- Passwords must be ASCII (as MacViewer).
- Login reports a fixed 1920x1080 viewer screen.
- Scaling is nearest-neighbour on the CPU.

## Third-party files

- `assets/8bit.dat`: UltraViewer's screen dictionary, copied from MacViewer.
- `third_party/g722`: the WebRTC G.722 decoder (public-domain SpanDSP code
  with a BSD-licensed WebRTC header; see the licence files there), plus
  MacViewer's output-compatibility shim.
- `tests/fixtures/g722-windows.json`: MacViewer's Windows decoder fixture.
