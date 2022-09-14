use std::{
    collections::HashMap,
    convert::TryInto,
    error::Error,
    io::{Read, Seek, SeekFrom, Write},
    num::ParseIntError,
};

use sha1::{Digest, Sha1};
use simple_uuid::v4;

use crate::{
    image, mass,
    pixel::{Pixel, PixelSpan},
    reader::TPX3Reader,
    stage::Direction,
};


const IMZML_FOOTER: &str = r#"        
        </spectrumList>
    </run>
</mzML>"#;

pub struct IMZMLMaker {
    pub image: image::Image,
    pub header: IMZMLHeader,
    pub ibd_file: std::fs::File,
    pub imzml_file: std::fs::File,
    pub index: usize,  // counter that imzml requires as an index for each spectrum
    pub offset: usize, // keeps track of the offset in the .ibd file for imzml
    pub low_crop_row: usize, // if no crop, make 0
    pub high_crop_row: usize, // if no crop, make super large
    pub low_crop_col: usize, // if no crop, make 0
    pub high_crop_col: usize, // if no crop, make super large
}

impl IMZMLMaker {
    pub fn new(image: image::Image) -> Result<IMZMLMaker, Box<dyn Error>> {
        let low_crop_row = 140 / 5; // if no crop, make 0
        let high_crop_row = 1265 / 5; // if no crop, make super large
        let low_crop_col = 155 / 5; // if no crop, make 0
        let high_crop_col = 2025 / 5; // if no crop, make super large
        let low_crop_row = 0;
        let high_crop_row = 10000;
        let low_crop_col = 0;
        let high_crop_col = 10000;
        let (xs, ys) = (image.config.cols(), image.config.rows());
        let pixel_size = 1000.0 / image.config.pixels_per_mm;
        if low_crop_row > 0 && low_crop_col > 0 {
            assert!(high_crop_col < xs && high_crop_row < ys);
            assert!(low_crop_col < high_crop_col && low_crop_row < high_crop_row);
            let (xs, ys) = (high_crop_col - low_crop_col, high_crop_row - low_crop_row);
        }
        let header = IMZMLHeader {
            uuid: v4!().replace('-', ""),
            x_pixel_maximum: format!("{xs}"),
            y_pixel_maximum: format!("{ys}"),
            width_micron: format!("{}", (xs as f64 * pixel_size) as u32),
            height_micron: format!("{}", (ys as f64 * pixel_size) as u32),
            x_pixel_size: format!("{pixel_size}"), // pixel size in micrometers as floating
            y_pixel_size: format!("{pixel_size}"), // pixel size in micrometers as floating
            number_of_spectra: format!("{}", xs * ys), // The total number of "spectra" or pixels
            ..Default::default()
        };
        let ibd_file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(image.tpx3_path.with_extension("ibd"))?;
        let imzml_file = std::fs::File::create(image.tpx3_path.with_extension("imzml"))?;
        Ok(IMZMLMaker {
            image,
            header,
            ibd_file,
            imzml_file,
            index: 0,
            offset: 16,
            low_crop_row: low_crop_row as usize,
            high_crop_row: high_crop_row as usize,
            low_crop_col: low_crop_col as usize,
            high_crop_col: high_crop_col as usize,
        })
    }

    /// turns the header uuid into a set of bytes to write
    pub fn uuid_as_bytes(&self) -> Result<Vec<u8>, ParseIntError> {
        (0..32).step_by(2).map(|i| u8::from_str_radix(&self.header.uuid[i..i + 2], 16)).collect()
    }

    /// generates a sha1 checksum for the ibd file -> only call this after IBD has been written!!
    pub fn ibd_to_sha1(&mut self) -> Result<String, Box<dyn Error>> {
        const BUFFER_SIZE: usize = 1024;
        let (mut sh, mut buffer) = (Sha1::default(), [0u8; BUFFER_SIZE]);
        self.ibd_file.seek(SeekFrom::Start(0))?;
        // maybe I need to panic here instead of using while let...
        while let Ok(bytes_read) = self.ibd_file.read(&mut buffer) {
            sh.update(&buffer[..bytes_read]);
            if bytes_read < BUFFER_SIZE {
                break;
            }
        }
        Ok(sh.finalize().iter().map(|b| format!("{:02x}", b)).collect())
    }

