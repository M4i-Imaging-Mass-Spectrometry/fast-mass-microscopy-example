#![allow(dead_code)]
// #![allow(unused_imports)]
#![allow(unused_must_use)]
#![allow(unused_mut)]
#![allow(unused_variables)]
// #![allow(unused_parens)]
#![feature(unchecked_math)]


use std::error::Error;

use rayon::prelude::*;

mod hit;
mod image;
mod imzml;
mod mass;
mod math;
mod pixel;
mod pulse;
mod reader;
mod stage;
mod writer;

fn main() -> Result<(), Box<dyn Error>> {
    let now = std::time::Instant::now();
    let current_dir = std::env::current_dir()?;
    for entry in std::fs::read_dir(current_dir)?.filter_map(Result::ok) {
        let path = entry.path();
        let tof_pulse_length = 56_673_605;

        // // For conversion of .tpx3 files to .tpx3c files
        // if path.extension() == Some(&std::ffi::OsString::from("tpx3")) {
        //     let now = std::time::Instant::now();
        //     writer::centroid_cluster_compress(&path)?;
        //     println!("centroiding took {} ms", now.elapsed().as_millis());
        // }

        // For quick plotting of the mass spectrum
        if path.extension() == Some(&std::ffi::OsString::from("tpx3c")) {
            let now = std::time::Instant::now();
            writer::plotly_spectra(&path, Some(tof_pulse_length));
            println!("plotly took {} ms", now.elapsed().as_millis());
        }
        let now = std::time::Instant::now();
        if path.extension() == Some(&std::ffi::OsString::from("tpx3c")) {
            // setup configuration options for the image
            let mut config = image::Config {
                width: 4.0,                   // dimension of the image in mm
                height: 2.75,                 // dimension of the image in mm
                pixels_per_mm: 200.0,         // desired pixel visualization size; 500 is 2 micrometer pixels
                rotation: 280.5 / 100.0,      // mounting angle of rotation of TPX3CAM
                scale_x: 1.0,                 // distortion scalar in x direction (1.0 is no distortion)
                scale_y: 1.0,                 // distortion scalar in y direction (1.0 is no distortion)
                camera_fov: 395.0 / 256.0,    // ratio of pixels to field-of-view
                tof_pulse_length, // time-of-flight repetition rate (m/z dependant)
                ..Default::default()
            };

            config.update();
            // make the image structure
            let mut image_data = image::Image {
                tpx3_path: path.clone(),
                config,
                meta: image::Metadata { ..Default::default() },
            };

            image_data.auto_generate_coordinates().unwrap();
            image_data.auto_generate_dead_pixels().unwrap();
            image_data.auto_generate_mass_list()?.unwrap();

            let base_name = path.file_stem().unwrap().to_str().unwrap();
            let fname = path.with_file_name(base_name.to_owned() + &format!("_tic.png"));
            let buffer = image_data.to_buffer().unwrap();
            // save total ion count image
            writer::save_png(&buffer, config.cols(), config.rows(), &fname);
            let masses = image_data.meta.found_peaks.take().unwrap();
            let masses: Vec<Vec<i64>> = masses.chunks(6).map(|a| a.to_vec()).collect();
            masses.par_iter().for_each(|peak_times|
                {
                    let now = std::time::Instant::now();
                    let mass_image = image::Image {
                        tpx3_path: path.clone(),
                        config: image::Config {
                            peak_time: None,
                            peak_time_window: 150_000, // +/- 150 ns
                            ..config
                        },
                        meta: image::Metadata {
                            coordinates: image_data.meta.coordinates.clone(),
                            dead_pixels: image_data.meta.dead_pixels.clone(),
                            ..Default::default()
                        },
                    };
                    let buffers = mass_image.times_to_buffers(peak_times).unwrap();
                    for (i, pt) in peak_times.iter().enumerate() {
                        let mz = mass::time_to_mass(*pt);
                        let fname = path.with_file_name(base_name.to_owned() + &format!("_{:.1$}mz.png", mz, 1));
                        let page = (config.cols() * config.rows()) as usize;
                        let (start, end) = (page * i, page * (i+1));
                        writer::save_png(&buffers[start..end], config.cols(), config.rows(), &fname);
                    }
                }
            );
            // to generate an imzml file, uncomment the next two commented lines:

            // let mut imzml_data = imzml::IMZMLMaker::new(image_data)?;
            // imzml_data.stream_convert_and_save();
            println!("processing took {} s", now.elapsed().as_secs());
        }


    }
    Ok(())
}
