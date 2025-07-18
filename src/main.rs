use arena::Arena;

pub fn main() {
    let mut arena = Arena::new(4096).expect("Should construct a new arena");

    let items: &mut Vec<i32> = arena
        .alloc(Vec::<i32>::new())
        .expect("Should allocate vector");

    for i in 0..5 {
        items.push(i);
    }

    let s_len = {
        let s: &str = arena.alloc_str("test str").expect("Should allocate str");
        println!("Arena: {s} len: {}", s.len());
        s.len()
    };

    assert_eq!(arena.len(), size_of::<Vec<i32>>() + s_len);
    println!("Arena: using {} of {} bytes", arena.len(), arena.cap());
}