    /// streams through a TPX3Reader, rasterizing it and converting it to imzml
    pub fn stream_convert_and_save(&mut self) -> Result<(), Box<dyn Error>> {
        let reader = TPX3Reader::new(&self.image.tpx3_path)?;
        let col_count = self.image.config.cols() as usize;
        let row_count = self.image.config.rows() as usize;
        let coords = self.image.meta.coordinates.take().ok_or("Coordinates not present!")?;
        self.imzml_file.write_all(self.header.to_string().as_bytes());
        self.ibd_file.write_all(&self.uuid_as_bytes()?); // first 16 bits
        let mut spans: HashMap<usize, PixelSpan> = HashMap::new(); // key is row index
        let mut rows_written: Vec<usize> = vec![];
        let mut direction = Direction::Right; // to determine if there is a new pass
        let mut count = 0;
        let mut max_pixel = 0;
        let dead_pix = self.image.meta.dead_pixels.clone();
        let dead_pix = dead_pix.as_ref().unwrap();
        for (pulse, coordinates) in reader.zip(&coords).filter(|(_, c)| c.is_not_inf()) {
            for hit in pulse.hits.iter().filter(|h| !h.is_dead(dead_pix)) {
                let (col, row) = hit.rasterize(&self.image.config, coordinates);
                let tof_ps = (hit.toa - pulse.time) % self.image.config.tof_pulse_length;
                let mz = mass::time_to_mass(tof_ps) as f32;
                if row < row_count && col < col_count && mz > 0.0 && mz < 300.0 {
                    let row = spans.entry(row).or_insert_with(|| PixelSpan::empty(col_count));
                    row.add_mz(mz, col);
                }
            }
            if coordinates.direction != direction {
                direction = coordinates.direction;
                spans.iter_mut().for_each(|(_, pixel_span)| pixel_span.update_end_pass());
                let mut finished_rows: Vec<usize> =
                    spans.iter().filter(|(_, v)| v.empty_pass_count > 2).map(|(k, _)| *k).collect();
                finished_rows.sort_unstable();
                let max_pix =
                    self.scan_write_rows(&mut spans, &mut rows_written, &finished_rows)?;
                if max_pix > max_pixel {
                    max_pixel = max_pix
                }
            }
            count += 1;
        }
        println!("rows read: {count}");
        let mut final_rows: Vec<usize> = spans.iter().map(|(k, _)| *k).collect();
        final_rows.sort_unstable();
        let max_pix = self.scan_write_rows(&mut spans, &mut rows_written, &final_rows)?;
        if max_pix > max_pixel {
            max_pixel = max_pix;
        }
        println!("The maximum intensity of a pixel is {}", max_pixel);
        self.imzml_file.write_all(IMZML_FOOTER.to_string().as_bytes());
        self.overwrite_header_with_sha1_checksum()?;
        Ok(())
    }

    pub fn scan_write_rows(
        &mut self, spans: &mut HashMap<usize, PixelSpan>, rows_written: &mut Vec<usize>,
        rows: &[usize],
    ) -> Result<usize, Box<dyn Error>> {
        let mut max_pixel = 0;
        for &row in rows.iter() {
            if rows_written.iter().any(|&i| i == row) {
                panic!("Attempting to write row {} twice!", row);
            }
            rows_written.push(row); // this catch won't work now probably due to the cropping; TODO: Update this
            let mut extracted_row = spans.remove(&row).ok_or("no row to remove!")?;
            if row >= self.low_crop_row && row < self.high_crop_row {
                extracted_row.pixels.iter_mut().enumerate().for_each(|(col, pixel)| {
                    if col >= self.low_crop_col && col < self.high_crop_col {
                        let max = self.write_spectrum(
                            pixel,
                            col - self.low_crop_col,
                            row - self.low_crop_row,
                        );
                        if max > max_pixel {
                            max_pixel = max; // this is just a counter for printing not something used in logic
                        }
                    }
                });
            }
        }
        Ok(max_pixel)
    }

