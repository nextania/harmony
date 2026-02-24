use flate2::Compression;
use rand::distr::Alphanumeric;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use rmp_serde::{Deserializer, Serializer};
use serde::{Deserialize, Serialize};

use std::io::{Cursor, Read};

pub fn random_number(size: usize) -> Vec<u8> {
    let mut rng = StdRng::from_os_rng();
    let mut result: Vec<u8> = vec![0; size];
    rng.fill(&mut result[..]);
    result
}

pub fn generate_token() -> String {
    let rng = rand::rng();
    rng.sample_iter(&Alphanumeric)
        .take(64)
        .map(char::from)
        .collect()
}

pub fn generate(alphabet: &[char], size: usize) -> String {
    assert!(
        alphabet.len() <= u8::MAX as usize,
        "The alphabet cannot be longer than a `u8` (to comply with the `random` function)"
    );
    let mask = alphabet.len().next_power_of_two() - 1;
    let step: usize = 8 * size / 5;
    let mut id = String::with_capacity(size);
    loop {
        let bytes = random_number(step);
        for &byte in &bytes {
            let byte = byte as usize & mask;
            if alphabet.len() > byte {
                id.push(alphabet[byte]);
                if id.len() == size {
                    return id;
                }
            }
        }
    }
}

pub fn generate_id() -> String {
    const ALPHABET: &[char] = &[
        'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r',
        's', 't', 'u', 'v', 'w', 'x', 'y', 'z',
    ];
    const LENGTH: usize = 10;
    generate(ALPHABET, LENGTH)
}

pub fn compress(buffer: Vec<u8>) -> Vec<u8> {
    let zlib = ZlibEncoder::new(buffer, Compression::best());
    zlib.finish().unwrap()
}

pub fn decompress(buffer: Vec<u8>) -> Vec<u8> {
    let mut zlib = ZlibDecoder::new(Cursor::new(buffer));
    let mut buf = Vec::new();
    zlib.read_to_end(&mut buf).unwrap();
    buf
}
pub fn serialize<T: Serialize>(value: &T) -> Result<Vec<u8>, rmp_serde::encode::Error> {
    let mut buf = Vec::new();
    value.serialize(&mut Serializer::new(&mut buf).with_struct_map())?;
    Ok(buf)
}

pub fn deserialize<T: for<'a> Deserialize<'a>>(buf: &[u8]) -> Result<T, rmp_serde::decode::Error> {
    let mut deserializer = Deserializer::new(buf);
    Deserialize::deserialize(&mut deserializer)
}
