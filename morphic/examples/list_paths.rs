//  Throwaway: list all VPK entry paths matching a substring.
//  Usage: cargo run -p morphic --example list_paths -- <vpk> <substr>
use std::path::Path;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let vpk = valve_pak::open(Path::new(&a[1])).unwrap();
    let needle = a[2].to_lowercase();
    let mut v: Vec<String> = vpk
        .file_paths()
        .filter(|p| p.to_lowercase().contains(&needle))
        .cloned()
        .collect();
    v.sort();
    for p in &v {
        println!("{p}");
    }
    println!("--- {} entries ---", v.len());
}
