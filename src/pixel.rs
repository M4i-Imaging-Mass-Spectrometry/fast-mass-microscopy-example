use std::ops::{Deref, DerefMut};

pub struct Pixel(Vec<f32>);

impl Pixel {
    pub fn empty() -> Pixel { Pixel(vec![]) }

    pub fn to_vecs(&mut self) -> (Vec<f32>, Vec<i16>) {
        self.sort_by(|a, b| a.total_cmp(b));
        let pixel_divisors: Vec<f32> = self.iter().map(|x| 10f32.powf(5.0 - x.log(10.0))).collect();
        let pixels: Vec<u64> =
            self.iter().zip(&pixel_divisors).map(|(x, div)| (x * div) as u64).collect();
        let (mut mzs, mut intensities) = (vec![], vec![]);
        let mut prev = 0;
        for (&mz, div) in pixels.iter().zip(&pixel_divisors) {
            if mz > prev {
                mzs.push(((mz as f32) / div) as f32);
                intensities.push(1);
            } else if mz == 0 {
                intensities.push(1);
            } else {
                *intensities.last_mut().unwrap() += 1;
            }
            prev = mz;
        }
        (mzs, intensities)
    }
}

impl Deref for Pixel {
    type Target = Vec<f32>;

    fn deref(&self) -> &Vec<f32> { &self.0 }
}

impl DerefMut for Pixel {
    fn deref_mut(&mut self) -> &mut Vec<f32> { &mut self.0 }
}

pub struct PixelSpan {
    pub pixel_added: bool,
    pub empty_pass_count: usize,
    pub pixels: Vec<Pixel>,
}

impl PixelSpan {
    pub fn empty(pixel_count: usize) -> PixelSpan {
        let pixels: Vec<Pixel> = (0..pixel_count).map(|_| Pixel::empty()).collect();
        PixelSpan { pixel_added: false, empty_pass_count: 0, pixels }
    }

    pub fn add_mz(&mut self, mz: f32, pixel_index: usize) {
        self.empty_pass_count = 0;
        self.pixel_added = true;
        self.pixels[pixel_index].push(mz);
    }

    // updates internal information and returns true if ready to write.
    pub fn update_end_pass(&mut self) {
        self.empty_pass_count = if self.pixel_added { 0 } else { self.empty_pass_count + 1 };
        self.pixel_added = false;
    }
}
