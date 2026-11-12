//! A message-level FIT file writer for tests: the crate's unit tests reach it
//! as `crate::test_support::fit_writer`, the integration tests include this
//! file by `#[path]`, so it names nothing outside itself. Its CRC is the FIT
//! SDK's own, kept apart from the reader's so a broken reader CRC fails the
//! tests instead of agreeing with them.

/// Unix seconds at the FIT epoch, 1989-12-31T00:00:00Z.
pub(crate) const FIT_EPOCH: i64 = 631_065_600;

/// FIT base types the tests write.
pub(crate) const ENUM: u8 = 0x00;
pub(crate) const U8: u8 = 0x02;
pub(crate) const STR: u8 = 0x07;
pub(crate) const I32: u8 = 0x85;
pub(crate) const U32: u8 = 0x86;

/// FIT CRC-16 (the SDK's nibble-table form).
pub(crate) fn fit_crc(data: &[u8]) -> u16 {
    const T: [u16; 16] = [
        0x0000, 0xCC01, 0xD801, 0x1400, 0xF001, 0x3C00, 0x2800, 0xE401, 0xA001, 0x6C00, 0x7800,
        0xB401, 0x5000, 0x9C01, 0x8801, 0x4400,
    ];
    let mut crc: u16 = 0;
    for &b in data {
        let mut tmp = T[(crc & 0xF) as usize];
        crc = (crc >> 4) & 0x0FFF;
        crc = crc ^ tmp ^ T[(b & 0xF) as usize];
        tmp = T[(crc & 0xF) as usize];
        crc = (crc >> 4) & 0x0FFF;
        crc = crc ^ tmp ^ T[((b >> 4) & 0xF) as usize];
    }
    crc
}

/// Degrees as FIT semicircles (2^31 semicircles = 180°), rounded.
pub(crate) fn semi(deg: f64) -> i32 {
    (deg * 2_147_483_648.0 / 180.0).round() as i32
}

/// A file header declaring `data_size` message bytes: 14 bytes with its own
/// CRC, or the legacy 12 without one.
pub(crate) fn header(header14: bool, data_size: u32) -> Vec<u8> {
    let mut f = vec![if header14 { 14 } else { 12 }, 0x20];
    f.extend_from_slice(&2132u16.to_le_bytes());
    f.extend_from_slice(&data_size.to_le_bytes());
    f.extend_from_slice(b".FIT");
    if header14 {
        let c = fit_crc(&f);
        f.extend_from_slice(&c.to_le_bytes());
    }
    f
}

/// Message-level FIT writer: definitions and data messages in `data`, then a
/// header and a trailing CRC from [`FitWriter::finish`].
#[derive(Default)]
pub(crate) struct FitWriter {
    pub(crate) data: Vec<u8>,
    big_endian: [bool; 16],
}

impl FitWriter {
    /// Define local type `local` as global `global` with `(num, size, base)`
    /// fields and `(num, size, dev_index)` developer fields.
    pub(crate) fn def(
        &mut self,
        local: u8,
        global: u16,
        big: bool,
        fields: &[(u8, u8, u8)],
        dev: &[(u8, u8, u8)],
    ) {
        let dev_flag = if dev.is_empty() { 0 } else { 0x20 };
        self.data.push(0x40 | dev_flag | local);
        self.data.push(0);
        self.data.push(u8::from(big));
        if big {
            self.data.extend_from_slice(&global.to_be_bytes());
        } else {
            self.data.extend_from_slice(&global.to_le_bytes());
        }
        self.data.push(fields.len() as u8);
        for &(n, s, b) in fields {
            self.data.extend_from_slice(&[n, s, b]);
        }
        if !dev.is_empty() {
            self.data.push(dev.len() as u8);
            for &(n, s, i) in dev {
                self.data.extend_from_slice(&[n, s, i]);
            }
        }
        self.big_endian[local as usize] = big;
    }

    pub(crate) fn msg(&mut self, local: u8, body: &[u8]) {
        self.data.push(local);
        self.data.extend_from_slice(body);
    }

    /// A compressed-timestamp data message (local types 0–3 only).
    pub(crate) fn compressed(&mut self, local: u8, offset: u8, body: &[u8]) {
        self.data.push(0x80 | (local << 5) | (offset & 0x1F));
        self.data.extend_from_slice(body);
    }

    /// `v` in local type `local`'s byte order.
    pub(crate) fn u32(&self, local: u8, v: u32) -> [u8; 4] {
        if self.big_endian[local as usize] {
            v.to_be_bytes()
        } else {
            v.to_le_bytes()
        }
    }

    /// `v` in local type `local`'s byte order.
    pub(crate) fn i32(&self, local: u8, v: i32) -> [u8; 4] {
        if self.big_endian[local as usize] {
            v.to_be_bytes()
        } else {
            v.to_le_bytes()
        }
    }

    /// The whole file: [`header`], the messages, the trailing CRC.
    pub(crate) fn finish(&self, header14: bool) -> Vec<u8> {
        let mut f = header(header14, self.data.len() as u32);
        f.extend_from_slice(&self.data);
        let crc = fit_crc(&f);
        f.extend_from_slice(&crc.to_le_bytes());
        f
    }
}
