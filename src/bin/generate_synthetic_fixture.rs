use std::{
    fs,
    io::{BufWriter, Write},
    path::PathBuf,
};
fn main() -> std::io::Result<()> {
    let directory = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("fixtures/synthetic"));
    fs::create_dir_all(&directory)?;
    let path = directory.join("R_100.csv");
    let mut out = BufWriter::new(fs::File::create(&path)?);
    writeln!(out, "epoch,quote,ask,bid,pip_size")?;
    let mut price_micro: i64 = 100_000_000;
    for index in 0..2_000_u64 {
        let wave = ((index.wrapping_mul(17) % 31) as i64) - 15;
        let trend = if (index / 250) % 2 == 0 { 23 } else { -19 };
        price_micro = (price_micro + wave + trend).max(1_000_000);
        let quote = price_micro as f64 / 1_000_000.0;
        let ask = (price_micro + 10) as f64 / 1_000_000.0;
        let bid = (price_micro - 10) as f64 / 1_000_000.0;
        writeln!(
            out,
            "{},{quote:.6},{ask:.6},{bid:.6},6",
            1_700_000_000 + index
        )?;
    }
    out.flush()?;
    eprintln!("{}", path.display());
    Ok(())
}
