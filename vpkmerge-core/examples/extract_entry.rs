// Dev throwaway: extract one raw entry from a VPK to a file so we can inspect
// the embedded material/texture path strings of a .vmdl_c.
// Usage: cargo run -p vpkmerge-core --example extract_entry -- <vpk> <entry> <out>
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let vpk = valve_pak::open(&args[1]).expect("open vpk");
    let bytes = vpk.read(&args[2]).expect("read entry");
    std::fs::write(&args[3], &bytes).expect("write out");
    eprintln!("wrote {} bytes", bytes.len());
}
