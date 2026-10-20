# LinuxViewer (uvview): UltraViewer client for Linux

A Linux app that shows a Windows PC's screen over UltraViewer, lets you control
it with the mouse and keyboard, and can play the PC's sound.

It isn't an official UltraViewer product and isn't affiliated with the
UltraViewer developers. Only connect to computers you're allowed to access. The
Windows user sees every session and can switch it to view-only or end it.

## Build

You need Rust 1.82 or later and a C compiler: the zstd library and the G.722
decoder are built from source. You don't need X11, Wayland or GTK development
packages. At run time the app needs OpenGL (Mesa is fine) and an X11 or Wayland
desktop.

```sh
cargo test
cargo build --release   # target/release/uvview
```

To try it from a Mac or another machine with Docker, run `dev/desktop/run.sh` and
open http://localhost:6080/vnc.html?autoconnect=1&resize=remote. That gives you a
Linux desktop in the browser with LinuxViewer already open (no sound output there).

## Use

Run `uvview` (or `uvview --id 123456789` to fill in the ID), enter the Windows
computer's UltraViewer ID and password, and click **Connect to another computer**.

- **Recent computers:** IDs you connected to successfully are listed under the
  Connect card; click one to fill in its details.
- **Remember password:** when ticked, the password of a successful connection is
  saved in the desktop keyring (GNOME Keyring, KWallet, or any Secret Service).
- **Toolbar:** during a session, switch the Windows computer's **Sound** on or
  off, show the screen to **Fit** the window or at **Actual size**, or
  **Disconnect**.
- **Mouse:** move, left/right/middle click, drag and the scroll wheel are sent
  to Windows while the pointer is over the remote screen. If the Windows user
  switches the session to view-only, input stops and the toolbar says so.
- **Keyboard:** keys are sent as physical US-layout keys while the window has
  focus. To type Vietnamese, turn on the input method on the Windows PC (for
  example Unikey); it composes the text there. Needs Windows UltraViewer 6.2.13
  or later. Shortcuts your Linux desktop grabs first (such as Super or Alt+Tab)
  stay on Linux.
- **Sound** plays through `pacat` (PulseAudio or PipeWire), `pw-play` or
  `aplay`, whichever is installed first.
- `UV_PASSWORD=1234 uvview --id 123456789 --snapshot screen.png` connects without
  a window, saves the first screen and exits.
- The install identity is stored in `~/.config/macviewer-linux/registration.json`,
  and the recent IDs in `recents.txt` next to it.

## Limits

- US keyboard layout only. CapsLock is assumed off when you connect; press it
  again if letters come out in the wrong case.
- No chat, clipboard or file transfer.
- Relay connections only, no direct LAN connection.
- Windows UltraViewer 6.0.0 or later, in 16-bit colour mode.
- Passwords must be ASCII.
- Login reports a fixed 1920x1080 viewer screen.

## Third-party files

- `assets/8bit.dat`: UltraViewer's screen dictionary.
- `third_party/g722`: the WebRTC G.722 decoder (public-domain SpanDSP code
  with a BSD-licensed WebRTC header; see the licence files there), plus an
  output-compatibility shim from MacViewer.
- `tests/fixtures/g722-windows.json`: a Windows decoder test fixture from MacViewer.

## References

- [UltraViewer](https://ultraviewer.net): the remote-desktop app this client
  connects to. LinuxViewer talks to its relay service and to UltraViewer
  running on the Windows PC.
- [MacViewer](https://github.com/DeanOpen/MacViewer): an independent macOS
  UltraViewer client (MIT). LinuxViewer's protocol code is a Rust port of parts of
  its `Sources/UltraViewerProtocol` (registration, partner lookup, login, the TLS
  relay transport, the zstd/RGB555/JPEG screen decoder, mouse and keyboard
  messages, and G.722 computer audio), and its window layout follows MacViewer's.
