use std::path::Path;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let vpk = valve_pak::open(Path::new(&a[1])).unwrap();
    let b = vpk.get_file(&a[2]).unwrap().read_all().unwrap();
    let info = morphic::inspect(&b).unwrap();
    println!(
        "{:?} {}x{} mips={}",
        info.format, info.width, info.height, info.mip_count
    );
}
