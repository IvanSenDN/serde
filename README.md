# Serde (allocator_api fork)

**Fork of [serde-rs/serde](https://github.com/serde-rs/serde) with support for custom allocators via Rust nightly `allocator_api`.**

Serialization/deserialization into arena (e.g. [bumpalo](https://github.com/fitzgen/bumpalo)): `DeserializeIn<'de, A>`, `#[derive(DeserializeIn)]`, and format-specific APIs like `serde_json::from_str_in`.

---

## Requirements

- **Rust 1.93** or newer for standard usage.
- **Nightly** and `#![feature(allocator_api)]` for the allocator API (arena deserialization).

```toml
# rust-toolchain.toml (optional, for allocator_api)
[toolchain]
channel = "nightly"
```

---

## Standard usage (stable)

Same API as upstream Serde. Use this fork via git:

```toml
[dependencies]
serde = { git = "https://github.com/IvanSenDN/serde", features = ["derive"] }
serde_json = { git = "https://github.com/IvanSenDN/serde_json" }
```

```rust
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
struct Point {
    x: i32,
    y: i32,
}

fn main() {
    let point = Point { x: 1, y: 2 };
    let json = serde_json::to_string(&point).unwrap();
    let restored: Point = serde_json::from_str(&json).unwrap();
    println!("{:?}", restored);
}
```

---

## Allocator API (nightly)

Deserialize directly into an arena so all allocations use a custom allocator (e.g. `Bump`). Requires **nightly** and `#![feature(allocator_api)]`.

### Cargo.toml

```toml
[dependencies]
serde = { git = "https://github.com/IvanSenDN/serde", features = ["derive", "allocator_api"] }
serde_json = { git = "https://github.com/IvanSenDN/serde_json", features = ["allocator_api"] }
bumpalo = { version = "3", features = ["allocator_api", "collections"] }
```

### Example

Your struct must be generic over an allocator and use types that support it (e.g. `String<A>`, `Vec<T, A>`). Use `#[derive(DeserializeIn)]` and `serde::DeserializeIn`:

```rust
#![feature(allocator_api)]

use bumpalo::Bump;
use core::alloc::Allocator;
use serde::DeserializeIn;

// Your own string type over allocator A (or use bumpalo::collections::String)
pub struct String<A: Allocator> {
    vec: Vec<u8, A>,
}

impl<A: Allocator> String<A> {
    pub fn from_str_in(s: &str, alloc: A) -> Self {
        let mut v = Vec::new_in(alloc);
        v.extend_from_slice(s.as_bytes());
        Self { vec: v }
    }
    pub fn as_str(&self) -> &str {
        unsafe { core::str::from_utf8_unchecked(&self.vec) }
    }
}

// Implement DeserializeIn for your string (see serde docs for full visitor).
impl<'de, A: Allocator + Copy> serde::de::DeserializeIn<'de, A> for String<A> {
    fn deserialize_in<D>(deserializer: D, alloc: A) -> Result<Self, D::Error>
    where
        D: serde::de::Deserializer<'de>,
    {
        struct StringVisitor<A: Allocator>(A);
        impl<'de, A: Allocator + Copy> serde::de::Visitor<'de> for StringVisitor<A> {
            type Value = String<A>;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a string")
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(String::from_str_in(v, self.0))
            }
        }
        deserializer.deserialize_str(StringVisitor(alloc))
    }
}

#[derive(DeserializeIn)]
pub struct Person<A: Allocator> {
    name: String<A>,
    age: u8,
    jobs: Vec<String<A>, A>,
}

fn main() {
    let bump = Bump::new();
    let json = r#"{"name": "Alice", "age": 30, "jobs": ["Dev", "Lead"]}"#;

    let person: Person<&Bump> = serde_json::from_str_in(json, &bump).unwrap();

    println!("{}", person.name.as_str());
    println!("{}", person.age);
    println!("allocated: {} bytes", bump.allocated_bytes());
}
```

### Available APIs (when `allocator_api` feature is enabled)

- **serde:** trait `DeserializeIn<'de, A>`, `#[derive(DeserializeIn)]`, blanket impls for primitives and `Option`/`Vec`/`Box` with allocator.
- **serde_json:** `from_str_in`, `from_slice_in`, `from_reader_in` — same as `from_str` / `from_slice` / `from_reader` but take an allocator and require `T: DeserializeIn<'de, A>`.

---

## Note

These are **personal-use forks**. Development will follow only my own needs. I do not plan to publish to crates.io.

---

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.
