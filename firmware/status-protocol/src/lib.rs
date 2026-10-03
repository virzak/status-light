//! The status-light serial protocol (see ../../PROTOCOL.md), shared by every
//! display board so they parse commands and settings identically.
//!
//! Pure `no_std`, no allocation: feed received bytes to [`LineBuf`], then
//! [`parse_line`] each completed line.

#![cfg_attr(not(test), no_std)]

use flame::FlameParams;

/// A state command from the router: one letter per line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    /// `B`: internet reachable.
    Online,
    /// `A`: 1-2 failed checks.
    Degraded,
    /// `R`: 3+ failed checks.
    Offline,
    /// `G`: free, for tests.
    Green,
    /// `W`: free, for tests.
    White,
    /// `O`: blank the display.
    Off,
}

impl Command {
    pub fn from_letter(c: u8) -> Option<Self> {
        match c.to_ascii_uppercase() {
            b'B' => Some(Self::Online),
            b'A' => Some(Self::Degraded),
            b'R' => Some(Self::Offline),
            b'G' => Some(Self::Green),
            b'W' => Some(Self::White),
            b'O' => Some(Self::Off),
            _ => None,
        }
    }
}

/// Accumulates received bytes into protocol lines. A line is handed out on
/// newline; a line longer than the buffer is dropped whole, so a garbled burst
/// cannot turn into a command.
pub struct LineBuf {
    buf: [u8; 48],
    len: usize,
    overflow: bool,
}

impl LineBuf {
    pub const fn new() -> Self {
        Self {
            buf: [0; 48],
            len: 0,
            overflow: false,
        }
    }

    pub fn push(&mut self, b: u8) -> Option<&[u8]> {
        if b == b'\r' || b == b'\n' {
            let done = !self.overflow && self.len > 0;
            let len = self.len;
            self.len = 0;
            self.overflow = false;
            return done.then(|| &self.buf[..len]);
        }
        if self.len < self.buf.len() {
            self.buf[self.len] = b;
            self.len += 1;
        } else {
            self.overflow = true;
        }
        None
    }
}

impl Default for LineBuf {
    fn default() -> Self {
        Self::new()
    }
}

/// One protocol line: a single state letter, or `S <key> [value]`.
#[derive(Debug, PartialEq, Eq)]
pub enum Line<'a> {
    Command(Command),
    Setting(&'a str, &'a str),
    Unknown,
}

pub fn parse_line(raw: &[u8]) -> Line<'_> {
    let Ok(text) = core::str::from_utf8(raw) else {
        return Line::Unknown;
    };
    let text = text.trim();
    let bytes = text.as_bytes();
    if bytes.len() == 1 {
        return Command::from_letter(bytes[0]).map_or(Line::Unknown, Line::Command);
    }
    match text.split_once(' ') {
        Some((s, rest)) if s.eq_ignore_ascii_case("S") => {
            let (key, value) = rest.trim().split_once(' ').unwrap_or((rest.trim(), ""));
            Line::Setting(key, value.trim())
        }
        _ => Line::Unknown,
    }
}

/// Apply a flame-tuning setting (`cooling`, `drift`, ...) to `params`, clamping
/// values the flame cannot use. Returns false if `key` is not a flame key.
pub fn apply_flame_setting(key: &str, value: u8, params: &mut FlameParams) -> bool {
    let is = |name: &str| key.eq_ignore_ascii_case(name);
    if is("cooling") {
        params.cooling = value;
    } else if is("drift") {
        params.drift = value.min(3);
    } else if is("flicker") {
        params.flicker = value.max(1);
    } else if is("seed_min") {
        params.seed_min = value;
    } else if is("seed_max") {
        params.seed_max = value;
    } else if is("blue_full") {
        params.blue_full = value.max(1);
    } else if is("green_start") {
        params.green_start = value;
    } else if is("white_start") {
        params.white_start = value;
    } else {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed<'a>(lb: &'a mut LineBuf, bytes: &[u8]) -> Option<Line<'a>> {
        let (last, rest) = bytes.split_last().unwrap();
        for &b in rest {
            assert!(lb.push(b).is_none());
        }
        lb.push(*last).map(parse_line)
    }

    #[test]
    fn commands() {
        let mut lb = LineBuf::new();
        assert_eq!(feed(&mut lb, b"B\n"), Some(Line::Command(Command::Online)));
        assert_eq!(feed(&mut lb, b"X\n"), Some(Line::Unknown));
    }

    #[test]
    fn crlf_is_one_line() {
        let mut lb = LineBuf::new();
        assert_eq!(feed(&mut lb, b"r\r"), Some(Line::Command(Command::Offline)));
        assert_eq!(lb.push(b'\n'), None);
    }

    #[test]
    fn settings_are_not_commands() {
        // The old per-byte parsers took `S brightness 40` as the G command.
        let mut lb = LineBuf::new();
        assert_eq!(feed(&mut lb, b"S brightness 40\n"), Some(Line::Setting("brightness", "40")));
        assert_eq!(feed(&mut lb, b"S reset\n"), Some(Line::Setting("reset", "")));
    }

    #[test]
    fn overlong_lines_are_dropped() {
        let mut lb = LineBuf::new();
        let mut long = [b'B'; 60];
        long[59] = b'\n';
        assert_eq!(feed(&mut lb, &long), None);
        assert_eq!(feed(&mut lb, b"B\n"), Some(Line::Command(Command::Online)));
    }

    #[test]
    fn flame_settings_clamp() {
        let mut p = FlameParams::default();
        assert!(apply_flame_setting("drift", 9, &mut p));
        assert_eq!(p.drift, 3);
        assert!(apply_flame_setting("FLICKER", 0, &mut p));
        assert_eq!(p.flicker, 1);
        assert!(!apply_flame_setting("brightness", 50, &mut p));
    }
}
