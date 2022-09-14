use std::error::Error;

use crate::{
    mass, reader,
    stage::{Coord, Direction},
    // hit::Hit,
};

#[derive(Copy, Clone)]
pub struct Config {
    /// TOF_PULSE_LENGTH: i64 = 94_554_700; // for 1000 m/z
    /// TOF_PULSE_LENGTH: i64 = 56_687_500; // for 350 m/z
    /// TOF_PULSE_LENGTH: i64 = 70_033_985; // for 500 m/z
    /// TOF_PULSE_LENGTH: i64 = 48_276_175; // for 200 m/z
    /// TOF_PULSE_LENGTH: i64 = 56_673_605; // for 350 m/z (with high res grid)
    pub width: f64, // in mm
    pub height: f64,     // in mm
    pub rotation: f64,   // angle of rotation 2.715,2.775,2.82
    pub rot_sin: f64,    // memoized
    pub rot_cos: f64,    // memoized
    pub camera_fov: f64, // fov of pixels 330.0 / 255.0
    pub pixels_per_mm: f64,
    pub scale_x: f64,
    pub scale_y: f64,
    pub scale_x_fov: f64,      // memoized
    pub scale_y_fov: f64,      // memoized
    pub tof_pulse_length: i64, // in ps
    pub peak_time_window: i64, // in ps, time window for mass selection
    pub peak_time: Option<i64>,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            width: 0.0,  // in mm
            height: 0.0, // in mm
            rotation: 2.82,
            rot_sin: 2.82f64.sin(),
            rot_cos: 2.82f64.cos(),
            camera_fov: 345.0 / 256.0,
            pixels_per_mm: 500.0,
            scale_x: 1.0,
            scale_y: 1.0,
            scale_x_fov: 1.0 * 0.001 * 345.0 / 256.0,
            scale_y_fov: 1.0 * 0.001 * 345.0 / 256.0,
            tof_pulse_length: 0,       // i64 in ps
            peak_time_window: 100_000, // +/- 100 ns
            peak_time: None,
        }
    }
}

impl Config {
    pub fn margin_y(&self) -> usize {
        ((self.camera_fov * 0.256 + 0.025) * self.scale_y * self.pixels_per_mm) as usize
    }

    pub fn margin_x(&self) -> usize {
        ((self.camera_fov * 0.256 + 0.025) * self.scale_x * self.pixels_per_mm) as usize
    }

    pub fn cols(&self) -> u32 { (self.width * self.pixels_per_mm) as u32 + self.margin_x() as u32 }

    pub fn rows(&self) -> u32 { (self.height * self.pixels_per_mm) as u32 + self.margin_y() as u32 }

    pub fn update(&mut self) {
        let rotation = self.rotation;
        self.rot_sin = rotation.sin();
        self.rot_cos = rotation.cos();
        self.scale_x_fov = self.camera_fov * self.scale_x * 0.001;
        self.scale_y_fov = self.camera_fov * self.scale_x * 0.001;
    }
}

#[derive(Default)]
pub struct Metadata {
    pub dead_pixels: Option<Vec<u16>>, // col, row out of 256 for dead pixels
    pub coordinates: Option<Vec<Coord>>,  // x, y, direction of stage_motion travel
    pub found_peaks: Option<Vec<i64>>,    // in ps, list of peak times -> each gens 1 image
}

/// This is simply a helper struct to combine the "coordinates" that can be generated from the
/// metadata data and some semi-constants on a per-dataset basis (scale and rotation)
// #[derive(Default)]
pub struct Image {
    pub tpx3_path: std::path::PathBuf,
    pub meta: Metadata,
    pub config: Config,
}

impl Image {
    fn new(tpx3_path: std::path::PathBuf) -> Image {
        Image {
            tpx3_path,
            meta: Metadata { ..Default::default() },
            config: Config { ..Default::default() },
        }
    }

    pub fn auto_generate(&mut self) -> Result<(), Box<dyn Error>> {
        self.auto_generate_coordinates()?;
        self.auto_generate_dead_pixels()?;
        self.auto_generate_mass_list()?;
        Ok(())
    }


