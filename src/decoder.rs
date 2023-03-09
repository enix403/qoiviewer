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

    pub fn to_channels4(&self) -> [u8; 4] {
        [self.r, self.g, self.b, self.a]
    }

    pub fn to_channels3(&self) -> [u8; 3] {
        [self.r, self.g, self.b]
    }

    pub fn to_channels4_iter(self) -> PixelChannelIterator {
        PixelChannelIterator { px: self, channels: 4, counter: 0 }
    }

    pub fn to_channels3_iter(self) -> PixelChannelIterator {
        PixelChannelIterator { px: self, channels: 3, counter: 0 }
    }

    pub fn to_rgba32(&self) -> u32 {
        u32::from_be_bytes([self.r, self.g, self.b, self.a])
    }
}

pub struct PixelChannelIterator {
    px: Pixel,
    channels: u8,
    counter: u8
}

impl Iterator for PixelChannelIterator {
    type Item = u8;
    fn next(&mut self) -> Option<Self::Item> {

        let val = match (self.channels, self.counter) {
            (3, 0) => Some(self.px.r),
            (3, 1) => Some(self.px.g),
            (3, 2) => Some(self.px.b),

            (4, 0) => Some(self.px.r),
            (4, 1) => Some(self.px.g),
            (4, 2) => Some(self.px.b),
            (4, 3) => Some(self.px.a),
            _ => None
        };

        // let val = if self.channels == 3 {
        //     match self.counter {
        //         0 => Some(self.px.r),
        //         1 => Some(self.px.g),
        //         2 => Some(self.px.b),
        //         _ => None
        //     }
        // } else if self.channels == 4 {
        //     match self.counter {
        //         0 => Some(self.px.r),
        //         1 => Some(self.px.g),
        //         2 => Some(self.px.b),
        //         3 => Some(self.px.a),
        //         _ => None
        //     }
        // };

        self.counter += 1;

        val
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
pub enum QOIChunk {
    ColorRGB(Pixel),
    ColorRGBA(Pixel),
    Index(u8), // index is stored as 8 bit value with 6th and 7th unused
    Diff(u8, u8, u8), // dr, dg, db
    Luma { diff_green: u8, drdg: u8, dbdg: u8 },
    Run(u8)
}

const INVALID_CHUNK: QOIChunk = QOIChunk::Run(255);

impl QOIChunk {
    /* Number of bytes the chunk consumes */
    const fn get_size(&self) -> usize {
        match self {
            QOIChunk::ColorRGB(..)  => 4,
            QOIChunk::ColorRGBA(..) => 5,
            QOIChunk::Index(..)     => 1,
            QOIChunk::Diff(..)      => 1,
            QOIChunk::Luma { .. }   => 2,
            QOIChunk::Run(..)       => 1,
        }
    }
}

#[derive(Debug, Clone)]
pub struct QOIHeader {
    pub width: u32,
    pub height: u32,
    pub channels: u8,
    pub colorspace: u8,
}

pub struct ImageDecoder<R> {
    source: R,
    header: QOIHeader
}

impl<R: Read> ImageDecoder<R> {
    pub fn new(mut source: R) -> Result<Self, QOIError> {
        let header = Self::parse_header(&mut source)?;

        Ok(Self { source, header })
    }

    fn verify_magic(buf: &[u8]) -> bool {
        match std::str::from_utf8(buf) {
            Ok(s) => s == "qoif",
            Err(_) => false
        }
    }

    fn parse_header(source: &mut R) -> Result<QOIHeader, QOIError> {
        let mut header_bytes = [0_u8; 14];

        source
            .read_exact(&mut header_bytes[..])
            .map_err(|err| QOIError::IO(err))
            .and_then(|_| {
                Self::verify_magic(&header_bytes[0..4])
                    .then(|| QOIHeader {
                        width: be_u32(&header_bytes[4..8]),
                        height: be_u32(&header_bytes[8..12]),
                        channels: header_bytes[12],
                        colorspace: header_bytes[13],
                    })
                    .ok_or(QOIError::IncorrectMagic)
            })
    }

    pub fn chunks_iter(self) -> DecodeChunks<R> {
        DecodeChunks::new(self)
    }

    pub fn header<'a>(&'a self) -> &'a QOIHeader {
        &self.header
    }
}

#[derive(Debug)]
pub enum QOIError {
    IO(std::io::Error),
    IncorrectMagic
}

pub enum EvaluatedChunk {
    Ok(Pixel),
    EndMarker,
    Faulty(String)
}

const SEEN_ARRAY_SIZE: usize = 64;

pub struct DecodeChunks<R> {
    decoder: ImageDecoder<R>,
    ended: bool,

    prev: Pixel, // Previous pixel
    seen: [Pixel; SEEN_ARRAY_SIZE], // The QOI array of pixels 

    window: [u8; 8],
    window_processed: usize,

    run_active: bool,
    run_length: u8,
}

