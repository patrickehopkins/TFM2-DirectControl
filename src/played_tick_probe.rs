//! Read-only presentation-clock scanner for Teamfight Manager 2 v0.5.8.
//!
//! Physical testing rejected the earlier `match_view + 0x250` played-tick hypothesis and the
//! adjacent `+0x258/+0x260` decoder also failed closed. Rather than burn one test per guessed
//! offset, this probe scans the entire known-live match-view prefix before the embedded camera
//! object (`0x000..0x960`) and ranks scalar fields by how closely their change matches the game's
//! visible MM:SS clock at 1x playback.
//!
//! The scanner considers common integer/floating-point time representations and keeps only the
//! best interpretation per offset. It performs reads only; no field is written and no thread is
//! delayed. A strong candidate here is evidence for a presentation clock, not permission to use
//! it for pacing until pause/speed behavior is physically verified.

use std::{cmp::Ordering, sync::Mutex};

const CAMERA_EMBED_OFFSET: usize = 0x960;
const SCAN_END_OFFSET: usize = CAMERA_EMBED_OFFSET;
const WORD_BYTES: usize = 4;
const WORD_COUNT: usize = SCAN_END_OFFSET / WORD_BYTES;
const TOP_COUNT: usize = 5;
const MAX_REASONABLE_TICKS: f64 = 10_000_000.0;

#[derive(Debug, Clone)]
pub struct ClockScanSnapshot {
    pub match_view_address: usize,
    pub visible_seconds: u64,
    pub elapsed_visible_seconds: u64,
    pub candidates: Vec<ClockCandidate>,
}

#[derive(Debug, Clone, Copy)]
pub struct ClockCandidate {
    pub offset: usize,
    pub kind: &'static str,
    pub ticks: f64,
    pub delta_ticks: f64,
    pub error_ticks: f64,
}

#[derive(Debug)]
struct ScanState {
    match_view_address: usize,
    start_visible_seconds: u64,
    baseline_words: Vec<u32>,
}

static STATE: Mutex<Option<ScanState>> = Mutex::new(None);

pub fn reset() {
    if let Ok(mut state) = STATE.lock() {
        *state = None;
    }
}

pub fn scan_from_camera_address(
    camera_address: usize,
    visible_clock: &str,
) -> Option<ClockScanSnapshot> {
    let visible_seconds = parse_visible_seconds(visible_clock)?;
    let match_view_address = camera_address.checked_sub(CAMERA_EMBED_OFFSET)?;
    if match_view_address == 0 {
        return None;
    }

    let words = unsafe { read_words(match_view_address) };
    let mut state_guard = STATE.lock().ok()?;

    let needs_reset = state_guard
        .as_ref()
        .map(|state| {
            state.match_view_address != match_view_address
                || visible_seconds < state.start_visible_seconds
                || state.baseline_words.len() != WORD_COUNT
        })
        .unwrap_or(true);

    if needs_reset {
        *state_guard = Some(ScanState {
            match_view_address,
            start_visible_seconds: visible_seconds,
            baseline_words: words.clone(),
        });
    }

    let state = state_guard.as_ref()?;
    let elapsed_visible_seconds = visible_seconds.saturating_sub(state.start_visible_seconds);
    let expected_delta = elapsed_visible_seconds as f64 * 60.0;
    let mut candidates = Vec::new();

    for index in 0..WORD_COUNT {
        let offset = index * WORD_BYTES;
        if let Some(candidate) = best_candidate_for_offset(
            offset,
            index,
            &state.baseline_words,
            &words,
            expected_delta,
        ) {
            candidates.push(candidate);
        }
    }

    candidates.sort_by(|a, b| {
        a.error_ticks
            .partial_cmp(&b.error_ticks)
            .unwrap_or(Ordering::Equal)
            .then_with(|| a.offset.cmp(&b.offset))
    });
    candidates.truncate(TOP_COUNT);

    Some(ClockScanSnapshot {
        match_view_address,
        visible_seconds,
        elapsed_visible_seconds,
        candidates,
    })
}

fn parse_visible_seconds(clock: &str) -> Option<u64> {
    let parts: Vec<&str> = clock.trim().split(':').collect();
    match parts.as_slice() {
        [minutes, seconds] => Some(minutes.parse::<u64>().ok()? * 60 + seconds.parse::<u64>().ok()?),
        [hours, minutes, seconds] => Some(
            hours.parse::<u64>().ok()? * 3600
                + minutes.parse::<u64>().ok()? * 60
                + seconds.parse::<u64>().ok()?,
        ),
        _ => None,
    }
}