    /// generates coordinates only using the .tpx3/tpx3c file - assumes serpentine motion for now
    /// also assumes only left-right motion for now
    pub fn auto_generate_coordinates(&mut self) -> Result<(), Box<dyn Error>> {
        if self.meta.coordinates.is_some() { return Ok(()) }
        let pulse_passes = self.to_pulse_passes()?;
        let pass_axis_value = self.config.height; // would be self.width if top/bottom is raster
        let row_y_coords: Vec<f64> = (0..pulse_passes.len())
            .map(|y| (y as f64 / (pulse_passes.len() as f64 - 1.0)) * pass_axis_value)
            .collect();
        let (mut coords, mut direction) = (vec![], Direction::Right);
        for (i, row) in pulse_passes.iter().enumerate() {
            let (start, end) = (row.first().ok_or("no start")?, row.last().ok_or("no end")?);
            let row_time = end - start;
            let y = row_y_coords[i];
            for pulse in row.iter() {
                let time_in_row = pulse - start;
                let row_fraction = time_in_row as f64 / row_time as f64;
                let x = match direction {
                    Direction::Right => row_fraction * self.config.width, // linear interpolation
                    _ => self.config.width - row_fraction * self.config.width,
                };
                coords.push(Coord { x, y, direction });
            }
            direction = direction.reverse();
        }
        self.meta.coordinates = Some(coords);
        Ok(())
    }

    /// finds any overactive / dead pixels and provides their coordinates to allow for masking
    pub fn auto_generate_dead_pixels(&mut self) -> Result<(), Box<dyn Error>> {
        if self.meta.dead_pixels.is_none() {
            let dead_pixels = self
                .to_masking_image()?
                .iter()
                .enumerate()
                .filter(|(_, &p)| p > 7) // 7 = emperically-determined noise threshold
                .map(|(i, _)| (((i % 256) as u16) << 8) | (i / 256) as u16) // col, row
                .collect::<Vec<u16>>();
            println!("{} dead pixels found!", dead_pixels.len());
            self.meta.dead_pixels = Some(dead_pixels);
        }
        Ok(())
    }

    /// simple function to integrate and then peak pick overall mass spectrum
    pub fn auto_generate_mass_list(&mut self) -> Result<Option<Vec<i64>>, Box<dyn Error>> {
        let (times, ints) = mass::spectrum(&self.tpx3_path, Some(self.config.tof_pulse_length))?;
        self.meta.found_peaks = Some(mass::find_peaks(&ints).iter().map(|&p| times[p]).collect());
        println!("{} peaks found!", self.meta.found_peaks.as_ref().ok_or("No peaks found!")?.len());
        Ok(self.meta.found_peaks.clone())
    }

    fn to_pulse_passes(&self) -> Result<Vec<Vec<i64>>, Box<dyn Error>> {
        let (mut prev_tdc, mut pulse_rows, mut row) = (0, vec![], vec![]);
        for tdc in reader::TDCReader::new(&self.tpx3_path)? {
            if (tdc - prev_tdc) > 30_000_000_000 && prev_tdc != 0 {
                pulse_rows.push(row.clone());
                row.clear();
            }
            row.push(tdc);
            prev_tdc = tdc;
        }
        Ok(pulse_rows)
    }

    pub fn to_masking_image(&self) -> Result<Vec<usize>, Box<dyn Error>> {
        let (mut buffer, mut data_len) = (vec![0; 256 * 256], 0);
        for pulse in reader::TPX3Reader::new(&self.tpx3_path)? {
            for hit in pulse.hits.iter().filter(|h| h.size < 2) {
                buffer[hit.col as usize + (hit.row as usize * 256)] += 1;
            }
            data_len += 1;
        }
        Ok(buffer.iter().map(|v| v / (data_len / 1000)).collect())// to 2 decimals
    }

