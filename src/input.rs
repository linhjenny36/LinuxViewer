//! Mouse and keyboard input (MouseMessage.swift, KeyboardMessage.swift), with the
//! held-button and held-key state of MacRemoteInputEncoder.swift.

#[derive(Clone, Copy)]
pub enum Mouse {
    Move { x: i32, y: i32 },
    /// 0 left, 1 right, 2 middle.
    Button { button: u8, pressed: bool },
    /// Wheel notches; positive scrolls up.
    Wheel(i32),
}

#[derive(Default)]
pub struct MouseEncoder {
    held: [bool; 3],
    pointer: Option<(i32, i32)>,
    last_move_flags: Option<i32>,
}

/// Win32 WM_* message per button: (down, up).
const BUTTON_MESSAGES: [(i32, i32); 3] = [(0x201, 0x202), (0x204, 0x205), (0x207, 0x208)];

fn text(message: i32, params: &[i32]) -> Vec<u8> {
    let mut f = vec!["8".to_string(), message.to_string()];
    f.extend(params.iter().map(i32::to_string));
    f.join("|||").into_bytes()
}

/// Win32 lParam: two 16-bit coordinate words.
fn packed(x: i32, y: i32) -> i32 {
    (x as u16 as u32 | (y as u16 as u32) << 16) as i32
}

impl MouseEncoder {
    /// MK_LBUTTON, MK_RBUTTON, MK_MBUTTON.
    fn flags(&self) -> i32 {
        [0x01, 0x02, 0x10].iter().zip(self.held).filter(|(_, h)| *h).map(|(f, _)| f).sum()
    }

    fn lparam(&self) -> i32 {
        self.pointer.map_or(0, |(x, y)| packed(x, y))
    }

    /// Returns the application payloads to send, in order.
    /// Coordinates are Windows virtual-desktop pixels.
    pub fn encode(&mut self, input: Mouse) -> Vec<Vec<u8>> {
        let mut out = vec![];
        match input {
            Mouse::Move { x, y } => self.move_to(x, y, false, &mut out),
            Mouse::Button { button, pressed } => self.button(button as usize, pressed, &mut out),
            Mouse::Wheel(notches) if notches != 0 => {
                let delta = (notches.clamp(-256, 256) * 120) as i16;
                let wparam = (self.flags() as u16 as u32 | (delta as u16 as u32) << 16) as i32;
                out.push(text(0x20a, &[wparam, self.lparam()]));
            }
            Mouse::Wheel(_) => {}
        }
        out
    }

    pub fn release_all(&mut self) -> Vec<Vec<u8>> {
        let mut out = vec![];
        (0..3).for_each(|b| self.button(b, false, &mut out));
        out
    }

    fn move_to(&mut self, x: i32, y: i32, force: bool, out: &mut Vec<Vec<u8>>) {
        let flags = self.flags();
        if force || self.pointer != Some((x, y)) || self.last_move_flags != Some(flags) {
            out.push(text(0x200, &[flags, x, y]));
            self.last_move_flags = Some(flags);
        }
        self.pointer = Some((x, y));
    }

    fn button(&mut self, b: usize, pressed: bool, out: &mut Vec<Vec<u8>>) {
        if b >= 3 || self.held[b] == pressed {
            return;
        }
        if pressed {
            // A press always restates the position first, as the Windows viewer does.
            let Some((x, y)) = self.pointer else { return };
            self.move_to(x, y, true, out);
        }
        self.held[b] = pressed;
        let (down, up) = BUTTON_MESSAGES[b];
        out.push(text(if pressed { down } else { up }, &[self.flags(), self.lparam()]));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: Vec<Vec<u8>>) -> Vec<String> {
        v.into_iter().map(|b| String::from_utf8(b).unwrap()).collect()
    }

    #[test]
    fn click_drag_wheel_match_win32_messages() {
        let mut e = MouseEncoder::default();
        assert!(e.encode(Mouse::Button { button: 0, pressed: true }).is_empty(), "no position yet");
        assert_eq!(s(e.encode(Mouse::Move { x: 10, y: 20 })), ["8|||512|||0|||10|||20"]);
        assert!(e.encode(Mouse::Move { x: 10, y: 20 }).is_empty(), "unchanged move");
        let lp = (10 | 20 << 16).to_string();
        assert_eq!(s(e.encode(Mouse::Button { button: 0, pressed: true })),
                   ["8|||512|||0|||10|||20".to_string(), format!("8|||513|||1|||{lp}")]);
        assert!(e.encode(Mouse::Button { button: 0, pressed: true }).is_empty(), "repeat press");
        assert_eq!(s(e.encode(Mouse::Move { x: -5, y: 20 })), ["8|||512|||1|||-5|||20"]);
        let lp = (0xfffb | 20 << 16).to_string();
        assert_eq!(s(e.release_all()), [format!("8|||514|||0|||{lp}")]);
        assert_eq!(s(e.encode(Mouse::Wheel(-1))), [format!("8|||522|||{}|||{lp}", (-120i32 << 16))]);
        assert_eq!(s(e.encode(Mouse::Button { button: 1, pressed: true }))[1], format!("8|||516|||2|||{lp}"));
    }
}

