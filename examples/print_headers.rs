use fefits::fits::Fits;
use std::env;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = env::args()
        .nth(1)
        .expect("Usage: print_headers <fits_file>");

    let mut fits = Fits::open(&path)?;
    println!("File: {path}");
    println!("{} HDUs", &fits.len());
    for i in 0..fits.len() {
        let header = fits.read_header(i).unwrap_or_default();
        println!("HDU {}: {} cards", i, header.len());
        println!("{}", "─".repeat(80));
        for card in header.cards() {
            println!("{card}");
        }
    }
    Ok(())
}