    pub fn write_spectrum(&mut self, pixel: &mut Pixel, col: usize, row: usize) -> usize {
        let (mzs, ints): (Vec<f32>, Vec<i16>) = pixel.to_vecs();
        let maximum_int = *ints.iter().max().unwrap_or(&0) as usize;
        let mzs_bytes: Vec<u8> = mzs.iter().flat_map(|m| m.to_le_bytes()).collect();
        let ints_bytes: Vec<u8> = ints.iter().flat_map(|i| i.to_le_bytes()).collect();
        let reverse_ints_bytes: Vec<i16> = ints_bytes
            .chunks(2)
            .map(|i| i16::from_le_bytes(i.try_into().expect("slice with incorrect length")))
            .collect::<Vec<i16>>();
        for &i in reverse_ints_bytes.iter() {
            assert!(i > 0);
        }
        self.ibd_file.write_all(&mzs_bytes);
        self.ibd_file.write_all(&ints_bytes);
        let (mz_enc_len, int_enc_len) = (mzs_bytes.len(), ints_bytes.len());
        let spectrum = IMZMLSpectrum {
            index: self.index,
            spectrum_sum: ints.iter().sum::<i16>() as u16,
            pixel_column: (col + 1) as u32, // we add 1 due to IMZML spec
            pixel_row: (row + 1) as u32,    // we add 1 due to IMZML spec
            mz_len: mzs.len(),
            mz_offset: self.offset, // starting offset
            mz_enc_len,
            int_len: ints.len(),
            int_offset: self.offset + mz_enc_len,
            int_enc_len,
        };
        self.imzml_file.write_all(spectrum.to_string().as_bytes());
        self.offset = self.offset + mz_enc_len + int_enc_len;
        self.index += 1;
        maximum_int
    }

    /// once everything is finished with the .ibd file, we need to fill in a correct checksum
    /// from our dummy checksum; although this is wasteful, the headers are pretty small and so
    /// overwriting them is a bit easier than finding the checksum and just overwriting that
    pub fn overwrite_header_with_sha1_checksum(&mut self) -> Result<(), Box<dyn Error>> {
        self.header.sha1sum = self.ibd_to_sha1()?; // add real checksum to header struct
        println!("checksum: {}", self.header.sha1sum);
        let overwrite_header = self.header.to_string(); // regenerate header string
        self.imzml_file.seek(SeekFrom::Start(0))?;
        self.imzml_file.write_all(overwrite_header.as_bytes())?; // write it to the start
        Ok(())
    }
}


pub struct IMZMLHeader {
    uuid: String,
    sha1sum: String,
    x_pixel_maximum: String,
    y_pixel_maximum: String,
    run_id: String,
    x_pixel_size: String,
    y_pixel_size: String,
    width_micron: String,
    height_micron: String,
    number_of_spectra: String,
    mz_data_type: String,
    obo_codes_mz_data_type: String,
    int_data_type: String,
    obo_codes_int_data_type: String,
    scan_direction: String,
    obo_codes_scan_direction: String,
    scan_pattern: String,
    obo_codes_scan_pattern: String,
    scan_type: String,
    obo_codes_scan_type: String,
    line_scan_direction: String,
    obo_codes_line_scan_direction: String,
    mode: String,
    obo_codes_mode: String,
    mz_compression: String,
    obo_codes_mz_compression: String,
    int_compression: String,
    obo_codes_int_compression: String,
    polarity: String,
    obo_codes_polarity: String,
}


impl Default for IMZMLHeader {
    fn default() -> IMZMLHeader {
        IMZMLHeader {
            uuid: "0".to_string(),
            sha1sum: "a_dummy_checksum_that_should_be_replaced".to_string(), // is 40 characters
            x_pixel_maximum: "0".to_string(),
            y_pixel_maximum: "0".to_string(),
            width_micron: "0".to_string(),
            height_micron: "0".to_string(),
            run_id: "Experiment0".to_string(),
            x_pixel_size: "1.0".to_string(), // pixel size in micrometers as floating
            y_pixel_size: "1.0".to_string(), // pixel size in micrometers as floating
            number_of_spectra: "0".to_string(), // The total number of "spectra" or pixels
            mz_data_type: "32-bit float".to_string(),
            obo_codes_mz_data_type: "MS:1000521".to_string(),
            // int_data_type: "32-bit float".to_string(),
            // obo_codes_int_data_type: "MS:1000521".to_string(),
            int_data_type: "16-bit integer".to_string(),
            obo_codes_int_data_type: "IMS:1100001".to_string(),
            scan_direction: "top down".to_string(),
            obo_codes_scan_direction: "IMS:1000401".to_string(),
            scan_pattern: "meandering".to_string(), //"flyback"
            obo_codes_scan_pattern: "IMS:1000410".to_string(), // IMS:1000413
            scan_type: "horizontal line scan".to_string(),
            obo_codes_scan_type: "IMS:1000480".to_string(),
            line_scan_direction: "linescan left right".to_string(),
            obo_codes_line_scan_direction: "IMS:1000491".to_string(),
            mode: "processed".to_string(),
            obo_codes_mode: "IMS:1000031".to_string(),
            mz_compression: "no compression".to_string(),
            obo_codes_mz_compression: "MS:1000576".to_string(),
            int_compression: "no compression".to_string(),
            obo_codes_int_compression: "MS:1000576".to_string(),
            polarity: "positive scan".to_string(), // "negative scan"
            obo_codes_polarity: "MS:1000130".to_string(), // "MS:1000129"
        }
    }
}

