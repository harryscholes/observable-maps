use std::collections::HashMap;
use std::hash::Hash;
use std::sync::mpsc::{sync_channel, Receiver, RecvError, SendError, SyncSender};
use std::sync::{Arc, RwLock};

pub trait ObservableMap<K, V> {
    fn insert(&mut self, key: K, value: V) -> Result<(), SendError<V>>;
    fn get(&self, key: K) -> Option<V>;
    fn observe(&mut self, key: K) -> Receiver<V>;
    fn wait(&mut self, key: K) -> Result<V, RecvError>;
}

pub struct ObserverMap<K, V> {
    hashmap: HashMap<K, Item<V>>,
}

impl<K, V> ObserverMap<K, V> {
    pub fn new() -> Self {
        Self {
            hashmap: HashMap::new(),
        }
    }
}

impl<K, V> ObservableMap<K, V> for ObserverMap<K, V>
where
    K: Hash + Eq + PartialEq,
    V: Clone,
{
    fn insert(&mut self, key: K, value: V) -> Result<(), SendError<V>> {
        match self.hashmap.get_mut(&key) {
            Some(item) => item.update(value),
            None => {
                self.hashmap.insert(key, Item::new(value));
                Ok(())
            }
        }
    }

    fn get(&self, key: K) -> Option<V> {
        match self.hashmap.get(&key) {
            Some(item) => item.value.clone(),
            None => None,
        }
    }

    fn observe(&mut self, key: K) -> Receiver<V> {
        let (tx, rx) = sync_channel(1);
        match self.hashmap.get_mut(&key) {
            Some(item) => {
                item.add_observer(tx);
            }
            None => {
                self.hashmap.insert(key, Item::from_observer(tx));
            }
        }
        rx
    }

    fn wait(&mut self, key: K) -> Result<V, RecvError> {
        self.observe(key).recv()
    }
}

impl<K, V> Default for ObserverMap<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone)]
pub struct ThreadSafeObserverMap<K, V> {
    inner: Arc<RwLock<ObserverMap<K, V>>>,
}

impl<K, V> ThreadSafeObserverMap<K, V> {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(ObserverMap::new())),
        }
    }
}

impl<K, V> ObservableMap<K, V> for ThreadSafeObserverMap<K, V>
where
    K: Hash + Eq + PartialEq,
    V: Clone,
{
    fn insert(&mut self, key: K, value: V) -> Result<(), SendError<V>> {
        self.inner.write().unwrap().insert(key, value)
    }

    fn get(&self, key: K) -> Option<V> {
        self.inner.read().unwrap().get(key)
    }

    fn observe(&mut self, key: K) -> Receiver<V> {
        self.inner.write().unwrap().observe(key)
    }

    fn wait(&mut self, key: K) -> Result<V, RecvError> {
        self.observe(key).recv()
    }
}

impl<K, V> Default for ThreadSafeObserverMap<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

struct Item<T> {
    value: Option<T>,
    observers: Option<Vec<SyncSender<T>>>,
}

