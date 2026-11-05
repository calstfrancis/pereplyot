//! The FSRS-4.5 memory model: how likely you are to remember a card, and when to ask again. Only the
//! formulas and the published default weights; there is no optimiser and nothing learns from you.

pub const DEFAULT_WEIGHTS: [f64; 17] = [
    0.4872, 1.4003, 3.7145, 13.8206, 5.1618, 1.2298, 0.8975, 0.031, 1.6474, 0.1367, 1.0461, 2.1072,
    0.0793, 0.3246, 1.587, 0.2272, 2.8755,
];

const DECAY: f64 = -0.5;
/// Chosen so that retrievability is exactly 90% when the time since review equals the stability.
const FACTOR: f64 = 19.0 / 81.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rating {
    Again = 1,
    Hard = 2,
    Good = 3,
    Easy = 4,
}

impl Rating {
    pub const ALL: [Rating; 4] = [Rating::Again, Rating::Hard, Rating::Good, Rating::Easy];

    pub fn from_digit(d: u32) -> Option<Rating> {
        Rating::ALL.get((d as usize).checked_sub(1)?).copied()
    }

    pub fn label(self) -> &'static str {
        match self {
            Rating::Again => "Again",
            Rating::Hard => "Hard",
            Rating::Good => "Good",
            Rating::Easy => "Easy",
        }
    }
}

/// What the model knows about one card.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Memory {
    /// Days for the chance of recall to fall to 90%.
    pub stability: f64,
    /// 1 (easy) to 10 (hard).
    pub difficulty: f64,
}

pub struct Scheduler {
    pub w: [f64; 17],
    /// The chance of recall you want at the moment of review.
    pub retention: f64,
}

impl Default for Scheduler {
    fn default() -> Scheduler {
        Scheduler {
            w: DEFAULT_WEIGHTS,
            retention: 0.9,
        }
    }
}

/// The chance of recalling a card `days` after review, given its stability.
pub fn retrievability(days: f64, stability: f64) -> f64 {
    (1.0 + FACTOR * days.max(0.0) / stability.max(0.01)).powf(DECAY)
}

fn clamp_d(d: f64) -> f64 {
    d.clamp(1.0, 10.0)
}

impl Scheduler {
    pub fn first(&self, rating: Rating) -> Memory {
        let g = rating as usize;
        Memory {
            stability: self.w[g - 1].max(0.1),
            difficulty: clamp_d(self.w[4] - (g as f64 - 3.0) * self.w[5]),
        }
    }

    /// The memory after a review `days` after the last one.
    pub fn next(&self, m: Memory, days: f64, rating: Rating) -> Memory {
        let w = &self.w;
        let g = rating as usize as f64;
        let r = retrievability(days, m.stability);
        let d0_good = w[4];
        let difficulty = clamp_d(w[7] * d0_good + (1.0 - w[7]) * (m.difficulty - w[6] * (g - 3.0)));
        let stability = if rating == Rating::Again {
            w[11]
                * m.difficulty.powf(-w[12])
                * ((m.stability + 1.0).powf(w[13]) - 1.0)
                * ((1.0 - r) * w[14]).exp()
        } else {
            let hard = if rating == Rating::Hard { w[15] } else { 1.0 };
            let easy = if rating == Rating::Easy { w[16] } else { 1.0 };
            m.stability
                * (1.0
                    + w[8].exp()
                        * (11.0 - m.difficulty)
                        * m.stability.powf(-w[9])
                        * (((1.0 - r) * w[10]).exp() - 1.0)
                        * hard
                        * easy)
        };
        Memory {
            stability: stability.max(0.1),
            difficulty,
        }
    }

