use bytemuck::{Pod, Zeroable};

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct FmphDataPacked {
    pub keys_len: u64,
    pub seed1: u64,
    pub seed2: u64,
    pub bucket_count: u64,
    pub displacements_offset: u64,
    pub _pad: [u8; 24],
}

#[derive(Debug, Clone)]
pub struct FmphData {
    pub keys_len: usize,
    pub seed1: u64,
    pub seed2: u64,
    pub bucket_count: usize,
    pub displacements: Vec<u32>,
}

impl FmphData {
    pub fn pack(&self, displacement_bytes: &mut Vec<u8>) -> FmphDataPacked {
        let offset = displacement_bytes.len() as u64;
        displacement_bytes.extend_from_slice(bytemuck::cast_slice(&self.displacements));
        FmphDataPacked {
            keys_len: self.keys_len as u64,
            seed1: self.seed1,
            seed2: self.seed2,
            bucket_count: self.bucket_count as u64,
            displacements_offset: offset,
            _pad: [0u8; 24],
        }
    }
}

#[inline]
fn hash_key(key: u64, seed: u64) -> u64 {
    key.wrapping_mul(0x9E3779B97F4A7C15).wrapping_add(seed)
}

pub fn build_fmph(keys: &[u64]) -> FmphData {
    use std::collections::HashSet;
    let unique: Vec<u64> = {
        let mut set = HashSet::new();
        keys.iter().copied().filter(|k| set.insert(*k)).collect()
    };

    let n = unique.len();
    assert!(n > 0, "cannot build FMph for empty key set");

    let bucket_count = (n / 2).max(1);
    let max_displacement = (n as u64 * 8).max(128) as u32;

    use rand::RngExt;
    let mut rng = rand::rng();
    let mut best: Option<(FmphData, u32)> = None;

    for _attempt in 0..5000 {
        let seed1 = rng.random();
        let seed2 = rng.random();

        let mut buckets: Vec<Vec<u64>> = vec![Vec::new(); bucket_count];
        for &key in &unique {
            let b = hash_key(key, seed1) as usize % bucket_count;
            buckets[b].push(key);
        }

        let mut displacements = vec![0u32; bucket_count];
        let mut used = vec![false; n];
        let mut ok = true;
        let mut max_d = 0u32;

        let mut perm: Vec<usize> = (0..bucket_count).collect();
        perm.sort_by_key(|&i| buckets[i].len());
        perm.reverse();

        for &b in &perm {
            let bucket = &buckets[b];
            if bucket.is_empty() {
                continue;
            }

            let mut found = false;
            for d in 0..max_displacement {
                let mut indices = Vec::with_capacity(bucket.len());
                let mut collision = false;
                for &k in bucket {
                    let idx = hash_key(k, seed2).wrapping_add(d as u64) as usize % n;
                    if used[idx] || indices.contains(&idx) {
                        collision = true;
                        break;
                    }
                    indices.push(idx);
                }
                if !collision {
                    for &idx in &indices {
                        used[idx] = true;
                    }
                    displacements[b] = d;
                    max_d = max_d.max(d);
                    found = true;
                    break;
                }
            }
            if !found {
                ok = false;
                break;
            }
        }

        if ok {
            if best.is_none() || max_d < best.as_ref().unwrap().1 {
                best = Some((
                    FmphData {
                        keys_len: n,
                        seed1,
                        seed2,
                        bucket_count,
                        displacements,
                    },
                    max_d,
                ));
            }
            if max_d <= 1 {
                break;
            }
        }
    }

    best.map(|(data, _)| data).expect("unable to find FMph")
}

pub fn eval_fmph(data: &FmphData, key: u64) -> usize {
    let b = hash_key(key, data.seed1) as usize % data.bucket_count;
    let d = data.displacements[b] as u64;
    hash_key(key, data.seed2).wrapping_add(d) as usize % data.keys_len
}