// --- keyboard ---

use winit::keyboard::KeyCode;

/// A US-layout key: Windows virtual key, set-1 scan code (E0-prefixed keys keep
/// the prefix in the high byte), and the ASCII it types unshifted / shifted (0: none).
struct Key(u8, u16, u8, u8);

/// Physical keys only: the Windows side's own layout and input method (for
/// example Unikey for Vietnamese) turn them into text.
fn key(code: KeyCode) -> Option<Key> {
    use KeyCode::*;
    let letter = |vk: u8, scan: u16| Key(vk, scan, vk.to_ascii_lowercase(), vk);
    Some(match code {
        KeyA => letter(b'A', 0x1e), KeyB => letter(b'B', 0x30), KeyC => letter(b'C', 0x2e), KeyD => letter(b'D', 0x20),
        KeyE => letter(b'E', 0x12), KeyF => letter(b'F', 0x21), KeyG => letter(b'G', 0x22), KeyH => letter(b'H', 0x23),
        KeyI => letter(b'I', 0x17), KeyJ => letter(b'J', 0x24), KeyK => letter(b'K', 0x25), KeyL => letter(b'L', 0x26),
        KeyM => letter(b'M', 0x32), KeyN => letter(b'N', 0x31), KeyO => letter(b'O', 0x18), KeyP => letter(b'P', 0x19),
        KeyQ => letter(b'Q', 0x10), KeyR => letter(b'R', 0x13), KeyS => letter(b'S', 0x1f), KeyT => letter(b'T', 0x14),
        KeyU => letter(b'U', 0x16), KeyV => letter(b'V', 0x2f), KeyW => letter(b'W', 0x11), KeyX => letter(b'X', 0x2d),
        KeyY => letter(b'Y', 0x15), KeyZ => letter(b'Z', 0x2c),
        Digit1 => Key(b'1', 0x02, b'1', b'!'), Digit2 => Key(b'2', 0x03, b'2', b'@'), Digit3 => Key(b'3', 0x04, b'3', b'#'),
        Digit4 => Key(b'4', 0x05, b'4', b'$'), Digit5 => Key(b'5', 0x06, b'5', b'%'), Digit6 => Key(b'6', 0x07, b'6', b'^'),
        Digit7 => Key(b'7', 0x08, b'7', b'&'), Digit8 => Key(b'8', 0x09, b'8', b'*'), Digit9 => Key(b'9', 0x0a, b'9', b'('),
        Digit0 => Key(b'0', 0x0b, b'0', b')'),
        Minus => Key(0xbd, 0x0c, b'-', b'_'), Equal => Key(0xbb, 0x0d, b'=', b'+'),
        BracketLeft => Key(0xdb, 0x1a, b'[', b'{'), BracketRight => Key(0xdd, 0x1b, b']', b'}'),
        Backslash => Key(0xdc, 0x2b, b'\\', b'|'), Semicolon => Key(0xba, 0x27, b';', b':'),
        Quote => Key(0xde, 0x28, b'\'', b'"'), Backquote => Key(0xc0, 0x29, b'`', b'~'),
        Comma => Key(0xbc, 0x33, b',', b'<'), Period => Key(0xbe, 0x34, b'.', b'>'), Slash => Key(0xbf, 0x35, b'/', b'?'),
        Space => Key(0x20, 0x39, b' ', b' '),
        Enter => Key(0x0d, 0x1c, 13, 13), Tab => Key(0x09, 0x0f, 9, 9), Backspace => Key(0x08, 0x0e, 8, 8),
        Escape => Key(0x1b, 0x01, 27, 27),
        ShiftLeft => Key(0xa0, 0x2a, 0, 0), ShiftRight => Key(0xa1, 0x36, 0, 0),
        ControlLeft => Key(0xa2, 0x1d, 0, 0), ControlRight => Key(0xa3, 0xe01d, 0, 0),
        AltLeft => Key(0xa4, 0x38, 0, 0), AltRight => Key(0xa5, 0xe038, 0, 0),
        SuperLeft => Key(0x5b, 0xe05b, 0, 0), SuperRight => Key(0x5c, 0xe05c, 0, 0),
        ContextMenu => Key(0x5d, 0xe05d, 0, 0), CapsLock => Key(0x14, 0x3a, 0, 0),
        F1 => Key(0x70, 0x3b, 0, 0), F2 => Key(0x71, 0x3c, 0, 0), F3 => Key(0x72, 0x3d, 0, 0), F4 => Key(0x73, 0x3e, 0, 0),
        F5 => Key(0x74, 0x3f, 0, 0), F6 => Key(0x75, 0x40, 0, 0), F7 => Key(0x76, 0x41, 0, 0), F8 => Key(0x77, 0x42, 0, 0),
        F9 => Key(0x78, 0x43, 0, 0), F10 => Key(0x79, 0x44, 0, 0), F11 => Key(0x7a, 0x57, 0, 0), F12 => Key(0x7b, 0x58, 0, 0),
        Insert => Key(0x2d, 0xe052, 0, 0), Delete => Key(0x2e, 0xe053, 0, 0),
        Home => Key(0x24, 0xe047, 0, 0), End => Key(0x23, 0xe04f, 0, 0),
        PageUp => Key(0x21, 0xe049, 0, 0), PageDown => Key(0x22, 0xe051, 0, 0),
        ArrowLeft => Key(0x25, 0xe04b, 0, 0), ArrowUp => Key(0x26, 0xe048, 0, 0),
        ArrowRight => Key(0x27, 0xe04d, 0, 0), ArrowDown => Key(0x28, 0xe050, 0, 0),
        PrintScreen => Key(0x2c, 0xe037, 0, 0), ScrollLock => Key(0x91, 0x46, 0, 0),
        Pause => Key(0x13, 0x45, 0, 0), NumLock => Key(0x90, 0xe045, 0, 0),
        // shortcut: keypad keys always type digits, as if NumLock were on; track NumLock if that matters.
        Numpad0 => Key(0x60, 0x52, b'0', b'0'), Numpad1 => Key(0x61, 0x4f, b'1', b'1'), Numpad2 => Key(0x62, 0x50, b'2', b'2'),
        Numpad3 => Key(0x63, 0x51, b'3', b'3'), Numpad4 => Key(0x64, 0x4b, b'4', b'4'), Numpad5 => Key(0x65, 0x4c, b'5', b'5'),
        Numpad6 => Key(0x66, 0x4d, b'6', b'6'), Numpad7 => Key(0x67, 0x47, b'7', b'7'), Numpad8 => Key(0x68, 0x48, b'8', b'8'),
        Numpad9 => Key(0x69, 0x49, b'9', b'9'), NumpadDecimal => Key(0x6e, 0x53, b'.', b'.'),
        NumpadAdd => Key(0x6b, 0x4e, b'+', b'+'), NumpadSubtract => Key(0x6d, 0x4a, b'-', b'-'),
        NumpadMultiply => Key(0x6a, 0x37, b'*', b'*'), NumpadDivide => Key(0x6f, 0xe035, b'/', b'/'),
        NumpadEnter => Key(0x0d, 0xe01c, 13, 13),
        _ => return None,
    })
}

