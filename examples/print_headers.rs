use fefits::fits::Fits;
use std::env;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = env::args()
        .nth(1)
        .expect("Usage: print_headers <fits_file>");

    let mut fits = Fits::open(&path)?;
    if !fits.is_empty() {
        let header = fits.read_header(0)?;
        println!("File: {path}");
        println!("Header: {} cards", header.len());
        println!("{}", "─".repeat(80));

        for card in header.cards() {
            println!("{card}");
        }
    }
    Ok(())
}
