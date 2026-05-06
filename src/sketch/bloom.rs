use crate::Value;
use md5::{Digest, Md5};

pub struct BloomSketch {
    bit_size: usize,
    num_hashes: usize,
    bits: Vec<u8>,
}

impl BloomSketch {
    pub fn new(bit_size: usize, num_hashes: usize) -> Self {
        Self {
            bit_size,
            num_hashes,
            bits: vec![0u8; bit_size / 8],
        }
    }

    pub fn default_size() -> Self {
        Self::new(1 << 14, 5)
    }

    fn positions(&self, value: &Value) -> Vec<usize> {
        let encoded = value.to_string();
        let digest = Md5::digest(encoded.as_bytes());
        let digest_bytes = digest.as_slice();
        let doubled: Vec<u8> = digest_bytes.iter().chain(digest_bytes.iter()).copied().collect();

        (0..self.num_hashes)
            .map(|i| {
                let start = (i * 4) % digest_bytes.len();
                let chunk = &doubled[start..start + 4];
                let n = u32::from_be_bytes(chunk.try_into().unwrap()) as usize;
                n % self.bit_size
            })
            .collect()
    }

    pub fn add(&mut self, value: &Value) {
        for p in self.positions(value) {
            self.bits[p / 8] |= 1 << (p % 8);
        }
    }

    pub fn maybe_contains(&self, value: &Value) -> bool {
        for p in self.positions(value) {
            if self.bits[p / 8] & (1 << (p % 8)) == 0 {
                return false;
            }
        }
        true
    }
}

impl Default for BloomSketch {
    fn default() -> Self {
        Self::default_size()
    }
}
