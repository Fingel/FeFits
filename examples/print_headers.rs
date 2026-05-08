use fefits::{header::Header, io::BlockReader};
use std::{env, fs::File};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = env::args()
        .nth(1)
        .expect("Usage: print_headers <fits_file>");

    let file = File::open(&path)?;
    let mut reader = BlockReader::new(file);
    let (header, blocks) = Header::read_from_block_reader(&mut reader)?;

    println!("File: {path}");
    println!("Header: {blocks} block(s), {} cards", header.len());
    println!("{}", "─".repeat(80));

    for card in header.cards() {
        println!("{card}");
    }

    Ok(())
}
