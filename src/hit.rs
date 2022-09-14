// use std::convert::TryInto;

use crate::{image, reader::HIT_LIMIT, stage::Coord};

/// a structure for holding "hit" data
#[derive(Clone, Copy, Debug, Default)]
pub struct Hit {
    pub toa: i64,   // time of arrival in ps
    pub tot: u32,   // time over threshold in ns / 25
    pub index: u32,
    pub label: u16, // 0 for unlabelled, 1 for start of labelling
    pub size: u16,
    pub col: u8, // column out of 256
    pub row: u8, // row out of 256
    pub col_offset: u8,
    pub row_offset: u8,
}

impl PartialEq for Hit {
    fn eq(&self, o: &Hit) -> bool { self.toa == o.toa && self.col == o.col && self.row == o.row }
}

impl Hit {

    pub fn new(index: u32, toa: i64, tot: u32, col: u8, row: u8) -> Hit {
        Hit { index, toa, tot, col, row, label: 0, size: 0, col_offset: 0, row_offset: 0 }
    }
    
    /// packs the hit into a "hit" packet
    pub fn to_hit_packet(self) -> u64 {
        let header = 0xB << 60;
        let toa = (self.toa % HIT_LIMIT) as u64; // get rollovers and convert to unsigned
        let (tot, col, row): (u64, u64, u64) = (self.tot.into(), self.col.into(), self.row.into());
        let pix = ((col % 2) << 2) | (row % 4); // extract pix from row/col
        let col_bits: u64 = (col - (pix / 4)) << 52; // use pix to find col bits
        let row_bits: u64 = (row - (pix & 0x3)) << 45; // use pix to find row bits
        let tot_bits: u64 = ((tot / 25) % 1024) << 20; // 1024 is for large, clustered TOTs
        let global_time = toa / 409_600_000; // extract "coarse" toa from the global time
        let remainder = (toa % 409_600_000) / (25_000 / 16);
        let remainder = remainder - (remainder / 3125); // fix off-by-0.5 error (3125 = 1562.5 * 2)
        let fta_bit = (!remainder & 0xF) << 16; // extract fine bits
        let cta_bit = ((remainder & !0xF) >> 4) << 30; // extract course bits
        header | col_bits | row_bits | (pix << 44) | cta_bit | tot_bits | fta_bit | global_time
    }

    pub fn to_blob_packet(self) -> u64 {
        let header = 0xCAu64 << 56; // 1 byte
        let col_offset_bits = (self.col_offset as u64) << 48; // 1 byte
        let row_offset_bits = (self.row_offset as u64) << 40; // 1 byte
        let tot_coarse = (self.tot as u64 / 1024) & 0x00FF_FFFF << 16; // 3 bytes
        let size = self.size as u64; // 2 bytes
        header | col_offset_bits | row_offset_bits | tot_coarse | size // 1| 1 | 1 | 3 | 2
    }

    pub fn update_with_blob_packet(&mut self, packet: u64) {
        unsafe {
            self.tot += (packet.unchecked_shr(16) & 0x00FF_FFFF).unchecked_mul(1024 * 25) as u32;
            self.col_offset = (packet.unchecked_shr(48) & 0xFF) as u8;
            self.row_offset = (packet.unchecked_shr(40) & 0xFF) as u8;
            self.size = (packet & 0xFFFF) as u16;
        }
    }

    // splats in a square, rather than in a circular pattern (but is a bit faster)
    pub fn quicksplat(&self) -> Vec<Hit> {
        if self.size == 0 {
            return vec![*self];
        }
        let (col, row, tot) = (self.col as i16, self.row as i16, self.tot / self.size as u32);
        let (mut dx, mut dy, mut x, mut y, mut hits) = (0, -1, 0, 0, vec![]);
        while hits.len() < self.size as usize {
            let (new_col, new_row) = (col + x, row + y);
            if (0..=255).contains(&new_col) && (0..=255).contains(&new_row) {
                hits.push(self.make_proximal(new_col as u8, new_row as u8, tot))
            }
            if x == y || (x < 0 && x == -y) || (x > 0 && x == 1 - y) {
                (dx, dy) = (-dy, dx);
            }
            x += dx;
            y += dy;
        }
        hits
    }

    #[inline(never)]
    /// Checks if two hits 1 tile apart (but not equal); -1 is 255 due to subtraction
    pub fn is_proximal(&self, other: &Hit) -> bool {
        matches!(
            (self.col.wrapping_sub(other.col), self.row.wrapping_sub(other.row)),
            (1, 0 | 1 | 255) | (0, 1 | 255) | (255, 0 | 1 | 255)
        )
    }

    pub fn make_proximal(&self, col: u8, row: u8, tot: u32) -> Hit {
        let mut new = *self;
        new.row = row;
        new.col = col;
        new.tot = tot;
        new.col_offset = 0;
        new.row_offset = 0;
        new
    }

    pub fn to_cr(&self) -> u16 { (((self.col as u16) << 8)) | (self.row as u16) }

    pub fn is_dead(&self, dead_pixels: &[u16]) -> bool {
        let cr = &self.to_cr();
        dead_pixels.iter().any(|&dp| dp == self.to_cr())
    }
        // dead_pixels.iter().any(|&(dpc, dpr)| dpc == self.col as usize && dpr == self.row as usize)
        // dead_pixels.iter().any(|&dp| dp == self.to_cr())
        
    
    pub fn rasterize(&self, cfg: &image::Config, c: &Coord) -> (usize, usize) {
        let center = 127.5;
        let fcol = self.col as f64 + (self.col_offset as f64 / 255.0) - center;
        let frow = self.row as f64 + (self.row_offset as f64 / 255.0) - center;
        let xrot = center + cfg.rot_cos * fcol - cfg.rot_sin * frow;
        let yrot = center + cfg.rot_sin * fcol + cfg.rot_cos * frow;
        let icol = indexify(cfg.scale_x_fov, cfg.pixels_per_mm, xrot, c.x);
        let irow = indexify(cfg.scale_y_fov, cfg.pixels_per_mm, 255.0 - yrot, c.y);
        (icol, irow)
    }

    pub fn rotate(&self, sin: f64, cos: f64) -> (f64, f64) {
        const CENTER: f64 = 127.5;
        let fcol = self.col as f64 + (self.col_offset as f64 / 255.0) - CENTER;
        let frow = self.row as f64 + (self.row_offset as f64 / 255.0) - CENTER;
        let xrot = CENTER + cos * fcol - sin * frow;
        let yrot = CENTER - (sin * fcol + cos * frow);
        (xrot, yrot)
    }

}

pub fn indexify(fov: f64, ppmm: f64, rot: f64, coord: f64) -> usize {
    unsafe { ((coord + rot * fov) * ppmm).to_int_unchecked::<usize>() }
}
