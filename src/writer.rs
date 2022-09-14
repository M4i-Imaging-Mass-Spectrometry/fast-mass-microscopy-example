use std::{
    error::Error,
    io::{BufWriter, Write},
};

use crate::{image, mass, pulse, reader};
use itertools::Itertools;
use plotly::{
    common::{Mode, Title},
    layout::Axis,
    Layout, Plot, Scatter,
};
use rayon::prelude::*;

/// writes a centroided .tpx3c file, requires a path as it is streaming
pub fn centroid_cluster_compress(path: &std::path::Path) -> Result<(), Box<dyn Error>> {
    let mut buffer = std::fs::File::create(path.with_extension("tpx3c"))?;
    let data = reader::TPX3Reader::new(path)?;
    let mut shots = 0;
    for shot in data.chunks(500).into_iter() {
        let mut collection = shot.collect::<Vec<pulse::Pulse>>();
        collection.par_iter_mut().for_each(|p| p.label_hits());
        let centroided =
            collection.par_iter().flat_map(|p| p.centroid().to_bytes()).collect::<Vec<u8>>();
        buffer.write_all(&centroided);
    }
    println!("shots = {}", shots);
    Ok(())
}

/// saves a buffer to a png with a width and a height (h) at a path
pub fn save_png(buf: &[u16], w: u32, h: u32, path: &std::path::Path) -> Result<(), Box<dyn Error>> {
    let (max, min) = (*buf.iter().max().unwrap() as f64, *buf.iter().min().unwrap() as f64);
    println!("saving png: maximum pixel value {} {:?}", max, &path);
    let data: Vec<u8> =
        buf.iter().flat_map(|i| ((((*i as f64) / max) * 65530.0) as u16).to_be_bytes()).collect();
    let buf_writer = &mut BufWriter::new(std::fs::File::create(path.with_extension("png"))?);
    let mut encoder = png::Encoder::new(buf_writer, w, h);
    encoder.set_color(png::ColorType::Grayscale);
    encoder.set_depth(png::BitDepth::Sixteen);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(&data)?;
    Ok(())
}

pub fn plotly_spectra(path: &std::path::Path, tof_len: Option<i64>) -> Result<(), Box<dyn Error>> {
    let base_name = path.file_stem().unwrap().to_str().unwrap();
    let spectrum_file = path.with_file_name(base_name.to_owned() + "_report_spectrum.html");
    let mut plot = Plot::new();
    let layout = Layout::new()
        .x_axis(Axis::new().title(Title::new("Time (ns)")))
        .y_axis(Axis::new().title(Title::new("Pixels activated")));
    plot.set_layout(layout);
    // this is backwards -> TODO: we should pass the data to this function
    let (time_axis, intensity_axis) = mass::spectrum(path, tof_len)?;
    let (time_axis, intensity_axis) = mass::zero_pad(&time_axis, &intensity_axis);
    let trace1 = Scatter::new(time_axis.clone(), intensity_axis.clone())
        .name("Full spectrum")
        .mode(Mode::Lines);
    plot.add_trace(trace1);
    let full_csv_file = path.with_file_name(base_name.to_owned() + "_report_full_spectrum.csv");
    let csv_strings: Vec<String> =
        time_axis.iter().zip(&intensity_axis).map(|(t, i)| format!("{},{}", t, i)).collect();
    let mut file = std::fs::File::create(full_csv_file).unwrap(); // scope / file dropped at end of fn
    writeln!(file, "{}", csv_strings.join("\n")).unwrap();
    plot.to_html(spectrum_file);
    Ok(())
}

pub fn save_masking_image(path: &std::path::Path) -> Result<(), Box<dyn Error>> {
    let image = image::Image {
        tpx3_path: path.to_path_buf(),
        meta: image::Metadata { ..Default::default() },
        config: image::Config { ..Default::default() },
    };
    let buffer = image.to_masking_image()?;
    let file = std::fs::File::create(path.with_extension("png"))?;
    let w = &mut BufWriter::new(file);
    let mut encoder = png::Encoder::new(w, 256, 256);
    encoder.set_color(png::ColorType::Grayscale);
    encoder.set_depth(png::BitDepth::Sixteen);
    let mut writer = encoder.write_header().unwrap();
    let max: f64 = *buffer.iter().max().unwrap() as f64;
    let data: Vec<u8> = buffer
        .iter()
        .flat_map(|i| {
            let value = ((*i as f64) / (max)) * 65530.0;
            (value as u16).to_be_bytes()
        })
        .collect();
    writer.write_image_data(&data).unwrap();
    Ok(())
}
