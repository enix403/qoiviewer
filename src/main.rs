#![allow(unreachable_code)]
#![allow(unused)]
#![allow(dead_code)]

mod decoder;

use std::fs::{File, OpenOptions};
use std::io;

use decoder::{ImageDecoder, EvaluatedChunk};

fn main() {

    // let path = "qoi_test_images/edgecase.qoi";
    let path = "qoi_test_images/kodim23.qoi";

    let file = OpenOptions::new()
        .read(true)
        .open(path)
        .expect(format!("Failed to open file: \"{}\"", path).as_str());

    let mut dec = ImageDecoder::new(file).unwrap();

    println!("{:#?}", dec.header());

    for c in dec.chunks_iter() {
        match c {
            EvaluatedChunk::Ok(px) => { println!("{:?}", px); },
            EvaluatedChunk::EndMarker => { println!("End Marker"); break; },
            _ => { break; }
        };
    }
}
