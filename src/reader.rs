// assert!(// check our header prediction math is correct, accounting for buffer
//     (self.packet_number == self.next_header) // header loc calc correct
//     // || (bytes_read / 8 + buffer_packet_number == next_header_location)
//     // || bytes_read / 8 < BUFFER_SIZE
// );
// let size = (u16::from_le_bytes(bytes[6..].try_into().unwrap()) / 8) as usize;
// self.next_header = self.packet_number + size as usize + 1;

use crate::{pulse::Pulse};
use std::{convert::TryInto, error::Error, io::Read, mem::take};

pub const TDC_LIMIT: i64 = 107_374_182_400_000; // in picoseconds
pub const HIT_LIMIT: i64 = 26_843_545_600_000; // in picoseconds
pub const ROLL: i64 = 26_000_000_000_000;
pub const CHECK: i64 = 100_000_000_000;

/// Iterator-based structure for traversing the .tpx3 file
pub struct TPX3Reader {
    file: std::fs::File,
    buffer: Vec<u8>,     // where we read into RAM
    pulse: Pulse,        // the output
    buffer_index: usize, // Keep track of our place
    buffer_bytes: usize, // Allows for tracking if we're near the end
    trolls: i64,         // counter
    hrolls: i64,         // counter
    ptdc: i64,           // for comparison and storage
    ptoa: i64,           // ditto
    ptri: u64,
}

impl TPX3Reader {
    pub fn new(tpx3_file_path: &std::path::Path) -> Result<TPX3Reader, Box<dyn Error>> {
        Ok(TPX3Reader {
            file: std::fs::File::open(tpx3_file_path)?,
            buffer: vec![0; 1_000_000],
            pulse: Pulse::default(),
            buffer_index: 0,
            buffer_bytes: 0,
            trolls: 0,
            hrolls: 0,
            ptdc: 0,
            ptoa: 0,
            ptri: 0,
        })
    }
}

impl Iterator for TPX3Reader {
    type Item = Pulse;

    fn next(&mut self) -> Option<Self::Item> {
        self.pulse.time = self.ptdc + self.trolls * TDC_LIMIT;
        self.pulse.triggers = self.ptri;
        if self.buffer_index == 0 {
            self.buffer_bytes = self.file.read(&mut self.buffer).unwrap(); // fill buffer up again
            if self.buffer_bytes == 0 {
                return if self.pulse.hits.is_empty() { None } else { Some(take(&mut self.pulse)) };
            };
        }
        for bs in self.buffer[self.buffer_index..self.buffer_bytes].chunks_exact(8) {
            self.buffer_index += 8;
            let packet = u64::from_le_bytes(bs.try_into().unwrap());
            match packet >> 60 {
                0x6 => {
                    let ((tdc, trigger), ptdc) = (parse_tdc_packet(packet), self.ptdc);
                    self.trolls += (tdc < self.ptdc) as i64;
                    self.ptri = trigger;
                    self.ptdc = tdc; // for next call / tdc
                    match ptdc {
                        0 => self.pulse = Pulse::default(),
                        _ => return Some(take(&mut self.pulse)),
                    }
                }
                0xB => {
                    let (col, row, tot, rtoa) = parse_hit_packet(packet);
                    self.hrolls += roll(rtoa, self.ptoa, self.hrolls, self.ptdc, self.trolls);
                    self.ptoa = rtoa;
                    self.pulse.add_hit(rtoa + self.hrolls * HIT_LIMIT, tot, col, row);
                }
                0xC => self.pulse.hits.last_mut().unwrap().update_with_blob_packet(packet),
                0x4 | 0x7 => (), // ignored headers for Mass spec imaging
                _ => assert!(&bs[..4] == b"TPX3"),
            }
        }
        self.buffer_index = 0; // reset buffer_index for next "loop" iteration; to read more
        self.next() // go again and return whatever that call returns
    }
}