/// These parameters satisfy the IMZML specification but are not necessarily correct for our instrument. TODO: update with correct instrument specifications.
impl std::fmt::Display for IMZMLHeader {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            r#"<?xml version="1.0" encoding="ISO-8859-1"?>
<mzML xmlns="http://psi.hupo.org/ms/mzml" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:schemaLocation="http://psi.hupo.org/ms/mzml http://psidev.info/files/ms/mzML/xsd/mzML1.1.0_idx.xsd" version="1.1">
    <cvList count="2">
        <cv URI="http://ontologies.berkeleybop.org/pato.obo" fullName="Phenotype And Trait Ontology" id="PATO" version="releases/2017-07-10"/>
        <cv URI="http://ontologies.berkeleybop.org/uo.obo" fullName="Units of Measurement Ontology" id="UO" version="releases/2017-09-25"/>
        <cv URI="https://raw.githubusercontent.com/hupo-psi/psi-ms-cv/master/psi-ms.obo" fullName="Proteomics Standards Initiative Mass Spectrometry Ontology" id="MS" version="4.1.0"/>
        <cv URI="https://raw.githubusercontent.com/imzML/imzML/master/imagingMS.obo" fullName="Mass Spectrometry Imaging Ontology" id="IMS" version="1.1.0"/>
    </cvList>
<fileDescription>
    <fileContent>
        <cvParam cvRef="MS" accession="MS:1000579" name="MS1 spectrum" value=""/>
        <cvParam cvRef="MS" accession="MS:1000128" name="profile spectrum" value=""/>
        <cvParam cvRef="IMS" accession="{obo_codes_mode}" name="{mode}" value=""/>
        <cvParam cvRef="IMS" accession="IMS:1000080" name="universally unique identifier" value="{uuid}"/>
        <cvParam cvRef="IMS" accession="IMS:1000091" name="ibd SHA-1" value="{sha1sum}"/>
    </fileContent>
</fileDescription>
<referenceableParamGroupList count="4">
    <referenceableParamGroup id="mzArray">
        <cvParam cvRef="MS" accession="{obo_codes_mz_compression}" name="{mz_compression}" value=""/>
        <cvParam cvRef="MS" accession="MS:1000514" name="m/z array" unitCvRef="MS" unitAccession="MS:1000040" unitName="m/z"/>
        <cvParam cvRef="MS" accession="{obo_codes_mz_data_type}" name="{mz_data_type}" value=""/>
        <cvParam cvRef="IMS" accession="IMS:1000101" name="external data" value="true"/>
    </referenceableParamGroup>
    <referenceableParamGroup id="intensityArray">
        <cvParam cvRef="IMS" accession="{obo_codes_int_data_type}" name="{int_data_type}" value=""/>
        <cvParam cvRef="MS" accession="MS:1000515" name="intensity array" unitCvRef="MS" unitAccession="MS:1000131" unitName="number of detector counts"/>
        <cvParam cvRef="MS" accession="{obo_codes_int_compression}" name="{int_compression}" value=""/>
        <cvParam cvRef="IMS" accession="IMS:1000101" name="external data" value="true"/>
    </referenceableParamGroup>
    <referenceableParamGroup id="scan1">
        <cvParam cvRef="MS" accession="MS:1000093" name="increasing m/z scan"/>
        <cvParam cvRef="MS" accession="MS:1000512" name="filter string" value=""/>
    </referenceableParamGroup>
    <referenceableParamGroup id="spectrum1">
        <cvParam cvRef="MS" accession="MS:1000579" name="MS1 spectrum" value=""/>
        <cvParam cvRef="MS" accession="MS:1000511" name="ms level" value="0"/>
        <cvParam cvRef="MS" accession="MS:1000128" name="profile spectrum" value=""/>
        <cvParam cvRef="MS" accession="{obo_codes_polarity}" name="{polarity}" value=""/>
    </referenceableParamGroup>
</referenceableParamGroupList>
<softwareList count="1">
    <software id="tpx3_to_imzml" version="0.1">
        <cvParam cvRef="MS" accession="MS:1000799" name="custom unreleased software tool" value="tpx3 to imzml converter"/>
    </software>
</softwareList>
<scanSettingsList count="1">
    <scanSettings id="scanSettings1">
        <cvParam cvRef="IMS" accession="{obo_codes_scan_direction}" name="{scan_direction}"/>
        <cvParam cvRef="IMS" accession="{obo_codes_scan_pattern}" name="{scan_pattern}"/>
        <cvParam cvRef="IMS" accession="{obo_codes_scan_type}" name="{scan_type}"/>
        <cvParam cvRef="IMS" accession="{obo_codes_line_scan_direction}" name="{line_scan_direction}"/>
        <cvParam cvRef="IMS" accession="IMS:1000042" name="max count of pixels x" value="{x_pixel_maximum}"/>
        <cvParam cvRef="IMS" accession="IMS:1000043" name="max count of pixels y" value="{y_pixel_maximum}"/>
        <cvParam cvRef="IMS" accession="IMS:1000044" name="max dimension x" value="{width_micron}" unitCvRef="UO" unitAccession="UO:0000017" unitName="micrometer"/>
        <cvParam cvRef="IMS" accession="IMS:1000045" name="max dimension y" value="{height_micron}" unitCvRef="UO" unitAccession="UO:0000017" unitName="micrometer"/>
        <cvParam cvRef="IMS" accession="IMS:1000046" name="pixel size (x)" value="{x_pixel_size}" unitCvRef="UO" unitAccession="UO:0000017" unitName="micrometer"/>
        <cvParam cvRef="IMS" accession="IMS:1000047" name="pixel size y" value="{y_pixel_size}" unitCvRef="UO" unitAccession="UO:0000017" unitName="micrometer"/>
    </scanSettings>
</scanSettingsList>
<instrumentConfigurationList count="1">
    <instrumentConfiguration id="IC1">
        <cvParam cvRef="MS" accession="MS:1000557" name="Trift II BioTRIFT"/>
        <cvParam cvRef="MS" accession="MS:1000529" name="instrument serial number" value="none"/>
        <componentList count="3">
        <source order="1">
            <cvParam cvRef="MS" accession="MS:1000073" name="electrospray ionization"/>
            <cvParam cvRef="MS" accession="MS:1000485" name="nanospray inlet"/>
            <cvParam cvRef="MS" accession="MS:1000844" name="focus diameter x" value="10.0"/>
            <cvParam cvRef="MS" accession="MS:1000845" name="focus diameter y" value="10.0"/>
            <cvParam cvRef="MS" accession="MS:1000846" name="pulse energy" value="10.0"/>
            <cvParam cvRef="MS" accession="MS:1000847" name="pulse duration" value="10.0"/>
            <cvParam cvRef="MS" accession="MS:1000848" name="attenuation" value="50.0"/>
            <cvParam cvRef="MS" accession="MS:1000850" name="gas laser"/>
            <cvParam cvRef="MS" accession="MS:1000836" name="dried droplet MALDI matrix preparation"/>
            <cvParam cvRef="MS" accession="MS:1000835" name="matrix solution concentration" value="10.0"/>
            <cvParam cvRef="MS" accession="MS:1000834" name="matrix solution" value="DHB"/>
        </source>
        <analyzer order="2">
            <cvParam cvRef="MS" accession="MS:1000264" name="ion trap"/>
            <cvParam cvRef="MS" accession="MS:1000014" name="accuracy" value="0.0" unitCvRef="MS" unitAccession="MS:1000040" unitName="m/z"/>
        </analyzer>
        <detector order="3">
            <cvParam cvRef="MS" accession="MS:1000253" name="electron multiplier"/>
            <cvParam cvRef="MS" accession="MS:1000120" name="transient recorder"/>
        </detector>
        </componentList>
    </instrumentConfiguration>
</instrumentConfigurationList>
<dataProcessingList count="1">
    <dataProcessing id="export_from_tpx3_to_imzml">
        <processingMethod order="1" softwareRef="tpx3_to_imzml">
            <cvParam cvRef="MS" accession="MS:1000544" name="Conversion to mzML" value=""/>
        </processingMethod>
    </dataProcessing>
</dataProcessingList>
<run defaultInstrumentConfigurationRef="IC1" id="{run_id}">
    <spectrumList count="{number_of_spectra}" defaultDataProcessingRef="export_from_tpx3_to_imzml">
"#,
            uuid = self.uuid,
            sha1sum = self.sha1sum,
            x_pixel_maximum = self.x_pixel_maximum,
            y_pixel_maximum = self.y_pixel_maximum,
            run_id = self.run_id,
            x_pixel_size = self.x_pixel_size,
            y_pixel_size = self.y_pixel_size,
            width_micron = self.width_micron,
            height_micron = self.height_micron,
            number_of_spectra = self.number_of_spectra,
            mz_data_type = self.mz_data_type,
            obo_codes_mz_data_type = self.obo_codes_mz_data_type,
            int_data_type = self.int_data_type,
            obo_codes_int_data_type = self.obo_codes_int_data_type,
            scan_direction = self.scan_direction,
            obo_codes_scan_direction = self.obo_codes_scan_direction,
            scan_pattern = self.scan_pattern,
            obo_codes_scan_pattern = self.obo_codes_scan_pattern,
            scan_type = self.scan_type,
            obo_codes_scan_type = self.obo_codes_scan_type,
            line_scan_direction = self.line_scan_direction,
            obo_codes_line_scan_direction = self.obo_codes_line_scan_direction,
            mode = self.mode,
            obo_codes_mode = self.obo_codes_mode,
            mz_compression = self.mz_compression,
            obo_codes_mz_compression = self.obo_codes_mz_compression,
            int_compression = self.int_compression,
            obo_codes_int_compression = self.obo_codes_int_compression,
            polarity = self.polarity,
            obo_codes_polarity = self.obo_codes_polarity
        )
    }
}