const SHIFT: [KeyCode; 2] = [KeyCode::ShiftLeft, KeyCode::ShiftRight];
const ALT: [KeyCode; 2] = [KeyCode::AltLeft, KeyCode::AltRight];
/// Shortcut modifiers: with one held, a key sends no character (the Windows viewer's VK path).
const SHORTCUT: [KeyCode; 6] = [KeyCode::ControlLeft, KeyCode::ControlRight, KeyCode::AltLeft, KeyCode::AltRight,
                                KeyCode::SuperLeft, KeyCode::SuperRight];

fn is_modifier(code: KeyCode) -> bool {
    SHIFT.contains(&code) || SHORTCUT.contains(&code)
}

/// A pressed key remembers what its down message carried, so the up message matches.
struct Held {
    code: KeyCode,
    vk: u8,
    scan: u16,
    character: u16,
    system: bool,
}

#[derive(Default)]
pub struct KeyboardEncoder {
    held: Vec<Held>,
    /// shortcut: assumes CapsLock starts off on Windows; the user can press it again if letters come out inverted.
    caps: bool,
}

/// 30 "|||" kind vk scan(LE) character(LE) capsLock: 11 bytes.
fn key_message(kind: u8, vk: u8, scan: u16, character: u16, caps: bool) -> Vec<u8> {
    let [s0, s1] = scan.to_le_bytes();
    let [c0, c1] = character.to_le_bytes();
    vec![30, b'|', b'|', b'|', kind, vk, s0, s1, c0, c1, caps as u8]
}

