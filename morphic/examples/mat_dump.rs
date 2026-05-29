//  Throwaway: dump a .vmat_c's texture parameters (slot -> .vtex path).
//  Usage: cargo run -p morphic --example mat_dump -- <vpk> <vmat_c path>
use std::path::Path;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let vpk = valve_pak::open(Path::new(&a[1])).unwrap();
    let bytes = vpk.get_file(&a[2]).unwrap().read_all().unwrap();
    let mat = morphic::material::parse(&bytes).unwrap();
    println!("texture params for {}:", a[2]);
    // Material exposes texture(slot); dump via Debug of the whole struct
    println!("{mat:#?}");
}
