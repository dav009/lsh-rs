use crate::hash::{Hash, SignRandomProjections, VecHash, L2, MIPS};
use crate::table::{Bucket, DataPoint, DataPointSlice, HashTableError, HashTables, MemoryTable};
use crate::utils::create_rng;
use fnv::{FnvBuildHasher, FnvHashSet as HashSet};
use rand::{Rng, SeedableRng};

pub struct LSH<T: HashTables, H: VecHash> {
    n_hash_tables: usize,
    n_projections: usize,
    hashers: Vec<H>,
    dim: usize,
    hash_tables: T,
    _seed: u64,
}

impl LSH<MemoryTable, SignRandomProjections> {
    /// Create a new SignRandomProjections LSH
    pub fn srp(&mut self) -> Self {
        let mut rng = create_rng(self._seed);
        let mut hashers = Vec::with_capacity(self.n_hash_tables);

        for _ in 0..self.n_hash_tables {
            let seed = rng.gen();
            let hasher = SignRandomProjections::new(self.n_projections, self.dim, seed);
            hashers.push(hasher);
        }
        LSH {
            n_hash_tables: self.n_hash_tables,
            n_projections: self.n_projections,
            hashers,
            dim: self.dim,
            hash_tables: MemoryTable::new(self.n_hash_tables),
            _seed: self._seed,
        }
    }
}

/// Create a new L2 LSH
///
/// See hash function:
/// https://www.cs.princeton.edu/courses/archive/spring05/cos598E/bib/p253-datar.pdf
/// in paragraph 3.2
///
/// h(v) = floor(a^Tv + b / r)
///
/// # Arguments
///
/// * `r` - Parameter of hash function.
impl LSH<MemoryTable, L2> {
    pub fn l2(&mut self, r: f32) -> Self {
        let mut rng = create_rng(self._seed);
        let mut hashers = Vec::with_capacity(self.n_hash_tables);
        for _ in 0..self.n_hash_tables {
            let seed = rng.gen();
            let hasher = L2::new(self.dim, r, self.n_projections, seed);
            hashers.push(hasher);
        }
        LSH {
            n_hash_tables: self.n_hash_tables,
            n_projections: self.n_projections,
            hashers,
            dim: self.dim,
            hash_tables: MemoryTable::new(self.n_hash_tables),
            _seed: self._seed,
        }
    }
}

impl LSH<MemoryTable, MIPS> {
    /// Create a new MIPS LSH
    ///
    /// Async hasher
    ///
    /// See hash function:
    /// https://www.cs.rice.edu/~as143/Papers/SLIDE_MLSys.pdf
    ///
    /// # Arguments
    ///
    /// * `r` - Parameter of hash function.
    /// * `U` - Parameter of hash function.
    /// * `m` - Parameter of hash function.
    pub fn mips(&mut self, r: f32, U: f32, m: usize) -> Self {
        let mut rng = create_rng(self._seed);
        let mut hashers = Vec::with_capacity(self.n_hash_tables);

        for _ in 0..self.n_hash_tables {
            let seed = rng.gen();
            let hasher = MIPS::new(self.dim, r, U, m, self.n_projections, seed);
            hashers.push(hasher);
        }
        LSH {
            n_hash_tables: self.n_hash_tables,
            n_projections: self.n_projections,
            hashers,
            dim: self.dim,
            hash_tables: MemoryTable::new(self.n_hash_tables),
            _seed: self._seed,
        }
    }
}

impl<H: VecHash> LSH<MemoryTable, H> {
    /// Create a new Base LSH
    ///
    /// # Arguments
    ///
    /// * `n_projections` - Hash length. Every projections creates an hashed integer
    /// * `n_hash_tables` - Increases the chance of finding the closest but has a performance and space cost.
    /// * `dim` - Dimensions of the data points.

    pub fn new(n_projections: usize, n_hash_tables: usize, dim: usize) -> Self {
        LSH {
            n_hash_tables,
            n_projections,
            hashers: Vec::with_capacity(0),
            dim,
            hash_tables: MemoryTable::new(n_hash_tables),
            _seed: 0,
        }
    }

    /// Set seed of LSH
    /// # Arguments
    /// * `seed` - Seed for the RNG's if 0, RNG's are seeded randomly.
    pub fn seed(&mut self, seed: u64) -> &mut Self {
        self._seed = seed;
        self
    }
}

impl<H: VecHash> LSH<MemoryTable, H> {
    /// Store a single vector in storage.
    ///
    /// # Arguments
    /// * `v` - Data point.
    pub fn store_vec(&mut self, v: &DataPointSlice) {
        for (i, proj) in self.hashers.iter().enumerate() {
            let hash = proj.hash_vec_put(v);
            match self.hash_tables.put(hash, v.to_vec(), i) {
                Ok(_) => (),
                Err(_) => panic!("Could not store vec"),
            }
        }
    }

    /// Store multiple vectors in storage. Before storing the storage capacity is possibly
    /// increased to match the data points.
    ///
    /// # Arguments
    /// * `vs` - Array of data points.
    pub fn store_vecs(&mut self, vs: &[DataPoint]) {
        self.hash_tables.increase_storage(vs.len());
        for d in vs {
            self.store_vec(d)
        }
    }

    /// Query all buckets in the hash tables. The union of the matching buckets over the `L`
    /// hash tables is returned
    ///
    /// # Arguments
    /// * `v` - Query vector
    pub fn query_bucket(&self, v: &DataPointSlice) -> Vec<&DataPoint> {
        let mut bucket_union = HashSet::default();

        for (i, proj) in self.hashers.iter().enumerate() {
            let hash = proj.hash_vec_query(v);
            match self.hash_tables.query_bucket(&hash, i) {
                Err(HashTableError::NotFound) => (),
                Ok(bucket) => {
                    bucket_union = bucket_union.union(bucket).copied().collect();
                }
                _ => panic!("Unexpected query result"),
            }
        }
        bucket_union
            .iter()
            .map(|&idx| self.hash_tables.idx_to_datapoint(idx))
            .collect()
    }

    /// Delete data point from storage. This does not free memory as the storage vector isn't resized.
    ///
    /// # Arguments
    /// * `v` - Data point
    pub fn delete_vec(&mut self, v: &DataPointSlice) {
        for (i, proj) in self.hashers.iter().enumerate() {
            let hash = proj.hash_vec_query(v);
            self.hash_tables.delete(hash, v, i).unwrap_or_default();
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_simhash() {
        // Only test if it runs
        let h = SignRandomProjections::new(5, 3, 1);
    }

    #[test]
    fn test_hash_table() {
        let mut lsh = LSH::new(5, 10, 3).seed(1).srp();
        let v1 = &[2., 3., 4.];
        let v2 = &[-1., -1., 1.];
        let v3 = &[0.2, -0.2, 0.2];
        lsh.store_vec(v1);
        lsh.store_vec(v2);
        assert!(lsh.query_bucket(v2).len() > 0);

        let bucket_len_before = lsh.query_bucket(v1).len();
        lsh.delete_vec(v1);
        let bucket_len_before_after = lsh.query_bucket(v1).len();
        assert!(bucket_len_before > bucket_len_before_after);
    }
}
