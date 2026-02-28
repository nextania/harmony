use std::collections::VecDeque;

use crate::media::audio::{CHANNELS, SAMPLE_RATE};

pub struct Limiter {
    threshold: f32,
    attack_coeff: f32,
    release_coeff: f32,
    lookahead_buf: VecDeque<f32>,
    lookahead_samples: usize,
    gain_db: f32,
}

impl Limiter {
    pub fn new() -> Self {
        let sr = SAMPLE_RATE as f32;
        let attack_ms = 1.0_f32;
        let release_ms = 50.0_f32;
        let lookahead_ms = 5.0_f32;

        let attack_coeff = (-1.0 / (attack_ms * 0.001 * sr)).exp();
        let release_coeff = (-1.0 / (release_ms * 0.001 * sr)).exp();

        let lookahead_samples = ((lookahead_ms * 0.001 * sr) as usize) * CHANNELS as usize;

        Self {
            threshold: 10.0_f32.powf(-0.1 / 20.0),
            attack_coeff,
            release_coeff,
            lookahead_buf: VecDeque::with_capacity(lookahead_samples + 256),
            lookahead_samples,
            gain_db: 0.0,
        }
    }

    pub fn process(&mut self, data: &mut [f32]) {
        self.lookahead_buf.extend(data.iter().copied());

        let out_len = data.len();
        let mut write_idx = 0;

        while write_idx + 1 < out_len {
            if self.lookahead_buf.len() < self.lookahead_samples + 2 {
                for s in &mut data[write_idx..] {
                    *s = 0.0;
                }
                return;
            }

            let left = self.lookahead_buf.pop_front().unwrap();
            let right = self.lookahead_buf.pop_front().unwrap();

            let mut peak = left.abs().max(right.abs());
            for i in (0..self.lookahead_samples.min(self.lookahead_buf.len())).step_by(2) {
                let l = self.lookahead_buf[i].abs();
                let r = if i + 1 < self.lookahead_buf.len() {
                    self.lookahead_buf[i + 1].abs()
                } else {
                    0.0
                };
                peak = peak.max(l).max(r);
            }

            let target_db = if peak > self.threshold {
                let over_db = 20.0 * (peak / self.threshold).log10();
                -over_db
            } else {
                0.0
            };

            let coeff = if target_db < self.gain_db {
                self.attack_coeff
            } else {
                self.release_coeff
            };
            self.gain_db = coeff * self.gain_db + (1.0 - coeff) * target_db;

            let gain_linear = 10.0_f32.powf(self.gain_db / 20.0);

            data[write_idx] = left * gain_linear;
            data[write_idx + 1] = right * gain_linear;
            write_idx += 2;
        }
    }
}
