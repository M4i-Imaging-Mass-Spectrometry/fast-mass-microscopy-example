use crate::{hit::Hit, reader::TDC_LIMIT};

#[derive(Clone, Debug)]
pub struct Pulse {
    pub time: i64, // time in nanoseconds (tdc for the shot)
    pub hits: Vec<Hit>,
    pub triggers: u64,   // trigger counter
    pub clusters: usize, // cluster labels begin at "1" once labelled
}

impl Default for Pulse {
    fn default() -> Pulse {
        Pulse {
            time: 0,
            hits: Vec::with_capacity(128),
            triggers: 0,
            clusters: 0,
        }
    }
}

impl Pulse {

    pub fn add_hit(&mut self, toa: i64, tot: u32, col: u8, row: u8) {
        self.hits.push(Hit::new(self.hits.len() as u32, toa, tot, col, row))
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut total = vec![self.to_tdc_packet().to_le_bytes()];
        for hit in &self.hits {
            total.push(hit.to_hit_packet().to_le_bytes());
            if hit.size > 1 {
                total.push(hit.to_blob_packet().to_le_bytes());
            }
        }
        total.into_iter().flatten().collect()
    }

    pub fn to_tdc_packet(&self) -> u64 {
        let add = (self.time % 25) as u64 + 1;
        let tdc = ((self.time % TDC_LIMIT) / 25) as u64;
        let header = 0x6Au64 << 56;
        let trigger = self.triggers << 44;
        let coarsetime = (tdc / 1000) << 12;
        let trigtime = (tdc % 1000) * 4096 / 1000;
        let upper = trigtime & 0x0E00;
        let lower = trigtime & 0x01FF;
        let lower = ((((lower * 12) >> 9) + add) & 0xFF) << 5;
        header | trigger | coarsetime | upper | lower
    }

    pub fn label_hits(&mut self) {
        let mut current_label = 1;
        let ohits = self.hits.clone();
        for i in 0..self.hits.len() {
            let hit = self.hits[i];
            if hit.label == 0 {
                let subset: Vec<&Hit> = ohits
                    .iter()
                    .filter(|o| {
                        ((hit.toa - o.toa).abs() < 1_000_000) // 1 us is really long for this.
                        && (o.label == 0)
                        && (hit.col as i16 - o.col as i16).abs() < 15
                        && (hit.row as i16 - o.row as i16).abs() < 15
                    })
                    .collect();
                let mut active = vec![&hit];
                let mut checked = vec![];
                while !active.is_empty() {
                    if let Some(check) = active.pop() {
                        for prox in subset.iter().filter(|h| h.is_proximal(check)) {
                            if !(checked.contains(prox) || active.contains(prox)) {
                                active.push(prox);
                            }
                        }
                        self.hits[check.index as usize].label = current_label;
                        checked.push(check);
                    }
                }
                current_label += 1;
            }
        }
        self.clusters = (current_label - 1) as usize;
    }

    pub fn centroid(&self) -> Pulse {
        let mut hits = vec![];
        let mut counter = 0;
        for i in 1..=self.clusters {
            let cluster: Vec<Hit> =
                self.hits.iter().filter(|h| h.label as usize == i).copied().collect();
            let size = cluster.len() as u16;
            if size == 0 {
                continue;
            }; // we skip occasional missed clusters
            counter += 1;
            let tot = cluster.iter().map(|h| h.tot).sum();
            let div = tot as f64;
            let mean_row = cluster.iter().map(|h| h.row as f64 * h.tot as f64).sum::<f64>() / div;
            let mean_col = cluster.iter().map(|h| h.col as f64 * h.tot as f64).sum::<f64>() / div;
            hits.push(Hit {
                toa: cluster.iter().map(|h| h.toa).min().unwrap(),
                tot,
                col: mean_col as u8,
                row: mean_row as u8,
                index: counter as u32,
                label: (counter + 1) as u16,
                size,
                col_offset: (mean_col.fract() * 255.0) as u8,
                row_offset: (mean_row.fract() * 255.0) as u8,
            });
        }
        Pulse { time: self.time, hits, triggers: self.triggers, clusters: counter }
    }

    pub fn quicksplat(&self) -> Pulse {
        Pulse {
            hits: self.hits.iter().flat_map(|h| h.quicksplat()).collect::<Vec<Hit>>(),
            time: self.time, // time in nanoseconds (tdc for the shot)
            triggers: self.triggers,
            clusters: self.clusters,
        }
    }
}