impl<R> DecodeChunks<R>
where
    R: Read
{
    fn new(decoder: ImageDecoder<R>) -> Self {
        Self {
            decoder: decoder,
            ended: false,

            seen: [Pixel::zero(); SEEN_ARRAY_SIZE],
            prev: Pixel::new(0, 0, 0, 255),

            window: [0; 8],
            window_processed: 8,

            run_active: false,
            run_length: 0,
        }
    }

    fn decode_next_chunk(&self) -> Option<QOIChunk> {
        let tag = self.window[0];
        // print!(" [W = {:02X?}, TG = {:#010b}] ", self.window, tag);

        let buf = &self.window[1..];

        let mut matched = true;

        let chunk: QOIChunk = match tag {
            /* QOI_OP_RGB */
            0xFE => {
                QOIChunk::ColorRGB(Pixel::new(
                    buf[0],
                    buf[1],
                    buf[2],
                    self.prev.a
                ))
            },

            /* QOI_OP_RGBA */
            0xFF => {
                QOIChunk::ColorRGBA(Pixel::new(
                    buf[0],
                    buf[1],
                    buf[2],
                    buf[3]
                ))
            },

            /* QOI_OP_INDEX */
            // Consective OP_INDEX's to same index are not allowed
            x if tag_2bit(x, 0b00) && self.window[1] != x => {
                // The lower 6 bits of tag contain index 
                QOIChunk::Index(tag & 0x3F)
            },

            /* QOI_OP_DIFF */
            x if tag_2bit(x, 0b01) => {
                QOIChunk::Diff(
                    ((tag >> 4) & 0x03).wrapping_sub(2),
                    ((tag >> 2) & 0x03).wrapping_sub(2),
                    ((tag     ) & 0x03).wrapping_sub(2)
                )
            },

            /* QOI_OP_LUMA */
            x if tag_2bit(x, 0b10) => {
                let diffs = buf[0];

                QOIChunk::Luma { 
                    diff_green: (tag & 0x3F).wrapping_sub(32),   // Unbias by 32
                    drdg: ((diffs >> 4) & 0x0F).wrapping_sub(8), // Unbias by 8
                    dbdg: ((diffs     ) & 0x0F).wrapping_sub(8), // Unbias by 8
                }
            },

            /* QOI_OP_RUN */
            x if tag_2bit(x, 0b11) => {
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
        match chunk {
            QOIChunk::ColorRGB(p) | QOIChunk::ColorRGBA(p) => p,
            QOIChunk::Index(index) => self.seen[index as usize].clone(),
            QOIChunk::Diff(dr, dg, db) => Pixel::new(
                // Unbiasing
                (WrappedU8(self.prev.r) + dr).into_inner(),
                (WrappedU8(self.prev.g) + dg).into_inner(),
                (WrappedU8(self.prev.b) + db).into_inner(),
                self.prev.a
            ),
            QOIChunk::Luma { diff_green, drdg, dbdg } => Pixel::new(
                (WrappedU8(self.prev.r) + diff_green + drdg).into_inner(),
                (WrappedU8(self.prev.g) + diff_green).into_inner(),
                (WrappedU8(self.prev.b) + diff_green + dbdg).into_inner(),
                self.prev.a
            ),

            // This must be handled by the caller by repeatedly emitting previous
            // pixel if it encounters a RUN chunk 
            QOIChunk::Run(..) => unreachable!()
        }
    }

    pub fn next_chunk(&mut self) -> EvaluatedChunk {
        if self.run_active {
            if self.run_length > 0 {
                self.run_length -= 1;
                return EvaluatedChunk::Ok(self.prev.clone());
            }
            else {
                self.run_active = false;
            }
        }

        if self.window_processed > 0 {
            self.window.rotate_left(self.window_processed);
        }

        self.decoder
            .source
            .read_exact(&mut self.window[(8 - self.window_processed)..])
            .expect("Failed to read source");

        if &self.window[..] == &QOI_END_MARKER[..] {
            EvaluatedChunk::EndMarker
        } else {
            match self.decode_next_chunk() {
                Some(mut chunk) => {
                    self.window_processed = chunk.get_size();

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

                    EvaluatedChunk::Ok(self.prev.clone())
                },
                None => EvaluatedChunk::Faulty(format!("Unrecognized chunk"))
            }   
        }
    }
}

impl<R> Iterator for DecodeChunks<R>
where
    R: Read
{
    type Item = Result<Pixel, String>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.next_chunk() {
            EvaluatedChunk::Ok(px) => Some(Ok(px)),
            EvaluatedChunk::EndMarker => None ,
            EvaluatedChunk::Faulty(s) => Some(Err(s))
        }
        // let chunk = self.next_chunk();

        // match chunk {
            // EvaluatedChunk::Ok(..) => {},
            // _ => { self.ended = true; }
        // };

        // Some(chunk)
    }
}

fn be_u32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes(bytes.try_into().unwrap())
}

fn tag_2bit(x: u8, tag: u8) -> bool {
    const MASK: u8 = 0b_11_00_00_00_u8;
    (x & MASK) >> 6 == tag
}