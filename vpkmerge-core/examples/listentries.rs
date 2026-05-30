//! List entries in a VPK. Usage: cargo run --example listentries -- <vpk>
use vpkmerge_core::inspect;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let vpk = args.get(1).expect("usage: listentries <vpk>");
    let info = inspect(vpk).expect("inspect");
    for e in &info.entries {
        println!("{} ({} bytes)", e.path, e.size);
    }
}