// #[inline(never)]
/// extracts four values: the column, the row, the time-over-threshold, and the
/// time-of-arrival from a "hit" packet; unsafe due to being extremely "hot" code for reading
/// shift rights after multiplication are in place of division; 25_000 is 25 * 1000; 409600000
/// is 16384 * 1000 * 25
fn parse_hit_packet(p: u64) -> (u8, u8, u32, i64) {
    unsafe {
        let pix = (p & 0x0000_7000_0000_0000).unchecked_shr(44);
        let col = (p & 0x0FE0_0000_0000_0000).unchecked_shr(52).unchecked_add(pix.unchecked_shr(2));
        let row = (p & 0x001F_8000_0000_0000).unchecked_shr(45).unchecked_add(pix & 0x3);
        let tot = ((p.unchecked_shr(20)) & 0x3FF).unchecked_mul(25); // should we multiply?
        let tmp = !(p.unchecked_shr(16)) & 0xF;
        let coa = ((p.unchecked_shr(30) & 0x3FFF).unchecked_shl(4) | tmp).unchecked_mul(25_000);
        let toa = (p & 0xFFFF).unchecked_mul(409600000).unchecked_add(coa.unchecked_shr(4)); // ps
        (col as u8, row as u8, tot as u32, toa as i64)
    }
}

// #[inline(never)]
/// extracts the tdc from the packet, there is also a tdc counter that is ignored
fn parse_tdc_packet(p: u64) -> (i64, u64) {
    let trigger_number = (p >> 44) & 0x0FFF; // helpful for debugging
    let coarsetime = (p >> 12) & 0xFFFF_FFFF;
    let expansion_time = (p >> 5 & 0xF).wrapping_sub(1) << 9;
    let finetime = expansion_time / 12;
    let trigtime = (p & 0x0000_0000_0000_0E00) | (finetime & 0x0000_0000_0000_01FF);
    let add_bit = !(expansion_time % 12 == 0 && expansion_time < 1023) as u64;
    let tdc = (coarsetime * 1000 + (trigtime * 1000) / 4096) * 25; // in ps
    ((tdc + add_bit) as i64, trigger_number)
}

// #[inline(never)]
/// ugly function that returns 1 if we need to roll over and 0 if not
fn roll(toa: i64, ptoa: i64, hrol: i64, tdc: i64, trol: i64) -> i64 {
    (toa + CHECK < ptoa && (toa + (hrol + 1) * HIT_LIMIT) - (tdc + trol * TDC_LIMIT) < ROLL) as i64
}

// only reads tdcs; tries to be fast
pub struct TDCReader {
    file: std::fs::File,
    buffer: Vec<u8>,     // where we read into RAM
    buffer_index: usize, // Keep track of our place
    buffer_bytes: usize, // Allows for tracking if we're near the end
    trolls: i64,         // counter
    tdc_full: i64,       // fully-reconstructed TDC time with rollovers
    ptdc: i64,           // for comparison and storage
    first_loop: bool,    // flag to disregard first tdc encountered
}

impl TDCReader {
    pub fn new(tpx3_file_path: &std::path::Path) -> Result<TDCReader, Box<dyn Error>> {
        Ok(TDCReader {
            file: std::fs::File::open(tpx3_file_path)?,
            buffer: vec![0; 1_000_000],
            buffer_index: 0,
            buffer_bytes: 0,
            trolls: 0,
            tdc_full: 0,
            ptdc: 0,
            first_loop: true,
        })
    }
}

impl Iterator for TDCReader {
    type Item = i64;

    /// called for each "next" item in an iterable chain (e.g., a for loop or map)
    fn next(&mut self) -> Option<Self::Item> {
        if self.buffer_index == 0 {
            self.buffer_bytes = self.file.read(&mut self.buffer).unwrap(); // fill buffer up again
            if self.buffer_bytes == 0 {
                return if self.tdc_full == self.ptdc + self.trolls * TDC_LIMIT {
                    None // we finished the file
                } else {
                    self.tdc_full = self.ptdc + self.trolls * TDC_LIMIT;
                    Some(self.tdc_full)
                };
            }
        };
        for (i, bytes) in self.buffer[self.buffer_index..self.buffer_bytes].chunks_exact(8).enumerate() {
            let packet = u64::from_le_bytes(bytes.try_into().unwrap());
            if packet >> 60 == 0x6 {
                self.tdc_full = self.ptdc + self.trolls * TDC_LIMIT;
                let (tdc, _) = parse_tdc_packet(packet);
                self.trolls += (tdc < self.ptdc) as i64;
                self.ptdc = tdc; // for next call / tdc
                if self.first_loop {
                    self.first_loop = false; // we return second & later tdcs
                } else {
                    self.buffer_index += i * 8 + 8; // save the index for next loop
                    return Some(self.tdc_full);
                }
            }
        }
        self.buffer_index = 0; // reset buffer_index for next "loop" iteration
        self.next() // go again and return whatever that call returns
    }
}
