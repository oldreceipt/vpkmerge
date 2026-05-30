fn main() -> anyhow::Result<()> {
    let mut a = std::env::args().skip(1);
    let base = valve_pak::open(&a.next().unwrap())?;
    let addon = valve_pak::open(&a.next().unwrap())?;
    for e in a {
        let b = base.get_file(&e).unwrap().read_all()?;
        let r = addon.get_file(&e).unwrap().read_all()?;
        let diff = b.iter().zip(&r).filter(|(x, y)| x != y).count();
        println!(
            "  {} base={} addon={} {}  diffbytes={}",
            e,
            b.len(),
            r.len(),
            if b.len() == r.len() {
                "SAME-SIZE"
            } else {
                "SIZE-CHANGED"
            },
            diff
        );
    }
    Ok(())
}
