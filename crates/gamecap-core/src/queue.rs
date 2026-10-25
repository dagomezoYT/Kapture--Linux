//! Bounded MPMC-style queue. Capture uses `try_send`; it never waits on disk.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug)]
#[allow(clippy::result_large_err)]
pub enum TrySendError<T> {
    Full(T),
    Closed(T),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecvTimeoutError {
    Timeout,
    Disconnected,
}

struct Inner<T> {
    deque: VecDeque<T>,
    cap: usize,
}

struct Shared<T> {
    inner: Mutex<Inner<T>>,
    not_empty: Condvar,
    not_full: Condvar,
    senders: AtomicUsize,
    closed: AtomicBool,
}

pub struct BoundedSender<T> {
    shared: Arc<Shared<T>>,
}

pub struct BoundedReceiver<T> {
    shared: Arc<Shared<T>>,
}

pub fn bounded<T: Send>(capacity: usize) -> (BoundedSender<T>, BoundedReceiver<T>) {
    let cap = capacity.max(1);
    let shared = Arc::new(Shared {
        inner: Mutex::new(Inner {
            deque: VecDeque::with_capacity(cap),
            cap,
        }),
        not_empty: Condvar::new(),
        not_full: Condvar::new(),
        senders: AtomicUsize::new(1),
        closed: AtomicBool::new(false),
    });
    (
        BoundedSender {
            shared: Arc::clone(&shared),
        },
        BoundedReceiver { shared },
    )
}

impl<T: Send> BoundedSender<T> {
    #[allow(clippy::result_large_err)]
    pub fn try_send(&self, item: T) -> Result<(), TrySendError<T>> {
        if self.shared.closed.load(Ordering::Acquire) {
            return Err(TrySendError::Closed(item));
        }
        let mut guard = match self.shared.inner.lock() {
            Ok(g) => g,
            Err(_) => return Err(TrySendError::Closed(item)),
        };
        if self.shared.closed.load(Ordering::Relaxed) {
            return Err(TrySendError::Closed(item));
        }
        if guard.deque.len() >= guard.cap {
            return Err(TrySendError::Full(item));
        }
        guard.deque.push_back(item);
        drop(guard);
        self.shared.not_empty.notify_one();
        Ok(())
    }

    #[allow(clippy::result_large_err)]
    pub fn send_timeout(&self, mut item: T, timeout: Duration) -> Result<(), TrySendError<T>> {
        let deadline = Instant::now() + timeout;
        loop {
            match self.try_send(item) {
                Ok(()) => return Ok(()),
                Err(TrySendError::Closed(v)) => return Err(TrySendError::Closed(v)),
                Err(TrySendError::Full(v)) => {
                    item = v;
                    let now = Instant::now();
                    if now >= deadline {
                        return Err(TrySendError::Full(item));
                    }
                    let remaining = deadline - now;
                    let guard = match self.shared.inner.lock() {
                        Ok(g) => g,
                        Err(_) => return Err(TrySendError::Closed(item)),
                    };
                    if guard.deque.len() >= guard.cap && !self.shared.closed.load(Ordering::Relaxed)
                    {
                        let _ = self.shared.not_full.wait_timeout(guard, remaining);
                    }
                }
            }
        }
    }

    pub fn len(&self) -> usize {
        self.shared.inner.lock().map(|g| g.deque.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn close(&self) {
        self.shared.closed.store(true, Ordering::Release);
        self.shared.not_empty.notify_all();
        self.shared.not_full.notify_all();
    }
}

impl<T: Send> Clone for BoundedSender<T> {
    fn clone(&self) -> Self {
        self.shared.senders.fetch_add(1, Ordering::Relaxed);
        Self {
            shared: Arc::clone(&self.shared),
        }
    }
}

impl<T> Drop for BoundedSender<T> {
    fn drop(&mut self) {
        let prev = self.shared.senders.fetch_sub(1, Ordering::AcqRel);
        if prev == 1 {
            self.shared.closed.store(true, Ordering::Release);
            self.shared.not_empty.notify_all();
            self.shared.not_full.notify_all();
        }
    }
}

impl<T: Send> BoundedReceiver<T> {
    pub fn recv_timeout(&self, timeout: Duration) -> Result<T, RecvTimeoutError> {
        let deadline = Instant::now() + timeout;
        loop {
            let mut guard = match self.shared.inner.lock() {
                Ok(g) => g,
                Err(_) => return Err(RecvTimeoutError::Disconnected),
            };
            if let Some(item) = guard.deque.pop_front() {
                drop(guard);
                self.shared.not_full.notify_one();
                return Ok(item);
            }
            if self.shared.closed.load(Ordering::Relaxed) {
                return Err(RecvTimeoutError::Disconnected);
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(RecvTimeoutError::Timeout);
            }
            let remaining = deadline - now;
            match self.shared.not_empty.wait_timeout(guard, remaining) {
                Ok(_) => {}
                Err(_) => return Err(RecvTimeoutError::Disconnected),
            }
        }
    }

    pub fn try_recv(&self) -> Result<T, RecvTimeoutError> {
        let mut guard = match self.shared.inner.lock() {
            Ok(g) => g,
            Err(_) => return Err(RecvTimeoutError::Disconnected),
        };
        if let Some(item) = guard.deque.pop_front() {
            drop(guard);
            self.shared.not_full.notify_one();
            return Ok(item);
        }
        if self.shared.closed.load(Ordering::Relaxed) {
            Err(RecvTimeoutError::Disconnected)
        } else {
            Err(RecvTimeoutError::Timeout)
        }
    }

    pub fn is_empty(&self) -> bool {
        self.shared
            .inner
            .lock()
            .map(|g| g.deque.is_empty())
            .unwrap_or(true)
    }

    pub fn len(&self) -> usize {
        self.shared.inner.lock().map(|g| g.deque.len()).unwrap_or(0)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn try_send_full() {
        let (tx, _rx) = bounded::<u32>(1);
        tx.try_send(1).unwrap();
        match tx.try_send(2) {
            Err(TrySendError::Full(2)) => {}
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn disconnect_after_last_sender_drop() {
        let (tx, rx) = bounded::<u32>(4);
        tx.try_send(1).unwrap();
        drop(tx);
        assert_eq!(rx.recv_timeout(Duration::from_millis(50)).unwrap(), 1);
        assert_eq!(
            rx.recv_timeout(Duration::from_millis(50)),
            Err(RecvTimeoutError::Disconnected)
        );
    }
}
