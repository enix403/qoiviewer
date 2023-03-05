use std::io::{Read, ErrorKind};
use std::ops::{Add, Sub};
use std::cell::RefCell;

type U8Array<const N: usize> = [u8; N];
type EndMarker = U8Array<8>;

const QOI_END_MARKER: EndMarker = [0, 0, 0, 0, 0, 0, 0, 1];

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub struct Pixel {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Pixel {
    fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    fn zero() -> Self {
        Self { r: 0, g: 0, b: 0, a: 0 }
    }

    fn hash_index(&self) -> usize {
        (( (self.r as usize) * 3
        +  (self.g as usize) * 5
        +  (self.b as usize) * 7
        +  (self.a as usize) * 11) % 64usize)
    }
}

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
struct WrappedU8(u8);
impl WrappedU8 {
    fn into_inner(self) -> u8 {
        self.0
    } 
}

impl Add<u8> for WrappedU8 {
    type Output = Self;

    fn add(self, rhs: u8) -> Self::Output {
        Self(self.0.wrapping_add(rhs))
    }
}

impl Sub<u8> for WrappedU8 {
    type Output = Self;

    fn sub(self, rhs: u8) -> Self::Output {
        Self(self.0.wrapping_sub(rhs))
    }
}

#[derive(Debug, Clone)]
enum SourceColorChannel {
    RGB,
    RGBA
}

#[derive(Debug, Clone)]
pub enum QOIChunk {
    Color(Pixel, SourceColorChannel), // Both RGB (with previous pixel's alpha) and RGBA
    Index(u8), // index is stored as 8 bit value with 6th and 7th unused
    Diff(u8, u8, u8), // dr, dg, db
    Luma { diff_green: u8, drdg: u8, dbdg: u8 },
    Run(u8)
}
const INVALID_CHUNK: QOIChunk = QOIChunk::Run(255);

const SEEN_ARRAY_SIZE: usize = 64;

pub struct ImageDecoder<R: Read> {
    // The image source
    source: R,

    // The QOI array of pixels
    seen: [Pixel; SEEN_ARRAY_SIZE],

    // Previous pixel
    prev: Pixel,

    window: [u8; 8],
    window_processed: usize,

    run_active: bool,
    run_length: u8,
}

#[derive(Debug, Clone)]
pub struct QOIHeader {
    pub width: u32,
    pub height: u32,
    pub channels: u8,
    pub colorspace: u8,
}

pub struct QOIImage {
    pub header: QOIHeader,
    pub pixels: Vec<Pixel>
}

pub enum ProcessedChunk {
    Ok(Pixel, QOIChunk),
    EndMarker,
    Invalid
}

impl<R: Read> ImageDecoder<R> {
    pub fn new(source: R) -> Self {
        Self {
            source: source,
            seen: [Pixel::zero(); SEEN_ARRAY_SIZE],
            prev: Pixel::new(0, 0, 0, 255),

            window: [0; 8],
            window_processed: 8,

            run_active: false,
            run_length: 0,
        }
    }

    fn verify_magic(buf: &[u8]) -> bool {
        match std::str::from_utf8(buf) {
            Ok(s) => s == "qoif",
            Err(_) => false
        }
    }

    fn decode_next_chunk(&self) -> Option<QOIChunk> {
        let tag = self.window[0];
        print!(" [W = {:02X?}, TG = {:#010b}] ", self.window, tag);

        let buf = &self.window[1..];

        let mut matched = true;

        let chunk: QOIChunk = match tag {
            0xFE => { /* RGB */
                QOIChunk::Color(Pixel::new(
                    buf[0],
                    buf[1],
                    buf[2],
                    self.prev.a
                ), SourceColorChannel::RGB)
            },

            0xFF => { /* RGBA */ 
                QOIChunk::Color(Pixel::new(
                    buf[0],
                    buf[1],
                    buf[2],
                    buf[3]
                ), SourceColorChannel::RGBA)
            },

            x if tag_2bit(x, 0b00) && self.window[1] != x => { /* INDEX */
                // The lower 6 bits of tag contain index 
                QOIChunk::Index(tag & 0x3F)
            },

            x if tag_2bit(x, 0b01) => { /* DIFF  */
                QOIChunk::Diff(
                    (tag >> 4) & 0x03,
                    (tag >> 2) & 0x03,
                    (tag >> 0) & 0x03
                )
            },

            x if tag_2bit(x, 0b10) => { /* LUMA  */
                let diffs = buf[0];

                QOIChunk::Luma { 
                    diff_green: tag & 0x3F,
                    drdg: (diffs >> 4) & 0x0F,
                    dbdg: (diffs >> 0) & 0x0F,
                }
            },

            x if tag_2bit(x, 0b11) => { /* RUN   */
                // The lower 6 bits of tag contain run length 
                QOIChunk::Run(tag & 0x3F)
            },

            _ => { matched = false; INVALID_CHUNK }
        };

        if matched {
            Some(chunk)
        } else {
            None
        }
    }

    fn transform_chunk(&self, chunk: QOIChunk) -> Pixel {
        // Pixel::zero()
        match chunk {
            QOIChunk::Color(p, _) => p,
            QOIChunk::Index(index) => self.seen[index as usize].clone(),
            QOIChunk::Diff(dr, dg, db) => Pixel::new(
                (WrappedU8(self.prev.r) + dr - 2).into_inner(),
                (WrappedU8(self.prev.g) + dg - 2).into_inner(),
                (WrappedU8(self.prev.b) + db - 2).into_inner(),
                self.prev.a
            ),
            QOIChunk::Luma { diff_green, drdg, dbdg } => Pixel::new(
                (WrappedU8(self.prev.r) + diff_green + drdg - 8).into_inner(),
                (WrappedU8(self.prev.g) + diff_green + 32).into_inner(),
                (WrappedU8(self.prev.b) + diff_green + drdg - 8).into_inner(),
                self.prev.a
            ),
            QOIChunk::Run(_) => unreachable!()
        }
    }

    pub fn next_chunk(&mut self) -> ProcessedChunk {

        if self.run_active {
            if self.run_length > 0 {
                let res = ProcessedChunk::Ok(self.prev.clone(), QOIChunk::Run(self.run_length));
                self.run_length -= 1;
                return res;
            }
            else {
                self.run_active = false;
            }
        }

        if self.window_processed > 0 {
            self.window.rotate_left(self.window_processed);
        }

        self.source.read_exact(&mut self.window[(8 - self.window_processed)..])
            .expect("Failed to read source");

        if &self.window[..] == &QOI_END_MARKER[..] {
            ProcessedChunk::EndMarker
        } else {
            match self.decode_next_chunk() {
                Some(mut chunk) => {
                    let processed = match chunk {
                        QOIChunk::Color(_, SourceColorChannel::RGB) => 4, 
                        QOIChunk::Color(_, SourceColorChannel::RGBA) => 5, 
                        QOIChunk::Index(..) => 1, 
                        QOIChunk::Diff(..) => 1, 
                        QOIChunk::Luma { .. } => 2, 
                        QOIChunk::Run(..) => 1,
                    };

                    self.window_processed = processed;

                    if let QOIChunk::Run(run_length) = &mut chunk {
                        // Un-bias the run length
                        *run_length += 1;
                        self.run_active = true;
                        self.run_length = *run_length - 1;
                    } else {
                        let pixel = self.transform_chunk(chunk.clone());
                        self.seen[pixel.hash_index()] = pixel.clone();
                        self.prev = pixel;
                    }

                    ProcessedChunk::Ok(self.prev.clone(), chunk)
                },
                None => ProcessedChunk::Invalid
            }   
        }
    }

    pub fn decode(&mut self) {
        let mut header_bytes = [0_u8; 14];

        match self.source.read_exact(&mut header_bytes[..]) {
            Ok(_) => {
                if !Self::verify_magic(&header_bytes[0..4]) {
                    println!("Not a QOI buffer [magic bytes invalid]");
                    return;
                }
            },
            Err(error) => { println!("Failed to read magic bytes"); return;; }
        }

        let header = QOIHeader {
            width: be_u32(&header_bytes[4..8]),
            height: be_u32(&header_bytes[8..12]),
            channels: header_bytes[12],
            colorspace: header_bytes[13],
        };

        println!("\n{:#?}\n", header);

        loop {
            match self.next_chunk() {
                ProcessedChunk::Ok(px, _) => {
                    println!("{:?}; P = {}", px, self.window_processed);
                },
                ProcessedChunk::EndMarker => { println!("End Marker"); break },
                ProcessedChunk::Invalid => { println!("Error next_chunk()"); break },
            }
        }
    }

}

fn be_u32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes(bytes.try_into().unwrap())
}

fn tag_2bit(x: u8, tag: u8) -> bool {
    const MASK: u8 = 0b_11_00_00_00_u8;
    (x & MASK) >> 6 == tag
}