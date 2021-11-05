use std::collections::HashMap;
use std::sync::mpsc::{sync_channel, Receiver, RecvError, SendError, SyncSender};
use std::sync::{Arc, Mutex};

pub trait PriceHolder<T> {
    fn put_price(&mut self, symbol: String, value: T) -> Result<(), SendError<T>>;
    fn get_price(&self, symbol: String) -> Option<T>;
    fn next_price(&mut self, symbol: String) -> Result<T, RecvError>;
}

pub struct ThreadUnsafe<T> {
    hashmap: HashMap<String, Price<T>>,
}

impl<T> ThreadUnsafe<T> {
    pub fn new() -> Self {
        Self {
            hashmap: HashMap::new(),
        }
    }

    fn price_receiver(&mut self, symbol: String) -> Receiver<T> {
        let (tx, rx) = sync_channel(1);
        match self.hashmap.get_mut(&symbol) {
            Some(price) => price.add_waiter(tx),
            None => {
                let mut p = Price::new();
                p.add_waiter(tx);
                self.hashmap.insert(symbol, p);
            }
        }
        rx
    }
}

impl<T> PriceHolder<T> for ThreadUnsafe<T>
where
    T: Clone,
{
    fn put_price(&mut self, symbol: String, value: T) -> Result<(), SendError<T>> {
        match self.hashmap.get_mut(&symbol) {
            Some(price) => price.update_price(value),
            None => {
                self.hashmap.insert(symbol, Price::from(value));
                Ok(())
            }
        }
    }

    fn get_price(&self, symbol: String) -> Option<T> {
        match self.hashmap.get(&symbol) {
            Some(price) => price.value.clone(),
            None => None,
        }
    }

    fn next_price(&mut self, symbol: String) -> Result<T, RecvError> {
        self.price_receiver(symbol).recv()
    }
}

impl<T> Default for ThreadUnsafe<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone)]
pub struct ThreadSafe<T> {
    inner: Arc<Mutex<ThreadUnsafe<T>>>,
}

impl<T> ThreadSafe<T> {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(ThreadUnsafe::new())),
        }
    }
}

impl<T> PriceHolder<T> for ThreadSafe<T>
where
    T: Clone,
{
    fn put_price(&mut self, symbol: String, value: T) -> Result<(), SendError<T>> {
        self.inner.lock().unwrap().put_price(symbol, value)
    }

    fn get_price(&self, symbol: String) -> Option<T> {
        self.inner.lock().unwrap().get_price(symbol)
    }

    fn next_price(&mut self, symbol: String) -> Result<T, RecvError> {
        let rx = { self.inner.lock().unwrap().price_receiver(symbol) }; // unlock mutex
        rx.recv()
    }
}

impl<T> Default for ThreadSafe<T> {
    fn default() -> Self {
        Self::new()
    }
}

struct Price<T> {
    value: Option<T>,
    waiters: Option<Vec<SyncSender<T>>>,
}

impl<T> Price<T> {
    fn new() -> Self {
        Self {
            value: None,
            waiters: None,
        }
    }

    fn from(value: T) -> Self {
        Self {
            value: Some(value),
            waiters: None,
        }
    }

    fn add_waiter(&mut self, waiter: SyncSender<T>) {
        match &mut self.waiters {
            Some(waiters) => waiters.push(waiter),
            None => self.waiters = Some(vec![waiter]),
        }
    }
}

