mod converter;

use crate::converter::convert;
use anyhow::Error;
use clap::Parser;
use std::fs::File;
use std::io::{self, BufRead};
use std::result::Result;

/// Parse a markdown file and generate a minimally styled LaTeX file,
/// written to standard out
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Markdown file to parse
    #[arg(short, long)]
    filename: String,
}

fn main() -> Result<(), Error> {
    let args = Args::parse();
    convert(io::BufReader::new(File::open(&args.filename)?).lines())
        .for_each(|processed_line| print!("{}", processed_line));
    Ok(())
}
