//! Read-only presentation-clock probe for Teamfight Manager 2 v0.5.8.
//!
//! Runtime Stage-2 testing rejected the earlier interpretation of `match_view + 0x250`: that
//! value remained `1` from visible 00:02 through 00:30. Static tracing had also identified the
//! adjacent `match_view + 0x258` field as the playback elapsed-time accumulator, so this probe
//! now targets that field instead of guessing another tick offset.
//!
//! The native type of the accumulator is not yet known. During the first ~1.5 seconds of a
//! watched match we therefore evaluate a small, explicit set of plausible encodings (integer
//! ticks/ms/us/ns, f32/f64 seconds or ticks, and Rust-style Duration). We score each encoding by
//! how closely its *delta* follows 1x wall-clock playback, then lock the best decoder for the
//! rest of that match. Once locked, no re-selection occurs; pause/speed tests can therefore
//! distinguish a real presentation clock from an unrelated wall timer.
//!
//! This module performs no mutation and no sleeping.

use std::sync::Mutex;

use windows_sys::Win32::System::Threading::GetTickCount64;

const CAMERA_EMBED_OFFSET: usize = 0x960;
const ACCUMULATOR_OFFSET: usize = 0x258;
const SECOND_WORD_OFFSET: usize = 0x260;
const LOCK_AFTER_MS: u64 = 1_500;
const MAX_REASONABLE_TICKS: f64 = 10_000_000.0;
const DECODER_COUNT: usize = 19;
const NO_DECODER: usize = usize::MAX;

#[derive(Debug, Clone, Copy)]
pub struct PlayedTickSnapshot {
    pub match_view_address: usize,
    pub played_tick: u64,
}

#[derive(Debug, Clone, Copy)]
struct DecoderState {
    match_view_address: usize,
    start_ms: u64,
    baseline: [f64; DECODER_COUNT],
    locked_decoder: usize,
}

impl DecoderState {
    const fn empty() -> Self {
        Self {
            match_view_address: 0,
            start_ms: 0,
            baseline: [f64::NAN; DECODER_COUNT],
            locked_decoder: NO_DECODER,
        }
    }
}

static STATE: Mutex<DecoderState> = Mutex::new(DecoderState::empty());

pub fn reset() {
    if let Ok(mut state) = STATE.lock() {
        *state = DecoderState::empty();
    }
}

pub fn read_from_camera_address(camera_address: usize) -> Option<PlayedTickSnapshot> {
    let match_view_address = camera_address.checked_sub(CAMERA_EMBED_OFFSET)?;
    if match_view_address == 0 {
        return None;
    }

    let candidates = unsafe { decode_candidates(match_view_address)? };
    let now_ms = unsafe { GetTickCount64() };

    let mut state = STATE.lock().ok()?;
    if state.match_view_address != match_view_address {
        state.match_view_address = match_view_address;
        state.start_ms = now_ms;
        state.baseline = candidates;
        state.locked_decoder = NO_DECODER;
    }

    let elapsed_ms = now_ms.saturating_sub(state.start_ms);
    let decoder = if state.locked_decoder != NO_DECODER {
        state.locked_decoder
    } else {
        let best = choose_decoder(&state.baseline, &candidates, elapsed_ms)?;
        if elapsed_ms >= LOCK_AFTER_MS {
            state.locked_decoder = best;
        }
        best
    };

    let tick = candidates[decoder];
    if !tick.is_finite() || !(0.0..=MAX_REASONABLE_TICKS).contains(&tick) {
        return None;
    }

    Some(PlayedTickSnapshot {
        match_view_address,
        played_tick: tick.round() as u64,
    })
}

