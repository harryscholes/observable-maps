# priceholder-rs

A threadsafe hashmap type that notifies waiters of state changes.

### Usage

```rust
use std::{thread, time::Duration};

use priceholder::{PriceHolder, ThreadSafe};
use rust_decimal_macros::dec;

fn main() {
    let pi = dec!(3.14159265358979323846264338327950288419716939937510);

    // Types that are `PriceHolder` are generic, which allows them to store
    // arbitrary precision decimal types, for example.
    let mut ph = ThreadSafe::new();

    {
        // `ThreadSafe` is made threadsafe by encapsulating an `Arc`, which
        // automatically increases the reference count to the price holder
        // when it is cloned.
        let mut ph = ph.clone();

        // Spawn a new thread ...
        thread::spawn(move || {
            // ... that waits for some time ...
            thread::sleep(Duration::from_secs(1));
            // ... then puts a new price in the price holder for BTC.
            ph.put_price("BTC".to_string(), pi).unwrap();
            println!("Put price: {}", pi);
        })
    };

    // Wait for the price of BTC to be updated in the price holder, by
    // blocking execution of the thread.
    let price = ph.next_price("BTC".to_string()).unwrap();
    println!("Received price: {}", pi);
    assert_eq!(price, pi.clone());
}
```

Outputs:

```sh
$ cargo run --quiet main.rs
Put price: 3.1415926535897932384626433833
Received price: 3.1415926535897932384626433833
```