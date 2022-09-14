two_grids Script

Using the two_grids_script files:

The two_grids program is a set of script files used for processing the two_grids.tpx3c data. It includes some code that is not relevant for the specific processing of two_grids.tpx3c, but is relevant for the corresponding manuscript (for example, for the creation of .imzml files or the .tpx3c file from a .tpx3 file).

The source code is in the "src" folder and can be compiled by the Nightly 1.63.0 (version bb8c2f411 2022-06-19) of the Rust programming language. It is highly likely that newer Nightly versions of the Rust programming language will work as well, however several "Nightly only" features are used, so the Stable branch of Rust will not work. The Rust language package manager Cargo is used with the "Cargo.toml" file to provide the proper versions of the libraries used in the script files. The Cargo.toml file provides documentation on all required libraries including version numbers.

System requirements (source code):
    * rustc 1.63.0-nightly (bb8c2f411 2022-06-19) - Rust programming language
    * cargo 1.63.0-nightly (8d42b0e87 2022-06-17) - Cargo package manager
    Library dependencies (copied from the Cargo.toml file):
    * rayon = "1.5.3"
    * png = "0.17.5"
    * plotly = "0.7.0"
    * itertools = "0.10.0"
    * simple-uuid = { version = "*" }
    * sha-1 = {version = "0.9.7"}
    * nohash-hasher = "0.2.0"

System requirements (compiled binary):
    * Microsoft Windows 10 Enterprise, 10.0.19044 Build 19044 (likely other Microsoft Windows operating systems will work as well)
    * No non-standard hardware is required. 

Installation / Compilation guide:
    1. Obtain the "two_grids.tpx3c" file from https://doi.org/10.34894/XKYD0Q and download this GitHub repository. Unzip the GitHub code and place the "two_grids.tpx3c" file in the unzipped directory.
    2a. For the compiled binary ("two_grids_script.exe"), simply double-clicking or running the compiled binary in the same directory as the two_grids.tpx3c file should produce the output that is in the "expected output" subdirectory. No installation of any languages or libraries should be required.
    2b. For compiling the source code, please install the Rust programming language and the Cargo package manager. Installation instructions may be found here: https://doc.rust-lang.org/cargo/getting-started/installation.html . The source code should allow for the two_grids_script to be run on Linux, macOS, and Windows.
    3. For the source code, after installation of the Rust programming language and the Cargo package manager, navigate to the top directory of the GitHub code that also contains the "two_grids.txp3c" file and run the command "cargo run --release". This should compile the source code in the "src" subdirectory and use the libraries specified in the "Cargo.toml" file. A "target" directory should appear where the compiled two_grids_script.exe file should reside (in the "release" subdirectory). Compilation and execution should take less than 2 minutes. Execution of the code on a workstation-class, desktop PC generally completed in 23 to 25 seconds.

Demo and Instructions for Use:
    1. After double-clicking the "two_grids_script.exe" file or running the "cargo run --release" command, the compilation step (if any) should be immediately proceeded by the code running. The compiled program will scan the current directory for any files labeled with an extension of “.tpx3c”, find the “two_grids.tpx3c” file, and begin processing automatically. 
    2. Completion of the code should take less than 2 minutes on a multi-core "normal" desktop computer. Some informational text (numbers of coordinates generated, dead pixels found, buffer lengths, etc.) should be printed to the console regarding different steps of the data processing.
    3. The example data set is the “two_grids.tpx3c” file that is a measurement used for Supplementary Fig. 6 in the manuscript. The script itself simply searches for any files that end with ".tpx3c" in the current directory and processes them. 
    4. Expected output: A set of “.png” files should appear, with the first being a file labeled “two_grids_tic.png” that represents the total ion count (TIC) image. The other files that appear are selected ion images at different time-of-flights. These images will be labeled with a (rough) mass-to-charge value in the form “two_grids_XX.X.png” where “XX.X” indicates a mass-to-charge with one decimal value. A single decimal value is not intended to define precision or accuracy of the measurement or mass accuracy, but is intended to prevent naming collisions and overwritten output files. Additionally, two files that are the "two_grids_report_full_spectrum.csv" and "two_grids_report_spectrum.html" should also be created. These are the (unprocessed from TOF to m/z) summed spectra of the .tpx3c file.
    5. A folder labeled “expected output” provides the expected images and files that should be the output from the program without any changes. These should exactly match the files produced by the compiled "two_grids_script.exe" provides.

Instructions for modification of source code: 
    1. Various parameters, such as the desired pixel size, can be changed in the “main.rs” file in the “src” directory to alter the output images.
    2. In general, parameters that are useful to alter are on lines 48 to 55 (for adjusting generation and visualization of the overall images) or on line 81 (for adjusting generation of specifically mass images). Additionally, commented-out lines from 33 to 43 provide the capabilities for converting .tpx3 files to .tpx3c files (not needed in this case as the .tpx3c file was provided) and for generating a "Plotly" plot of the time-of-flight "mass spectrum" (in this case, with time-of-flight rather than m/z as the x-axis).
    3. Uncommenting lines 107 and 108 in the "main.rs" source file and recompilling should produce a ".imzml" and accompanying ".ibd" files that should be able to be opened with the Datacube Explorer software found at: https://amolf.nl/download/datacubeexplorer.