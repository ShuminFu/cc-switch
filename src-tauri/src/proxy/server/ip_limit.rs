//! Per-remote-IP connection limit for the proxy listener.
//!
//! The global connection semaphore (SEC-14) bounds the total number of open
//! sockets, but when the proxy listens on a non-loopback address a single LAN
//! host could still take every slot and lock out the local CLIs. This limiter
//! caps concurrently open connections per remote IP. Loopback peers are not
//! limited here (every local tool shares `127.0.0.1`); they remain bounded by
//! the global cap.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};

/// Default per-IP cap for non-loopback peers.
pub(crate) const MAX_CONNECTIONS_PER_REMOTE_IP: usize = 64;

#[derive(Clone)]
pub(crate) struct PerIpLimiter {
    limit: usize,
    counts: Arc<Mutex<HashMap<IpAddr, usize>>>,
}

/// Releases the connection slot for its IP when dropped.
pub(crate) struct PerIpPermit {
    ip: Option<IpAddr>,
    counts: Arc<Mutex<HashMap<IpAddr, usize>>>,
}

impl PerIpLimiter {
    pub(crate) fn new(limit: usize) -> Self {
        Self {
            limit,
            counts: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Take a slot for `ip`, or `None` when that IP is already at the limit.
    pub(crate) fn try_acquire(&self, ip: IpAddr) -> Option<PerIpPermit> {
        if ip.is_loopback() || ip.to_canonical().is_loopback() {
            return Some(PerIpPermit {
                ip: None,
                counts: self.counts.clone(),
            });
        }
        let mut counts = self.counts.lock().unwrap_or_else(|e| e.into_inner());
        let count = counts.entry(ip).or_insert(0);
        if *count >= self.limit {
            return None;
        }
        *count += 1;
        Some(PerIpPermit {
            ip: Some(ip),
            counts: self.counts.clone(),
        })
    }

    #[cfg(test)]
    fn open_for(&self, ip: IpAddr) -> usize {
        self.counts
            .lock()
            .unwrap()
            .get(&ip)
            .copied()
            .unwrap_or_default()
    }
}

impl Drop for PerIpPermit {
    fn drop(&mut self) {
        let Some(ip) = self.ip else {
            return;
        };
        let mut counts = self.counts.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(count) = counts.get_mut(&ip) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                counts.remove(&ip);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_remote_ip_and_releases_on_drop() {
        let limiter = PerIpLimiter::new(2);
        let lan: IpAddr = "192.168.1.20".parse().unwrap();
        let other: IpAddr = "192.168.1.21".parse().unwrap();

        let a = limiter.try_acquire(lan).expect("first");
        let _b = limiter.try_acquire(lan).expect("second");
        assert!(limiter.try_acquire(lan).is_none(), "third must be refused");
        assert!(limiter.try_acquire(other).is_some(), "other IPs unaffected");

        drop(a);
        assert_eq!(limiter.open_for(lan), 1);
        assert!(limiter.try_acquire(lan).is_some());
    }

    #[test]
    fn loopback_is_not_limited() {
        let limiter = PerIpLimiter::new(1);
        let v4: IpAddr = "127.0.0.1".parse().unwrap();
        let mapped: IpAddr = "::ffff:127.0.0.1".parse().unwrap();
        let permits: Vec<_> = (0..10)
            .map(|i| limiter.try_acquire(if i % 2 == 0 { v4 } else { mapped }))
            .collect();
        assert!(permits.iter().all(Option::is_some));
    }
}