fn choose_decoder(
    baseline: &[f64; DECODER_COUNT],
    current: &[f64; DECODER_COUNT],
    elapsed_ms: u64,
) -> Option<usize> {
    let expected_delta = elapsed_ms as f64 * 60.0 / 1_000.0;
    let mut best_index = None;
    let mut best_score = f64::INFINITY;

    for index in 0..DECODER_COUNT {
        let first = baseline[index];
        let now = current[index];
        if !first.is_finite() || !now.is_finite() || now < 0.0 || now > MAX_REASONABLE_TICKS {
            continue;
        }

        let delta = now - first;
        if delta < -1.0 {
            continue;
        }

        // Before enough wall time has elapsed, prefer plausible low absolute clocks and avoid
        // locking. Afterward, rate agreement dominates the score.
        let rate_error = (delta - expected_delta).abs();
        let absolute_penalty = if now > 100_000.0 { 1_000.0 } else { 0.0 };
        let static_penalty = if elapsed_ms >= 500 && delta.abs() < 0.5 {
            10_000.0
        } else {
            0.0
        };
        let score = rate_error + absolute_penalty + static_penalty + decoder_priority_penalty(index);

        if score < best_score {
            best_score = score;
            best_index = Some(index);
        }
    }

    best_index
}

fn decoder_priority_penalty(index: usize) -> f64 {
    // Static evidence specifically points at +0x258. Prefer decoders starting there over the
    // +0x260 fallback word when two interpretations fit equally well.
    if index <= 11 { 0.0 } else { 5.0 }
}

unsafe fn decode_candidates(match_view_address: usize) -> Option<[f64; DECODER_COUNT]> {
    let p0 = (match_view_address + ACCUMULATOR_OFFSET) as *const u8;
    let p8 = (match_view_address + SECOND_WORD_OFFSET) as *const u8;

    let u64_0 = std::ptr::read_unaligned(p0.cast::<u64>());
    let u64_8 = std::ptr::read_unaligned(p8.cast::<u64>());
    let u32_0 = std::ptr::read_unaligned(p0.cast::<u32>());
    let u32_8 = std::ptr::read_unaligned(p8.cast::<u32>());
    let f32_0 = f32::from_bits(u32_0) as f64;
    let f32_8 = f32::from_bits(u32_8) as f64;
    let f64_0 = f64::from_bits(u64_0);
    let f64_8 = f64::from_bits(u64_8);

    let duration_ticks = {
        let seconds = u64_0 as f64;
        let nanos = u32_8 as f64;
        if nanos < 1_000_000_000.0 {
            (seconds + nanos / 1_000_000_000.0) * 60.0
        } else {
            f64::NAN
        }
    };

    Some([
        duration_ticks,             // 0: Duration { secs @ +258, nanos @ +260 }
        u64_0 as f64,               // 1: u64 ticks @ +258
        u64_0 as f64 * 0.060,       // 2: u64 milliseconds @ +258
        u64_0 as f64 * 0.000_060,   // 3: u64 microseconds @ +258
        u64_0 as f64 * 0.000_000_060, // 4: u64 nanoseconds @ +258
        f64_0 * 60.0,               // 5: f64 seconds @ +258
        f64_0,                      // 6: f64 ticks @ +258
        u32_0 as f64,               // 7: u32 ticks @ +258
        u32_0 as f64 * 0.060,       // 8: u32 milliseconds @ +258
        u32_0 as f64 * 0.000_060,   // 9: u32 microseconds @ +258
        f32_0 * 60.0,               // 10: f32 seconds @ +258
        f32_0,                      // 11: f32 ticks @ +258
        u64_8 as f64,               // 12: u64 ticks @ +260
        u64_8 as f64 * 0.060,       // 13: u64 milliseconds @ +260
        u64_8 as f64 * 0.000_060,   // 14: u64 microseconds @ +260
        u64_8 as f64 * 0.000_000_060, // 15: u64 nanoseconds @ +260
        f64_8 * 60.0,               // 16: f64 seconds @ +260
        f32_8 * 60.0,               // 17: f32 seconds @ +260
        f32_8,                      // 18: f32 ticks @ +260
    ])
}