impl KeyboardEncoder {
    fn holds(&self, codes: &[KeyCode]) -> bool {
        self.held.iter().any(|h| codes.contains(&h.code))
    }

    /// Returns the payloads to send. Auto-repeat presses send another key-down, as Windows expects.
    pub fn encode(&mut self, code: KeyCode, pressed: bool) -> Vec<Vec<u8>> {
        let Some(Key(vk, scan, plain, shifted)) = key(code) else { return vec![] };
        if !pressed {
            let Some(i) = self.held.iter().position(|h| h.code == code) else { return vec![] };
            return vec![self.release(i)];
        }
        if code == KeyCode::CapsLock && !self.holds(&[code]) {
            self.caps = !self.caps;
        }
        let shift = self.holds(&SHIFT) ^ (self.caps && plain.is_ascii_lowercase());
        let character = if self.holds(&SHORTCUT) || is_modifier(code) { 0 } else if shift { shifted } else { plain };
        // Alt makes Windows report WM_SYSKEYDOWN / WM_SYSKEYUP.
        let system = ALT.contains(&code) || self.holds(&ALT);
        let msg = key_message(if system { 3 } else { 1 }, vk, scan, character as u16, self.caps);
        if !self.holds(&[code]) {
            self.held.push(Held { code, vk, scan, character: character as u16, system });
        }
        vec![msg]
    }

    fn release(&mut self, i: usize) -> Vec<u8> {
        let h = self.held.remove(i);
        key_message(if h.system { 4 } else { 2 }, h.vk, h.scan, h.character, self.caps)
    }

    /// Releases ordinary keys first, then modifiers, newest first.
    pub fn release_all(&mut self) -> Vec<Vec<u8>> {
        let mut out = vec![];
        for modifiers in [false, true] {
            while let Some(i) = self.held.iter().rposition(|h| is_modifier(h.code) == modifiers) {
                out.push(self.release(i));
            }
        }
        out
    }
}

#[cfg(test)]
mod keyboard_tests {
    use super::*;
    use KeyCode::*;

    #[test]
    fn layout_matches_swift_fixture() {
        // KeyboardMessageTests.swift: down, VK_A, scan 0x1e, 'A', caps off.
        assert_eq!(key_message(1, 0x41, 0x1e, 0x41, false), [0x1e, 0x7c, 0x7c, 0x7c, 1, 0x41, 0x1e, 0, 0x41, 0, 0]);
    }

    #[test]
    fn typing_shortcuts_and_alt() {
        let mut k = KeyboardEncoder::default();
        assert_eq!(k.encode(KeyA, true), [key_message(1, 0x41, 0x1e, b'a' as u16, false)]);
        assert_eq!(k.encode(KeyA, true), [key_message(1, 0x41, 0x1e, b'a' as u16, false)], "auto-repeat");
        assert_eq!(k.encode(KeyA, false), [key_message(2, 0x41, 0x1e, b'a' as u16, false)]);
        assert!(k.encode(KeyA, false).is_empty(), "release of a key not held");

        k.encode(ShiftLeft, true);
        assert_eq!(k.encode(Digit1, true), [key_message(1, b'1', 0x02, b'!' as u16, false)]);
        k.release_all();

        k.encode(ControlLeft, true);
        assert_eq!(k.encode(KeyC, true), [key_message(1, 0x43, 0x2e, 0, false)], "Ctrl+C sends no character");
        assert_eq!(k.release_all(), [key_message(2, 0x43, 0x2e, 0, false), key_message(2, 0xa2, 0x1d, 0, false)]);

        assert_eq!(k.encode(AltLeft, true), [key_message(3, 0xa4, 0x38, 0, false)]);
        assert_eq!(k.encode(F4, true), [key_message(3, 0x73, 0x3e, 0, false)]);
        k.encode(AltLeft, false);
        assert_eq!(k.encode(F4, false), [key_message(4, 0x73, 0x3e, 0, false)], "up matches its system down");

        k.encode(CapsLock, true);
        k.encode(CapsLock, false);
        assert_eq!(k.encode(KeyQ, true), [key_message(1, 0x51, 0x10, b'Q' as u16, true)]);
        assert_eq!(k.encode(Enter, true), [key_message(1, 0x0d, 0x1c, 13, true)]);
        assert!(k.encode(Fn, true).is_empty(), "unmapped key");
    }
}
