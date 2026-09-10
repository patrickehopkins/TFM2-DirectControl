//! Read-only accessor for the v0.5.8 match-view played/presentation tick.
//!
//! Static tracing established that the camera object captured by `camera_probe` is embedded at
//! `match_view + 0x960`, and that the currently played tick lives at `match_view + 0x250`.
//! This module keeps that private-layout assumption isolated and performs no mutation.

use std::ptr;

const CAMERA_EMBED_OFFSET: usize = 0x960;
const PLAYED_TICK_OFFSET: usize = 0x250;

// Far above any normal watched-match duration, but useful for rejecting an obviously wrong field.
const MAX_REASONABLE_PLAYED_TICK: usize = 10_000_000;

#[derive(Debug, Clone, Copy)]
pub struct PlayedTickSnapshot {
    pub match_view_address: usize,
    pub played_tick: u64,
}

pub fn read_from_camera_address(camera_address: usize) -> Option<PlayedTickSnapshot> {
    let match_view_address = camera_address.checked_sub(CAMERA_EMBED_OFFSET)?;
    if match_view_address == 0 {
        return None;
    }

    // The camera address comes from the known-live v0.5.8 camera handler and this function is only
    // called from the InGame render path. We still keep the read narrow and reject implausible data.
    let played_tick = unsafe {
        ptr::read_unaligned(
            (match_view_address + PLAYED_TICK_OFFSET) as *const usize,
        )
    };

    if played_tick > MAX_REASONABLE_PLAYED_TICK {
        return None;
    }

    Some(PlayedTickSnapshot {
        match_view_address,
        played_tick: played_tick as u64,
    })
}
