use crate::{math, reader};
use std::{collections::HashMap, error::Error};

const TIME_BIN_WIDTH: i64 = 1563; // ps to bins (decimal loss from 1.5625, but is hash)

/// Takes a tpx3 or tpx3c file path and a pulse length to produce a spectrum
/// The tof_pulse_length is the length of the 'true' tof cycle (in ps)
/// TODO: if we decrease the processing cost of TDCs (and then do not need to
/// double or multi-up the pulses for each TDC) then we can simplify this
/// by removing the pulse length parameter
/// We might add a time correction / calibration factor here
/// const TOF_PULSE_LENGTH: i64 = 94_554_700; // 1000 m/z
/// const TOF_PULSE_LENGTH: i64 = 70_033_985; // 500 m/z
/// const TOF_PULSE_LENGTH: i64 = 56_687_500; // 350 m/z
/// const TOF_PULSE_LENGTH: i64 = 48_276_175; // 200 m/z
pub fn spectrum(
    tpx3_path: &std::path::Path, tof_pulse_length: Option<i64>,
) -> Result<(Vec<i64>, Vec<u32>), Box<dyn Error>> {
    let data = reader::TPX3Reader::new(tpx3_path)?;
    let mut map: HashMap<i64, u32, nohash_hasher::BuildNoHashHasher<i64>> =
        (0..1).map(|i| (i as i64, i as u32)).collect();
    let now = std::time::Instant::now();
    for pulse in reader::TPX3Reader::new(tpx3_path)? {
        for hit in pulse.hits.iter() {
            let tof = (hit.toa - pulse.time) % tof_pulse_length.unwrap_or(i64::MAX);
            if tof < 0 { // remove any negative TOF values due to TPX3 firmware issue
                continue;
            }
            let index: i64 = (tof / TIME_BIN_WIDTH) * TIME_BIN_WIDTH;
            let count = map.entry(index).or_insert(0);
            *count += 1;
        }
    }
    println!("building hashmap took {} ms", now.elapsed().as_millis());
    // now that we've extracted the data, sort it to spectrum based on time
    let now = std::time::Instant::now();
    let mut pairs: Vec<(i64, u32)> = map.iter().map(|(a, b)| (*a, *b)).collect();
    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    println!("sorting hashmap took {} ms", now.elapsed().as_millis());
    let (mut times, mut intensities) = (vec![], vec![]);
    for (time_index, intensity) in pairs.iter() {
        times.push(*time_index);
        intensities.push(*intensity);
    }
    Ok((times, intensities))
}

/// adds zeros to starts and ends of peaks to allow for easy plotting of mass spectra
pub fn zero_pad(times: &[i64], intensities: &[u32]) -> (Vec<i64>, Vec<u32>) {
    let mut prev_time: i64 = *times.first().unwrap();
    let (mut pad_time, mut pad_intensity) = (vec![prev_time - TIME_BIN_WIDTH], vec![0]);
    for (&time, &intensity) in times.iter().zip(intensities) {
        if time - prev_time != TIME_BIN_WIDTH {
            pad_time.push(prev_time + TIME_BIN_WIDTH);
            pad_time.push(time - TIME_BIN_WIDTH);
            pad_intensity.push(0);
            pad_intensity.push(0);
        }
        pad_time.push(time);
        pad_intensity.push(intensity);
        prev_time = time;
    }
    pad_time.push(pad_time.last().unwrap() + TIME_BIN_WIDTH);
    pad_intensity.push(0);
    (pad_time, pad_intensity)
}

pub fn find_peaks(chromatogram: &[u32]) -> Vec<usize> {
    let diff: Vec<f64> = chromatogram.windows(2).map(|a| a[1] as f64 - a[0] as f64).collect();
    let wind = 15;
    let smooth = math::smooth(&diff, wind);
    let smooth = math::smooth(&smooth, wind);
    let mut peaks = vec![];
    for i in 0..(smooth.len() - 1) {
        let this = smooth[i];
        let next = smooth[i + 1];
        if this > 0.0
            && next < 0.0
            && i > wind + 3
            && this - next >= 0.7
            && chromatogram[i + wind + 7] as f64 > 5000.0
        {
            peaks.push(i + math::argmax_u32(&chromatogram[i..i + 2 * wind]).0);
        }
    }
    peaks
}


/// this equation is simply a quick conversion function and does not represent a calibrated mass function
pub fn time_to_mass(time: i64) -> f64 {
    // 0.0461 * ((time as f64) / 1_000_000.0).powf(2.1782)
    let x = time as f64 / 1_000_000.0;
    0.139 * x.powf(2.0) - 1.413 * x + 3.686
} // HOT FUNCTION -> WORK TO OPTIMIZE!!!
// y2=0.139*x.^2-1.413*x+3.686