    /// to make a buffer suitable for saving directly as a .png -> useful for tic images or
    /// pairing/modifying for individual mass images
    pub fn to_buffer(&self) -> Result<Vec<u16>, Box<dyn Error>> {
        let reader = reader::TPX3Reader::new(&self.tpx3_path)?;
        let coords = self.meta.coordinates.as_ref().ok_or("Coordinates not generated")?;
        let dead_pix = self.meta.dead_pixels.as_ref().unwrap();
        let ppmm = self.config.pixels_per_mm;
        let (tpl, ptw) = (self.config.tof_pulse_length, self.config.peak_time_window);
        let (sin, cos) = (self.config.rot_sin, self.config.rot_cos);
        let (rows, cols) = (self.config.rows() as usize, self.config.cols() as usize);
        let (xfov, yfov) = (self.config.scale_x_fov, self.config.scale_y_fov);
        let mut buffer = vec![0; cols * rows];
        for (pulse, coordinates) in reader.zip(coords).filter(|(_, c)| c.is_not_inf()) {
            let (cx, cy, time) = (coordinates.x, coordinates.y, pulse.time);
            for hit in pulse.hits.iter().filter(|h| h.size > 1 || !h.is_dead(&dead_pix)) {
                let (xrot, yrot) = hit.rotate(sin, cos);
                let icol = indexify(xfov, ppmm, xrot, cx);
                let irow = indexify(yfov, ppmm, yrot, cy);
                if irow < rows && icol < cols {
                    increment_total(&mut buffer, icol, irow, cols);
                }
            }
        }
        println!("Made buffer!");
        Ok(buffer)
    }

    /// to make a buffer suitable for saving directly as a .png -> useful for tic images or
    /// pairing/modifying for individual mass images
    pub fn times_to_buffers(&self, pts: &[i64]) -> Result<Vec<u16>, Box<dyn Error>> {
        let reader = reader::TPX3Reader::new(&self.tpx3_path)?;
        let coords = self.meta.coordinates.as_ref().expect("coordinates not generated!");
        let (dead_pix, cfg) = (self.meta.dead_pixels.as_ref().unwrap(), self.config);
        let ppmm = cfg.pixels_per_mm;
        let (tpl, ptw) = (cfg.tof_pulse_length as i32, cfg.peak_time_window as u64);
        let (sin, cos) = (cfg.rot_sin, cfg.rot_cos);
        let (rows, cols) = (cfg.rows() as usize, cfg.cols() as usize);
        let (xfov, yfov) = (cfg.scale_x_fov, cfg.scale_y_fov);
        let mut buffers = vec![0; cols * rows * pts.len()];
        for (pulse, coordinates) in reader.zip(coords) {
            let (cx, cy, time) = (coordinates.x, coordinates.y, pulse.time);
            for hit in pulse.hits.iter().filter(|h| h.size > 1 || !h.is_dead(&dead_pix)) {
                let t = ((hit.toa - time) as i32 % tpl) as u64; // i32 shaves off time
                for (j, _) in pts.iter().enumerate().filter(|(_, &pt)| betwix(t, pt as u64, ptw)) {
                    let (xrot, yrot) = hit.rotate(sin, cos);
                    let icol = indexify(xfov, ppmm, xrot, cx);
                    let irow = indexify(yfov, ppmm, yrot, cy);
                    if irow < rows && icol < cols {
                        increment(&mut buffers, icol, irow, cols, cols * rows, j);
                    }
                }
            }
        }
        println!("Made buffers!");
        Ok(buffers)
    }
}

fn increment(buffers: &mut Vec<u16>, icol: usize, irow: usize, cols: usize, cr: usize, i: usize) {
    unsafe { *buffers.get_unchecked_mut(make_index(icol, irow, cols, cr, i)) += 1; }
}

fn increment_total(buffers: &mut Vec<u16>, icol: usize, irow: usize, cols: usize) {
    unsafe { *buffers.get_unchecked_mut(icol.unchecked_add(irow.unchecked_mul(cols))) += 1; }
}

fn indexify(fov: f64, ppmm: f64, rot: f64, coord: f64) -> usize {
    unsafe { ((coord + rot * fov) * ppmm).to_int_unchecked::<usize>() }
}

fn make_index(icol: usize, irow: usize, cols: usize, cr: usize, j: usize) -> usize {
    unsafe { icol.unchecked_add(irow.unchecked_mul(cols)).unchecked_add(cr.unchecked_mul(j)) }
}

pub fn betwix(v: u64, pt: u64, ptw: u64) -> bool {
    unsafe { (v).wrapping_sub(pt.unchecked_sub(ptw)) < ptw.unchecked_add(ptw) }
}

pub fn is_between(value: i64, high: i64, low: i64) -> bool { value < high && value > low }