impl<T> Item<T>
where
    T: Clone,
{
    fn new(value: T) -> Self {
        Self {
            value: Some(value),
            observers: None,
        }
    }

    fn from_observer(observer: SyncSender<T>) -> Self {
        Self {
            value: None,
            observers: Some(vec![observer]),
        }
    }

    fn update(&mut self, value: T) -> Result<(), SendError<T>> {
        self.value = Some(value.clone());
        self.notify(value)
    }

    fn add_observer(&mut self, observer: SyncSender<T>) {
        match &mut self.observers {
            Some(observers) => observers.push(observer),
            None => self.observers = Some(vec![observer]),
        }
    }

    fn notify(&mut self, value: T) -> Result<(), SendError<T>> {
        if let Some(observers) = &self.observers {
            for observer in observers {
                observer.send(value.clone())?;
            }
            self.observers = None;
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
    fn insert_and_get() {
        let mut map = ObserverMap::new();

        map.insert("key".to_string(), 1u32).unwrap();
        assert_eq!(map.get("key".to_string()).unwrap(), 1);

        map.insert("key".to_string(), 2).unwrap();
        assert_eq!(map.get("key".to_string()).unwrap(), 2);

        map.insert("another_key".to_string(), 3).unwrap();
        assert_eq!(map.get("another_key".to_string()).unwrap(), 3);

        assert!(map.get("not_a_key".to_string()).is_none());
    }

    #[test]
    fn value_is_arbitrary_structs_that_are_copy() {
        #[derive(Copy, Clone, PartialEq, Eq, Debug)]
        struct Tmp();

        let mut map = ObserverMap::new();
        map.insert("is_copy".to_string(), Tmp()).unwrap();
        assert_eq!(map.get("is_copy".to_string()).unwrap(), Tmp());
    }

    #[test]
    fn value_is_arbitrary_precision_decimal_type() {
        let mut map = ObserverMap::new();
        map.insert("pi".to_string(), dec!(3.1415926535897932384))
            .unwrap();
        assert_eq!(
            map.get("pi".to_string()).unwrap(),
            dec!(3.1415926535897932384)
        );
    }

    #[test]
    fn value_is_num_biguint_type() {
        let mut map = ObserverMap::new();
        map.insert("key".to_string(), 123.to_biguint()).unwrap();
        assert_eq!(map.get("key".to_string()).unwrap(), 123.to_biguint(),);
    }

    #[test]
    fn thread_safe_wait_for_next_value() {
        let mut map = ThreadSafeObserverMap::new();

        map.insert("key".to_string(), 1u64).unwrap();

        {
            let mut map = map.clone();
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(100));
                map.insert("key".to_string(), 2).unwrap();
            })
        };

        assert_eq!(map.wait("key".to_string()).unwrap(), 2);
    }

    #[test]
    fn wait_for_value_of_new_key() {
        let mut map = ThreadSafeObserverMap::new();

        {
            let mut map = map.clone();
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(100));
                map.insert("key".to_string(), 2u64).unwrap();
            })
        };

        assert_eq!(map.wait("key".to_string()).unwrap(), 2);
    }

    #[test]
    fn multiple_observers() {
        let mut map = ThreadSafeObserverMap::new();

        let mut handles = vec![];

        for _ in 1..=4 {
            let mut map = map.clone();
            let handle = thread::spawn(move || {
                assert_eq!(map.wait("key".to_string()).unwrap(), 1);
            });
            handles.push(handle);
        }

        thread::sleep(Duration::from_millis(100));
        map.insert("key".to_string(), 1u8).unwrap();

        for handle in handles {
            handle.join().unwrap();
        }
    }

    #[test]
    fn wait_for_next_value_multiple_times() {
        let mut map = ThreadSafeObserverMap::new();

        for v in 1u64..=4 {
            {
                let mut map = map.clone();
                thread::spawn(move || {
                    thread::sleep(Duration::from_millis(100));
                    map.insert("key".to_string(), v).unwrap();
                })
            };

            assert_eq!(map.wait("key".to_string()).unwrap(), v);
        }
    }

    #[test]
    fn next_value_is_the_same_as_current_value() {
        let mut map = ThreadSafeObserverMap::new();

        map.insert("key".to_string(), 1u32).unwrap();

        {
            let mut map = map.clone();
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(100));
                map.insert("key".to_string(), 1).unwrap();
            })
        };

        assert_eq!(map.wait("key".to_string()).unwrap(), 1);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn get_value_multiple_times_whilst_waiting_for_next_value() {
        let mut map = ThreadSafeObserverMap::new();

        map.insert("key".to_string(), 1u64).unwrap();

        {
            let mut map = map.clone();
            tokio::spawn(async move {
                assert_eq!(map.wait("key".to_string()).unwrap(), 2);
            });
        }

        let mut handles = vec![];

        {
            for _ in 0..100_000 {
                let map = map.clone();
                let handle = tokio::spawn(async move {
                    assert_eq!(map.get("key".to_string()).unwrap(), 1);
                });
                handles.push(handle);
            }
        }

        for handle in handles {
            handle.await.unwrap();
        }

        map.insert("key".to_string(), 2).unwrap();
    }

    #[test]
    fn thread_unsafe_channel_closed() {
        let mut map: ObserverMap<String, u32> = ObserverMap::new();

        let rx = map.observe("key".to_string());

        // Close the channel
        map.hashmap.get_mut("key").unwrap().observers = None;

        assert_eq!(rx.recv().unwrap_err(), RecvError);
    }

    #[test]
    fn thread_safe_channel_closed() {
        let mut map: ThreadSafeObserverMap<String, u64> = ThreadSafeObserverMap::new();

        let rx = map.observe("key".to_string());

        // Close the channel
        map.inner
            .write()
            .unwrap()
            .hashmap
            .get_mut("key")
            .unwrap()
            .observers = None;

        assert_eq!(rx.recv().unwrap_err(), RecvError);
    }
}
