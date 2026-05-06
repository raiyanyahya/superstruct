use crate::Value;
use md5::{Digest, Md5};

pub struct CountMinSketch {
    width: usize,
    depth: usize,
    table: Vec<Vec<u64>>,
}

impl CountMinSketch {
    pub fn new(width: usize, depth: usize) -> Self {
        Self {
            width,
            depth,
            table: vec![vec![0u64; width]; depth],
        }
    }

    pub fn default_size() -> Self {
        Self::new(1024, 5)
    }

    fn positions(&self, value: &Value) -> Vec<usize> {
        let encoded = value.to_string();
        (0..self.depth)
            .map(|row| {
                let mut hasher = Md5::new();
                hasher.update(encoded.as_bytes());
                hasher.update(&[row as u8]);
                let digest = hasher.finalize();
                let n = u32::from_be_bytes(digest[0..4].try_into().unwrap()) as usize;
                n % self.width
            })
            .collect()
    }

    pub fn add(&mut self, value: &Value) {
        for (row, col) in self.positions(value).iter().enumerate() {
            self.table[row][*col] += 1;
        }
    }

    pub fn estimate(&self, value: &Value) -> u64 {
        self.positions(value)
            .iter()
            .enumerate()
            .map(|(row, col)| self.table[row][*col])
            .min()
            .unwrap_or(0)
    }
}

impl Default for CountMinSketch {
    fn default() -> Self {
        Self::default_size()
    }
}
