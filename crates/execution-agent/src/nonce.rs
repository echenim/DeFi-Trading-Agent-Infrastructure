use std::sync::atomic::{AtomicU64, Ordering};

/// Thread-safe nonce manager backed by an atomic counter.
///
/// Each call to `next()` atomically increments and returns the current nonce,
/// ensuring no two transactions share the same nonce.
pub struct NonceManager {
    nonce: AtomicU64,
}

impl NonceManager {
    /// Create a new `NonceManager` starting at the given nonce.
    pub fn new(initial: u64) -> Self {
        Self {
            nonce: AtomicU64::new(initial),
        }
    }

    /// Atomically fetch the current nonce and increment it.
    pub fn next(&self) -> u64 {
        self.nonce.fetch_add(1, Ordering::SeqCst)
    }

    /// Return the current nonce without incrementing.
    pub fn current(&self) -> u64 {
        self.nonce.load(Ordering::SeqCst)
    }

    /// Reset the nonce to a value fetched from the chain (for startup/recovery).
    pub fn sync(&self, chain_nonce: u64) {
        self.nonce.store(chain_nonce, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nonce_increment() {
        let nm = NonceManager::new(0);
        assert_eq!(nm.next(), 0);
        assert_eq!(nm.next(), 1);
        assert_eq!(nm.next(), 2);
        assert_eq!(nm.current(), 3);
    }

    #[test]
    fn test_nonce_starts_at_initial() {
        let nm = NonceManager::new(42);
        assert_eq!(nm.current(), 42);
        assert_eq!(nm.next(), 42);
        assert_eq!(nm.current(), 43);
    }

    #[test]
    fn test_nonce_sync() {
        let nm = NonceManager::new(0);
        nm.next();
        nm.next();
        assert_eq!(nm.current(), 2);

        nm.sync(100);
        assert_eq!(nm.current(), 100);
        assert_eq!(nm.next(), 100);
        assert_eq!(nm.current(), 101);
    }

    #[test]
    fn test_nonce_concurrent() {
        use std::sync::Arc;
        use std::thread;

        let nm = Arc::new(NonceManager::new(0));
        let mut handles = vec![];

        for _ in 0..10 {
            let nm = Arc::clone(&nm);
            handles.push(thread::spawn(move || nm.next()));
        }

        let mut results: Vec<u64> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        results.sort();
        // All 10 values should be unique and span 0..10
        assert_eq!(results, (0..10).collect::<Vec<_>>());
        assert_eq!(nm.current(), 10);
    }
}
