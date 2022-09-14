#[inline(always)]
pub fn smooth(vector: &[f64], window: usize) -> Vec<f64> {
    vector.windows(window).map(|a| a.iter().sum::<f64>() / a.len() as f64).collect()
}

#[inline(always)]
pub fn distance(x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
    ((x2 - x1).powi(2) + (y2 - y1).powi(2)).sqrt()
}

#[inline(always)]
pub fn argmax_u32(slice: &[u32]) -> (usize, u32) {
    slice.iter().enumerate().fold((0, slice[0]), |(idx_max, val_max), (idx, val)| {
        if &val_max > val {
            (idx_max, val_max)
        } else {
            (idx, *val)
        }
    })
}
