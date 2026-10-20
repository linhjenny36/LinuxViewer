//! Computer audio: ComputerAudioMessage.swift, G722Decoder.swift and RemoteAudioPlayer.swift.

use crate::wire::Result;
use std::io::Write;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{sync_channel, SyncSender};
use std::thread;

pub const MAX_ENCODED_PACKET: usize = 64 * 1024;
pub const MAX_PCM_PACKET: usize = MAX_ENCODED_PACKET * 4;

#[repr(C)]
struct RawDecoder {
    _opaque: [u8; 0],
}

extern "C" {
    fn MacViewerG722Create() -> *mut RawDecoder;
    fn MacViewerG722Destroy(d: *mut RawDecoder);
    fn MacViewerG722Decode(d: *mut RawDecoder, encoded: *const u8, len: usize, pcm: *mut i16, cap: usize) -> usize;
}

/// Stateful 64 kbps G.722 decoder. One encoded byte gives two i16 samples, played
/// as 48 kHz stereo. Keep one per session: state spans packets.
pub struct G722(*mut RawDecoder);

// SAFETY: the C state is plain memory, used from one thread at a time through &mut self.
unsafe impl Send for G722 {}

impl G722 {
    pub fn new() -> Result<Self> {
        let d = unsafe { MacViewerG722Create() };
        if d.is_null() {
            return Err("G.722 decoder allocation failed".into());
        }
        Ok(G722(d))
    }

    /// Returns little-endian i16 PCM.
    pub fn decode(&mut self, encoded: &[u8]) -> Result<Vec<u8>> {
        if encoded.len() > MAX_ENCODED_PACKET {
            return Err("audio packet too large".into());
        }
        if encoded.is_empty() {
            return Ok(vec![]);
        }
        let mut pcm = vec![0i16; encoded.len() * 2];
        // SAFETY: both buffers are valid for the lengths passed.
        let n = unsafe { MacViewerG722Decode(self.0, encoded.as_ptr(), encoded.len(), pcm.as_mut_ptr(), pcm.len()) };
        if n != pcm.len() {
            return Err("G.722 decoder failure".into());
        }
        Ok(pcm.iter().flat_map(|s| s.to_le_bytes()).collect())
    }
}

impl Drop for G722 {
    fn drop(&mut self) {
        unsafe { MacViewerG722Destroy(self.0) }
    }
}

/// Host to viewer: command 43 carries G.722 bytes, command 42 an i32 LE count of
/// silent PCM bytes (silence does not advance the decoder). Returns PCM to play.
pub fn decode_message(parts: &[&[u8]], decoder: &mut G722) -> Result<Vec<u8>> {
    if parts.len() < 2 || parts[0].is_empty() {
        return Err("malformed audio message".into());
    }
    if parts.iter().map(|p| p.len()).sum::<usize>() > MAX_PCM_PACKET {
        return Err("audio packet too large".into());
    }
    match parts[0][0] {
        43 => decoder.decode(parts[1]),
        42 if parts[1].len() >= 4 => {
            let n = i32::from_le_bytes([parts[1][0], parts[1][1], parts[1][2], parts[1][3]]);
            if n < 0 || n as usize > MAX_PCM_PACKET {
                return Err("invalid audio silence length".into());
            }
            Ok(vec![0; n as usize])
        }
        _ => Err("malformed audio message".into()),
    }
}

/// Plays raw 48 kHz stereo s16le PCM through the desktop's sound server.
/// shortcut: pipes to pacat / pw-play / aplay instead of linking an audio library;
/// switch to cpal if none of those players is installed on a target system.
pub struct Player {
    tx: SyncSender<Vec<u8>>,
}

impl Player {
    pub fn start() -> Result<Self> {
        let mut child = spawn_player()?;
        let mut stdin = child.stdin.take().expect("piped stdin");
        // A short queue caps latency: packets beyond it are dropped, not delayed.
        let (tx, rx) = sync_channel::<Vec<u8>>(8);
        thread::spawn(move || {
            for pcm in rx {
                if stdin.write_all(&pcm).is_err() {
                    break;
                }
            }
            drop(stdin);
            let _ = child.wait();
        });
        Ok(Player { tx })
    }

    pub fn play(&self, pcm: &[u8]) {
        let _ = self.tx.try_send(pcm.to_vec());
    }
}

fn spawn_player() -> Result<Child> {
    let players: [(&str, &[&str]); 3] = [
        ("pacat", &["--playback", "--raw", "--format=s16le", "--rate=48000", "--channels=2", "--latency-msec=100"]),
        ("pw-play", &["--format=s16", "--rate=48000", "--channels=2", "-"]),
        ("aplay", &["-q", "-t", "raw", "-f", "S16_LE", "-r", "48000", "-c", "2"]),
    ];
    for (cmd, args) in players {
        if let Ok(child) = Command::new(cmd).args(args).stdin(Stdio::piped()).stdout(Stdio::null()).spawn() {
            return Ok(child);
        }
    }
    Err("no audio player found; install pulseaudio-utils (pacat), pipewire (pw-play) or alsa-utils (aplay)".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose::STANDARD, Engine};

    #[test]
    fn naudio_output_wraps_instead_of_saturating() {
        // Windows fixture: encoded byte 38 gives raw QMF 37090 at sample 76; NAudio wraps it to -28446.
        let pcm = G722::new().unwrap().decode(&(0..=38).collect::<Vec<u8>>()).unwrap();
        assert_eq!(i16::from_le_bytes([pcm[152], pcm[153]]), -28446);
    }

    #[test]
    fn matches_windows_codec_fixture_one_shot_and_streamed() {
        let text = include_str!("../tests/fixtures/g722-windows.json");
        let encoded = STANDARD.decode(crate::profile::json_field(text, "EncodedBase64").unwrap()).unwrap();
        let want = STANDARD.decode(crate::profile::json_field(text, "PCMBase64").unwrap()).unwrap();
        assert_eq!(G722::new().unwrap().decode(&encoded).unwrap(), want);
        let mut d = G722::new().unwrap();
        let streamed: Vec<u8> = encoded.chunks(7).flat_map(|c| d.decode(c).unwrap()).collect();
        assert_eq!(streamed, want);
    }

    #[test]
    fn messages_decode_audio_and_silence() {
        let mut d = G722::new().unwrap();
        assert_eq!(decode_message(&[&[43], &[0, 1, 2]], &mut d).unwrap().len(), 12);
        assert_eq!(decode_message(&[&[42], &10i32.to_le_bytes()], &mut d).unwrap(), vec![0; 10]);
        assert!(decode_message(&[&[42], &(-1i32).to_le_bytes()], &mut d).is_err());
        assert!(decode_message(&[&[43]], &mut d).is_err());
    }
}
