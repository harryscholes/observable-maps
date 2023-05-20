# observable-maps

Thread-safe, generic, observable hash maps that notify observers of state changes.

### Usage

```rust
use std::{thread, time::Duration};

use observable_maps::{ObservableMap, ThreadSafeObserverMap};
use rust_decimal_macros::dec;

fn main() {
    // Types that are `ObservableMap` are generic, which allows them to store
    // arbitrary precision decimal types, for example.
    let mut map = ThreadSafeObserverMap::new();

    let key = "pi";
    let value = dec!(3.1415926535897932384);

    {
        // `ThreadSafeObserverMap` is made thread-safe by encapsulating an `Arc`,
        // which automatically increases the reference count to the price holder
        // when it is cloned.
        let mut map = map.clone();

        // Spawn a new thread ...
        thread::spawn(move || {
            // ... that waits for some time ...
            thread::sleep(Duration::from_secs(1));

            // ... then inserts a new value into the map.
            map.insert(key, value).unwrap();

            println!("Inserted {} => {}", key, value);
        })
    };

    // Wait for the value of a key to be updated in the map,
    // by blocking execution of the thread.
    let updated_value = map.wait(key).unwrap();

    println!("Updated {} => {}", key, updated_value);

    assert_eq!(updated_value, value);
}
```

Outputs:

```sh
$ cargo run --quiet main.rs
Updated pi => 3.1415926535897932384
Inserted pi => 3.1415926535897932384
```
