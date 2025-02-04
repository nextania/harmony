use rand::{rngs::StdRng, Rng, SeedableRng};

pub fn random_number(size: usize) -> Vec<u8> {
    let mut rng = StdRng::from_entropy();
    let mut result: Vec<u8> = vec![0; size];
    rng.fill(&mut result[..]);
    result
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

use lazy_static::lazy_static;

lazy_static! {
    pub static ref HEARTBEAT_INTERVAL: u64 = 30000;
    pub static ref HEARTBEAT_TIMEOUT: u64 = 60000;
}
