//! perception/world.rs — the WORLD MODEL rendezvous (sense-market doctrine, 2026-07-08).
//!
//! All senses feed ONE place at their natural cadence; the dispatcher reads MEASURED
//! fitness facts from here (never a learned router). This is also where the adiabatic
//! idea is finally done right: an a11y read is VALID-UNTIL-DAMAGED — invalidation by
//! membrane push (damage rects from the feed's event ring), not by hashing or polling.
//!
//! v1 discipline: MEASURE FIRST. The agent loop logs staleness/coverage facts at every
//! perceive; the skip-optimization (reuse a fresh a11y read) flips on only after the
//! logged data proves it safe.
//!
//! Event ring (/dev/shm/lagado_events, written by canvas_feed.py):
//!   "LGEV" | u64 count, then 4096 fixed 16-byte slots (f64 t | u16 x,y,w,h),
//!   slot = (count-1) % 4096. Readers keep their own cursor and diff `count`.

use std::io::Read;

const EV_MAGIC: &[u8; 4] = b"LGEV";
const EV_REC: usize = 16;
const EV_MAX: u64 = 4096;

pub fn events_path() -> String {
    std::env::var("LAGADO_EVENTS").unwrap_or_else(|_| "/dev/shm/lagado_events".to_string())
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DamageRect {
    pub t: f64,
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

/// Read damage events newer than `cursor`; returns (events, new_cursor).
/// If more than EV_MAX arrived since the cursor, the ring wrapped past us — return
/// the OVERFLOWED marker (caller must treat everything as damaged; fail-closed).
pub fn read_events(cursor: u64) -> Option<(Vec<DamageRect>, u64, bool)> {
    let mut f = std::fs::File::open(events_path()).ok()?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).ok()?;
    if buf.len() < 12 || &buf[..4] != EV_MAGIC {
        return None;
    }
    let count = u64::from_le_bytes(buf[4..12].try_into().ok()?);
    if count <= cursor {
        return Some((vec![], count, false));
    }
    let overflowed = count - cursor > EV_MAX;
    let first = if overflowed { count - EV_MAX } else { cursor };
    let mut out = Vec::new();
    for i in first..count {
        let slot = (i % EV_MAX) as usize;
        let base = 12 + slot * EV_REC;
        if buf.len() < base + EV_REC {
            break;
        }
        out.push(DamageRect {
            t: f64::from_le_bytes(buf[base..base + 8].try_into().ok()?),
            x: u16::from_le_bytes(buf[base + 8..base + 10].try_into().ok()?),
            y: u16::from_le_bytes(buf[base + 10..base + 12].try_into().ok()?),
            w: u16::from_le_bytes(buf[base + 12..base + 14].try_into().ok()?),
            h: u16::from_le_bytes(buf[base + 14..base + 16].try_into().ok()?),
        });
    }
    Some((out, count, overflowed))
}

fn intersects(r: &DamageRect, win: (i32, i32, i32, i32)) -> bool {
    let (wx, wy, ww, wh) = win;
    (r.x as i32) < wx + ww && wx < r.x as i32 + r.w as i32
        && (r.y as i32) < wy + wh && wy < r.y as i32 + r.h as i32
}

/// The rendezvous. One instance lives in the agent loop; senses note their reads,
/// the dispatcher (and today, the chronos log) reads facts.
pub struct WorldModel {
    ev_cursor: u64,
    has_read: bool,
    /// canvas seq at the moment of the last a11y read (informational)
    a11y_canvas_seq: u64,
    /// union bbox of the last a11y read's elements (screen px); None = whole screen
    a11y_window: Option<(i32, i32, i32, i32)>,
    a11y_elements: usize,
    cv_boxes: usize,
    /// damage rects that arrived since the last a11y read and INTERSECT its window
    damage_since_read: usize,
    ring_overflowed: bool,
}

impl WorldModel {
    pub fn new() -> Self {
        Self {
            ev_cursor: 0,
            has_read: false,
            a11y_canvas_seq: 0,
            a11y_window: None,
            a11y_elements: 0,
            cv_boxes: 0,
            damage_since_read: 0,
            ring_overflowed: false,
        }
    }

    /// Record an a11y read: its element count, the CV box count on the same frame
    /// (coverage), and the union window of its elements. Resets damage tracking.
    pub fn note_a11y_read(
        &mut self,
        elements: usize,
        cv_boxes: usize,
        window: Option<(i32, i32, i32, i32)>,
    ) {
        self.a11y_elements = elements;
        self.cv_boxes = cv_boxes;
        self.a11y_window = window;
        self.has_read = true;
        self.a11y_canvas_seq = super::canvas::canvas_seq().map(|(_, _, s)| s).unwrap_or(0);
        self.damage_since_read = 0;
        self.ring_overflowed = false;
        // fast-forward the cursor: damage before this read is already reflected in it
        if let Some((_, count, _)) = read_events(self.ev_cursor) {
            self.ev_cursor = count;
        }
    }

    /// Pull new damage events and update staleness. Call before consulting facts.
    pub fn ingest_damage(&mut self) {
        if let Some((evs, count, overflowed)) = read_events(self.ev_cursor) {
            self.ev_cursor = count;
            self.ring_overflowed |= overflowed;
            for e in &evs {
                match self.a11y_window {
                    Some(win) => {
                        if intersects(e, win) {
                            self.damage_since_read += 1;
                        }
                    }
                    None => self.damage_since_read += 1,
                }
            }
        }
    }

    /// VALID-UNTIL-DAMAGED: the last a11y read is stale iff damage touched its
    /// window since (or the ring wrapped past us — fail closed), or no read exists.
    pub fn a11y_stale(&self) -> bool {
        !self.has_read || self.damage_since_read > 0 || self.ring_overflowed
    }

    /// a11y coverage vs CV on the same frame: <1.0 = CV sees boxes a11y doesn't
    /// (the eyes+hands dispatch signal); >1.0 = a11y-rich surface.
    pub fn coverage(&self) -> f32 {
        self.a11y_elements as f32 / self.cv_boxes.max(1) as f32
    }

    /// One-line measured fact for chronos (the audit currency of the dispatcher).
    pub fn fact(&self) -> String {
        format!(
            "world: a11y_stale={} damage_since_read={} coverage={:.2} (a11y={} cv={}){}",
            self.a11y_stale(),
            self.damage_since_read,
            self.coverage(),
            self.a11y_elements,
            self.cv_boxes,
            if self.ring_overflowed { " RING-OVERFLOW" } else { "" }
        )
    }
}

impl Default for WorldModel {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_ring(path: &str, rects: &[(f64, u16, u16, u16, u16)]) {
        let mut f = std::fs::File::create(path).unwrap();
        f.write_all(EV_MAGIC).unwrap();
        f.write_all(&(rects.len() as u64).to_le_bytes()).unwrap();
        let mut slots = vec![0u8; EV_MAX as usize * EV_REC];
        for (i, (t, x, y, w, h)) in rects.iter().enumerate() {
            let base = (i % EV_MAX as usize) * EV_REC;
            slots[base..base + 8].copy_from_slice(&t.to_le_bytes());
            slots[base + 8..base + 10].copy_from_slice(&x.to_le_bytes());
            slots[base + 10..base + 12].copy_from_slice(&y.to_le_bytes());
            slots[base + 12..base + 14].copy_from_slice(&w.to_le_bytes());
            slots[base + 14..base + 16].copy_from_slice(&h.to_le_bytes());
        }
        f.write_all(&slots).unwrap();
    }

    fn ring_path(name: &str) -> String {
        std::env::temp_dir().join(name).to_str().unwrap().to_string()
    }

    #[test]
    fn ring_and_valid_until_damaged() {
        // ONE test: LAGADO_EVENTS is process-global; parallel tests would race it.
        let p = ring_path("lg_ev_t1");
        write_ring(&p, &[(1.0, 10, 20, 30, 40), (2.0, 50, 60, 70, 80)]);
        std::env::set_var("LAGADO_EVENTS", &p);
        let (evs, cur, ovf) = read_events(0).unwrap();
        assert_eq!((evs.len(), cur, ovf), (2, 2, false));
        assert_eq!((evs[1].x, evs[1].h), (50, 80));
        let (evs2, _, _) = read_events(cur).unwrap();
        assert!(evs2.is_empty(), "cursor consumed");

        let p = ring_path("lg_ev_t2");
        write_ring(&p, &[]);
        std::env::set_var("LAGADO_EVENTS", &p);
        let mut w = WorldModel::new();
        assert!(w.a11y_stale(), "no read yet = stale");
        w.note_a11y_read(12, 8, Some((100, 100, 400, 300)));
        w.ingest_damage();
        assert!(!w.a11y_stale(), "fresh read, no damage");
        assert!((w.coverage() - 1.5).abs() < 1e-6);
        // damage OUTSIDE the window: still fresh
        write_ring(&p, &[(1.0, 900, 900, 50, 50)]);
        w.ingest_damage();
        assert!(!w.a11y_stale(), "damage outside the a11y window must not invalidate");
        // damage INTERSECTING the window: stale
        write_ring(&p, &[(1.0, 900, 900, 50, 50), (2.0, 350, 250, 60, 60)]);
        w.ingest_damage();
        assert!(w.a11y_stale(), "damage in the window invalidates the read");
        assert!(w.fact().contains("a11y_stale=true"));
        std::env::remove_var("LAGADO_EVENTS");
    }
}