impl<T> Price<T>
where
    T: Clone,
{
    fn update_price(&mut self, value: T) -> Result<(), SendError<T>> {
        self.value = Some(value.clone());
        self.notify_waiters(value)
    }

    fn notify_waiters(&mut self, value: T) -> Result<(), SendError<T>> {
        if let Some(waiters) = &self.waiters {
            for waiter in waiters {
                waiter.send(value.clone())?;
            }
            self.waiters = None;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::{sync::mpsc::RecvError, thread, time::Duration};

    use num::bigint::ToBigUint;
    use rust_decimal_macros::dec;

    #[test]
    fn put_and_get_price() {
        let mut ph = ThreadUnsafe::new();

        ph.put_price("symbol".to_string(), 1u32).unwrap();
        assert_eq!(ph.get_price("symbol".to_string()).unwrap(), 1);

        ph.put_price("symbol".to_string(), 2).unwrap();
        assert_eq!(ph.get_price("symbol".to_string()).unwrap(), 2);

        ph.put_price("another_symbol".to_string(), 3).unwrap();
        assert_eq!(ph.get_price("another_symbol".to_string()).unwrap(), 3);

        assert!(ph.get_price("not_a_symbol".to_string()).is_none());
    }

    #[test]
    fn price_holder_works_with_arbitrary_structs_that_are_copy() {
        #[derive(Copy, Clone, PartialEq, Eq, Debug)]
        struct Tmp();

        let mut ph = ThreadUnsafe::new();
        ph.put_price("is_copy".to_string(), Tmp()).unwrap();
        assert_eq!(ph.get_price("is_copy".to_string()).unwrap(), Tmp());
    }

    #[test]
    fn price_holder_works_with_arbitrary_precision_decimal_type() {
        let mut ph = ThreadUnsafe::new();
        ph.put_price(
            "pi".to_string(),
            dec!(3.14159265358979323846264338327950288419716939937510),
        )
        .unwrap();
        assert_eq!(
            ph.get_price("pi".to_string()).unwrap(),
            dec!(3.14159265358979323846264338327950288419716939937510)
        );
    }

    #[test]
    fn price_holder_works_with_num_biguint_type() {
        let mut ph = ThreadUnsafe::new();
        ph.put_price("symbol".to_string(), 123.to_biguint())
            .unwrap();
        assert_eq!(
            ph.get_price("symbol".to_string()).unwrap(),
            123.to_biguint(),
        );
    }

    #[test]
    fn next_price() {
        let mut ph = ThreadSafe::new();

        ph.put_price("symbol".to_string(), 1u64).unwrap();

        {
            let mut ph = ph.clone();
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(100));
                ph.put_price("symbol".to_string(), 2).unwrap();
            })
        };

        let price = ph.next_price("symbol".to_string()).unwrap();
        assert_eq!(price, 2);
    }

    #[test]
    fn wait_for_price_of_new_symbol() {
        let mut ph = ThreadSafe::new();

        {
            let mut ph = ph.clone();
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(100));
                ph.put_price("symbol".to_string(), 2u64).unwrap();
            })
        };

        let price = ph.next_price("symbol".to_string()).unwrap();
        assert_eq!(price, 2);
    }

    #[test]
    fn multiple_wait_for_next_price() {
        let mut ph = ThreadSafe::new();

        let mut handles = vec![];

        for _ in 1..=4 {
            let mut ph = ph.clone();
            let handle = thread::spawn(move || {
                let price = ph.next_price("symbol".to_string()).unwrap();
                assert_eq!(price, 1);
            });
            handles.push(handle);
        }

        thread::sleep(Duration::from_millis(100));
        ph.put_price("symbol".to_string(), 1u8).unwrap();

        for handle in handles {
            handle.join().unwrap();
        }
    }

    #[test]
    fn wait_for_next_price_multiple_times() {
        let mut ph = ThreadSafe::new();

        for p in 1u64..=4 {
            {
                let mut ph = ph.clone();
                thread::spawn(move || {
                    thread::sleep(Duration::from_millis(100));
                    ph.put_price("symbol".to_string(), p).unwrap();
                })
            };

            let price = ph.next_price("symbol".to_string()).unwrap();
            assert_eq!(price, p);
        }
    }

    #[test]
    fn next_price_is_the_same() {
        let mut ph = ThreadSafe::new();

        ph.put_price("symbol".to_string(), 1u32).unwrap();

        {
            let mut ph = ph.clone();
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(100));
                ph.put_price("symbol".to_string(), 1).unwrap();
            })
        };

        let price = ph.next_price("symbol".to_string()).unwrap();
        assert_eq!(price, 1);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn get_price_multiple_times_whilst_waiting_for_next_price() {
        let mut ph = ThreadSafe::new();

        ph.put_price("symbol".to_string(), 1u64).unwrap();

        {
            let mut ph = ph.clone();
            tokio::spawn(async move {
                let price = ph.next_price("symbol".to_string()).unwrap();
                assert_eq!(price, 2);
            });
        }

        let mut handles = vec![];

        {
            for _ in 0..100_000 {
                let ph = ph.clone();
                let handle = tokio::spawn(async move {
                    let price = ph.get_price("symbol".to_string()).unwrap();
                    assert_eq!(price, 1);
                });
                handles.push(handle);
            }
        }

        for handle in handles {
            handle.await.unwrap();
        }

        ph.put_price("symbol".to_string(), 2).unwrap();
    }

    #[test]
    fn test_thread_unsafe_channel_closed() {
        let mut ph: ThreadUnsafe<u64> = ThreadUnsafe::new();

        let rx = ph.price_receiver("symbol".to_string());

        ph.hashmap.get_mut("symbol").unwrap().waiters = None;
        assert_eq!(rx.recv().unwrap_err(), RecvError);
    }

    #[test]
    fn test_thread_safe_channel_closed() {
        let mut ph: ThreadSafe<u64> = ThreadSafe::new();

        {
            let ph = ph.clone();
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(100));
                ph.inner
                    .lock()
                    .unwrap()
                    .hashmap
                    .get_mut("symbol")
                    .unwrap()
                    .waiters = None;
            });
        }

        assert_eq!(ph.next_price("symbol".to_string()).unwrap_err(), RecvError)
    }
}
