#[allow(unused_imports)]
use arena::Arena;

#[cfg(feature = "debug")]
pub fn main() {
    let mut arena = Arena::new().expect("Should construct a new arena");

    arena.alloc_str("wtf");

    {
        let s: &str = arena.alloc_str("test str").expect("Should allocate str");
        println!("Arena: {s} len: {}", s.len());
    };

    arena.dump();
}

#[cfg(not(any(feature = "debug", feature = "wasm")))]
pub fn main() {}