    /// Whole days until the card should be asked again, at least one.
    pub fn interval(&self, m: Memory) -> u32 {
        let days = m.stability / FACTOR * (self.retention.powf(1.0 / DECAY) - 1.0);
        days.round().clamp(1.0, 36_500.0) as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retrievability_is_ninety_percent_when_a_stability_has_passed() {
        for s in [0.5, 1.0, 2.4, 30.0, 400.0] {
            assert!((retrievability(s, s) - 0.9).abs() < 1e-9, "stability {s}");
        }
        assert!((retrievability(0.0, 5.0) - 1.0).abs() < 1e-12);
        assert!(retrievability(10.0, 5.0) < retrievability(5.0, 5.0));
    }

    #[test]
    fn at_ninety_percent_the_interval_is_the_stability() {
        let sched = Scheduler::default();
        for s in [1.0, 3.7, 20.0, 150.0] {
            let m = Memory {
                stability: s,
                difficulty: 5.0,
            };
            assert_eq!(sched.interval(m), s.round() as u32, "stability {s}");
        }
        let hungry = Scheduler {
            retention: 0.97,
            ..Scheduler::default()
        };
        assert!(
            hungry.interval(Memory {
                stability: 20.0,
                difficulty: 5.0
            }) < 20
        );
        let relaxed = Scheduler {
            retention: 0.8,
            ..Scheduler::default()
        };
        assert!(
            relaxed.interval(Memory {
                stability: 20.0,
                difficulty: 5.0
            }) > 20
        );
    }

    #[test]
    fn a_first_review_starts_from_the_published_stabilities() {
        let sched = Scheduler::default();
        for (rating, i) in [
            (Rating::Again, 0),
            (Rating::Hard, 1),
            (Rating::Good, 2),
            (Rating::Easy, 3),
        ] {
            assert_eq!(sched.first(rating).stability, DEFAULT_WEIGHTS[i]);
        }
        let d: Vec<f64> = Rating::ALL
            .iter()
            .map(|r| sched.first(*r).difficulty)
            .collect();
        assert!(
            d.windows(2).all(|p| p[0] > p[1]),
            "an easier answer makes an easier card: {d:?}"
        );
        assert!(d.iter().all(|x| (1.0..=10.0).contains(x)));
    }

    #[test]
    fn better_answers_earn_longer_waits_and_forgetting_resets() {
        let sched = Scheduler::default();
        let m = Memory {
            stability: 10.0,
            difficulty: 5.0,
        };
        let after = |r| sched.next(m, 10.0, r);
        let (a, h, g, e) = (
            after(Rating::Again),
            after(Rating::Hard),
            after(Rating::Good),
            after(Rating::Easy),
        );
        assert!(a.stability < m.stability, "forgetting shortens: {a:?}");
        assert!(h.stability > m.stability, "even Hard grows it");
        assert!(
            a.stability < h.stability && h.stability < g.stability && g.stability < e.stability
        );
        assert!(a.difficulty > g.difficulty && g.difficulty > e.difficulty);
    }

    #[test]
    fn recalling_late_is_worth_more_than_recalling_early() {
        let sched = Scheduler::default();
        let m = Memory {
            stability: 10.0,
            difficulty: 5.0,
        };
        let early = sched.next(m, 2.0, Rating::Good).stability;
        let late = sched.next(m, 30.0, Rating::Good).stability;
        assert!(late > early, "{early} vs {late}");
    }

    #[test]
    fn difficulty_stays_in_range_over_a_long_run() {
        let sched = Scheduler::default();
        let mut m = sched.first(Rating::Good);
        for i in 0..200 {
            let r = if i % 3 == 0 {
                Rating::Again
            } else {
                Rating::Easy
            };
            m = sched.next(m, sched.interval(m) as f64, r);
            assert!((1.0..=10.0).contains(&m.difficulty), "{m:?}");
            assert!(m.stability.is_finite() && m.stability >= 0.1);
        }
    }

    #[test]
    fn a_two_hundred_card_deck_schedules_without_surprises() {
        let sched = Scheduler::default();
        let mut waits = Vec::new();
        for i in 0..200 {
            let rating = Rating::ALL[(i * 7 + i / 3) % 4];
            let m = sched.first(rating);
            let m = sched.next(m, sched.interval(m) as f64, Rating::Good);
            waits.push(sched.interval(m));
        }
        assert!(waits.iter().all(|w| (1..=36_500).contains(w)));
        assert!(waits.iter().any(|w| *w > 1) && waits.iter().any(|w| *w <= 3));
    }
}