unsafe fn read_words(match_view_address: usize) -> Vec<u32> {
    let mut words = Vec::with_capacity(WORD_COUNT);
    for index in 0..WORD_COUNT {
        let address = (match_view_address + index * WORD_BYTES) as *const u32;
        words.push(std::ptr::read_unaligned(address));
    }
    words
}

fn best_candidate_for_offset(
    offset: usize,
    index: usize,
    baseline: &[u32],
    current: &[u32],
    expected_delta: f64,
) -> Option<ClockCandidate> {
    let first32 = baseline[index];
    let now32 = current[index];
    let first_f32 = f32::from_bits(first32) as f64;
    let now_f32 = f32::from_bits(now32) as f64;

    let mut interpretations: Vec<(&'static str, f64, f64)> = vec![
        ("u32 ticks", first32 as f64, now32 as f64),
        ("u32 ms", first32 as f64 * 0.060, now32 as f64 * 0.060),
        (
            "u32 us",
            first32 as f64 * 0.000_060,
            now32 as f64 * 0.000_060,
        ),
        ("f32 sec", first_f32 * 60.0, now_f32 * 60.0),
        ("f32 ticks", first_f32, now_f32),
    ];

    if index + 1 < WORD_COUNT {
        let first64 = baseline[index] as u64 | ((baseline[index + 1] as u64) << 32);
        let now64 = current[index] as u64 | ((current[index + 1] as u64) << 32);
        let first_f64 = f64::from_bits(first64);
        let now_f64 = f64::from_bits(now64);

        interpretations.extend_from_slice(&[
            ("u64 ticks", first64 as f64, now64 as f64),
            ("u64 ms", first64 as f64 * 0.060, now64 as f64 * 0.060),
            (
                "u64 us",
                first64 as f64 * 0.000_060,
                now64 as f64 * 0.000_060,
            ),
            (
                "u64 ns",
                first64 as f64 * 0.000_000_060,
                now64 as f64 * 0.000_000_060,
            ),
            ("f64 sec", first_f64 * 60.0, now_f64 * 60.0),
            ("f64 ticks", first_f64, now_f64),
        ]);
    }

    if index + 2 < WORD_COUNT {
        let secs = baseline[index] as u64 | ((baseline[index + 1] as u64) << 32);
        let now_secs = current[index] as u64 | ((current[index + 1] as u64) << 32);
        let nanos = baseline[index + 2] as u64;
        let now_nanos = current[index + 2] as u64;
        if nanos < 1_000_000_000 && now_nanos < 1_000_000_000 {
            interpretations.push((
                "Duration",
                (secs as f64 + nanos as f64 / 1_000_000_000.0) * 60.0,
                (now_secs as f64 + now_nanos as f64 / 1_000_000_000.0) * 60.0,
            ));
        }
    }

    let mut best: Option<ClockCandidate> = None;
    for (kind, first_ticks, now_ticks) in interpretations {
        if !first_ticks.is_finite()
            || !now_ticks.is_finite()
            || first_ticks < -1.0
            || now_ticks < -1.0
            || first_ticks > MAX_REASONABLE_TICKS
            || now_ticks > MAX_REASONABLE_TICKS
        {
            continue;
        }

        let delta_ticks = now_ticks - first_ticks;
        if delta_ticks < -1.0 {
            continue;
        }

        let mut error_ticks = (delta_ticks - expected_delta).abs();

        // Once at least two visible seconds have elapsed, static/nearly-static fields should
        // never outrank a genuine presentation clock simply because of a lucky representation.
        if expected_delta >= 120.0 && delta_ticks < expected_delta * 0.10 {
            error_ticks += 10_000.0;
        }

        // A presentation clock should begin near the visible match origin. Keep this penalty
        // deliberately small so a modest initialization offset does not hide a correct field.
        if first_ticks > 600.0 {
            error_ticks += 100.0;
        } else {
            error_ticks += first_ticks.abs() * 0.01;
        }

        let candidate = ClockCandidate {
            offset,
            kind,
            ticks: now_ticks,
            delta_ticks,
            error_ticks,
        };

        if best
            .as_ref()
            .map(|current_best| candidate.error_ticks < current_best.error_ticks)
            .unwrap_or(true)
        {
            best = Some(candidate);
        }
    }

    best
}