pub struct IMZMLSpectrum {
    index: usize,
    spectrum_sum: u16, // sum of intensities in spectrum
    pixel_column: u32,
    pixel_row: u32,
    mz_len: usize,      // 8399
    mz_offset: usize,   // 16
    mz_enc_len: usize,  // 33596
    int_len: usize,     // 8399
    int_offset: usize,  // 33612
    int_enc_len: usize, //33596
}

impl std::fmt::Display for IMZMLSpectrum {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            r#"<spectrum defaultArrayLength="0" id="Scan={scan_number}"  index="{index}">
    <referenceableParamGroupRef ref="spectrum1"/>
    <cvParam cvRef="MS" accession="MS:1000285" name="total ion current" value="{spectrum_sum}"/>
    <scanList count="1">
        <cvParam cvRef="MS" accession="MS:1000795" name="no combination"/>
        <scan instrumentConfigurationRef="IC1">
            <referenceableParamGroupRef ref="scan1"/>
            <cvParam cvRef="IMS" accession="IMS:1000050" name="position x" value="{pixel_column}"/>
            <cvParam cvRef="IMS" accession="IMS:1000051" name="position y" value="{pixel_row}"/>
        </scan>
    </scanList>
    <binaryDataArrayList count="2">
        <binaryDataArray encodedLength="0">
            <referenceableParamGroupRef ref="mzArray"/>
            <cvParam accession="IMS:1000103" cvRef="IMS" name="external array length" value="{mz_len}"/>
            <cvParam accession="IMS:1000104" cvRef="IMS" name="external encoded length" value="{mz_enc_len}"/>
            <cvParam accession="IMS:1000102" cvRef="IMS" name="external offset" value="{mz_offset}"/>
            <binary/>
        </binaryDataArray>
        <binaryDataArray encodedLength="0">
            <referenceableParamGroupRef ref="intensityArray"/>
            <cvParam accession="IMS:1000103" cvRef="IMS" name="external array length" value="{int_len}"/>
            <cvParam accession="IMS:1000104" cvRef="IMS" name="external encoded length" value="{int_enc_len}"/>
            <cvParam accession="IMS:1000102" cvRef="IMS" name="external offset" value="{int_offset}"/>
            <binary/>
        </binaryDataArray>
    </binaryDataArrayList>
</spectrum>"#,
            index = self.index,
            scan_number = self.index + 1,
            spectrum_sum = self.spectrum_sum,
            pixel_column = self.pixel_column,
            pixel_row = self.pixel_row,
            mz_len = self.mz_len,
            mz_enc_len = self.mz_enc_len,
            mz_offset = self.mz_offset,
            int_len = self.int_len,
            int_enc_len = self.int_enc_len,
            int_offset = self.int_offset,
        )
    }
}
