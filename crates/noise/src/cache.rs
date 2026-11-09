//! Data computed once per column of cubes (heightmaps, biomes) and shared by every cube of it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

const SHARDS: usize = 32;

type Shard<T> = HashMap<(u64, i32, i32), Arc<OnceLock<Arc<T>>>>;

/// Generation runs on many threads at once, so the map is split into locked shards; a shard that
/// grows past its share of the capacity drops a quarter of its entries (clearing it whole made a
/// view of 64, some 13k columns, compute most columns twice).
pub struct ColumnCache<T> {
    shards: [Mutex<Shard<T>>; SHARDS],
    per_shard: usize,
}

impl<T> ColumnCache<T> {
    pub fn new(capacity: usize) -> ColumnCache<T> {
        ColumnCache { shards: std::array::from_fn(|_| Mutex::new(HashMap::new())), per_shard: capacity.div_ceil(SHARDS).max(1) }
    }

    fn shard(&self, x: i32, z: i32) -> MutexGuard<'_, Shard<T>> {
        let h = crate::hash(0, x as i64, 0, z as i64) as u32;
        self.shards[(h >> 27) as usize % SHARDS]
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    /// The value for cube column (`x`, `z`) under `seed` if it is computed already.
    pub fn peek(&self, seed: u64, x: i32, z: i32) -> Option<Arc<T>> {
        self.shard(x, z)
            .get(&(seed, x, z))
            .and_then(|c| c.get().cloned())
    }

    /// The value for cube column (`x`, `z`) under `seed`, computing it with `make` on a miss.
    /// Threads asking while it is computed wait for it: the cubes of a column come together,
    /// and each computed it again.
    pub fn get(&self, seed: u64, x: i32, z: i32, make: impl FnOnce() -> T) -> Arc<T> {
        let cell = {
            let mut map = self.shard(x, z);
            match map.get(&(seed, x, z)) {
                Some(cell) => cell.clone(),
                None => {
                    if map.len() >= self.per_shard {
                        let mut n = 0usize;
                        map.retain(|_, _| {
                            n += 1;
                            n % 4 != 0
                        });
                    }
                    map.entry((seed, x, z)).or_default().clone()
                }
            }
        };
        cell.get_or_init(|| Arc::new(make())).clone()
    }
}
