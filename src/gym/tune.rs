//! Evolutionary tuning of [`AiProfile`] in the headless gym.
//!
//! A (1+λ) evolution strategy with Gaussian mutation in the normalised
//! parameter box (`AiProfile::BOUNDS`) and a 1/5th-rule step-size adaptation.
//! Fitness is measured over full simulated matches with common random numbers
//! (every candidate in a generation sees the same seeds) against three
//! opponents: the [`ROOKIE`] AI, the [`PRO`] AI, and the scripted human proxy
//! (`scripted_driver` + PRO teammates). The score rewards point differential
//! *and* defense (opponent FG%, steals, blocks) and penalises a candidate that
//! stops scoring itself.
//!
//! Reproduce the shipped constants with:
//!
//! ```text
//! CARGO_TARGET_DIR=/workspace/target FINN_TUNE_GENS=40 \
//!   cargo test --release --offline tune_pro_profile -- --ignored --nocapture
//! ```
//!
//! and print the before/after table with `report_profiles -- --ignored --nocapture`.

use super::{scripted_driver, Action, Gym, GymRng, TeamStats};
use crate::ai::{AiProfile, LEGEND, PRO, ROOKIE};
use crate::roster::Side;

/// A tie loops 30 s overtimes forever; cap a match well past regulation.
pub const MATCH_CAP: u64 = 40_000;

/// Aggregate of one side over a series of matches.
#[derive(Clone, Copy, Debug, Default)]
pub struct SeriesStats {
    pub matches: u32,
    pub wins: u32,
    pub pts: u32,
    pub opp_pts: u32,
    pub fga: u32,
    pub fgm: u32,
    pub opp_fga: u32,
    pub opp_fgm: u32,
    pub stl: u32,
    pub blk: u32,
    pub opp_stl: u32,
    pub opp_blk: u32,
}

impl SeriesStats {
    pub fn add(&mut self, mine: &TeamStats, theirs: &TeamStats) {
        self.matches += 1;
        if mine.pts > theirs.pts {
            self.wins += 1;
        }
        self.pts += mine.pts;
        self.opp_pts += theirs.pts;
        self.fga += mine.fga;
        self.fgm += mine.fgm;
        self.opp_fga += theirs.fga;
        self.opp_fgm += theirs.fga.min(theirs.fgm);
        self.stl += mine.stl;
        self.blk += mine.blk;
        self.opp_stl += theirs.stl;
        self.opp_blk += theirs.blk;
    }

    pub fn merge(&mut self, o: &SeriesStats) {
        self.matches += o.matches;
        self.wins += o.wins;
        self.pts += o.pts;
        self.opp_pts += o.opp_pts;
        self.fga += o.fga;
        self.fgm += o.fgm;
        self.opp_fga += o.opp_fga;
        self.opp_fgm += o.opp_fgm;
        self.stl += o.stl;
        self.blk += o.blk;
        self.opp_stl += o.opp_stl;
        self.opp_blk += o.opp_blk;
    }

    fn per_match(&self, v: u32) -> f32 {
        if self.matches == 0 {
            0.0
        } else {
            v as f32 / self.matches as f32
        }
    }

    pub fn avg_pts(&self) -> f32 {
        self.per_match(self.pts)
    }
    pub fn avg_opp_pts(&self) -> f32 {
        self.per_match(self.opp_pts)
    }
    /// Average point differential per match.
    pub fn diff(&self) -> f32 {
        self.avg_pts() - self.avg_opp_pts()
    }
    pub fn fg_pct(&self) -> f32 {
        if self.fga == 0 {
            0.0
        } else {
            self.fgm as f32 / self.fga as f32
        }
    }
    pub fn opp_fg_pct(&self) -> f32 {
        if self.opp_fga == 0 {
            0.0
        } else {
            self.opp_fgm as f32 / self.opp_fga as f32
        }
    }
    pub fn stl_per_match(&self) -> f32 {
        self.per_match(self.stl)
    }
    pub fn blk_per_match(&self) -> f32 {
        self.per_match(self.blk)
    }
    pub fn win_rate(&self) -> f32 {
        self.per_match(self.wins)
    }

    pub fn line(&self) -> String {
        format!(
            "n={:<3} W {:>4.0}%  pts {:>5.1}-{:<5.1}  diff {:>+6.1}  FG {:>4.1}%  oppFG {:>4.1}%  stl {:>4.2}  blk {:>4.2}",
            self.matches,
            self.win_rate() * 100.0,
            self.avg_pts(),
            self.avg_opp_pts(),
            self.diff(),
            self.fg_pct() * 100.0,
            self.opp_fg_pct() * 100.0,
            self.stl_per_match(),
            self.blk_per_match()
        )
    }
}

/// `a` vs `b`, every seed played twice (`a` at home, then away) so hoop-side
/// asymmetries cancel. All six players are AI.
pub fn series(gym: &mut Gym, a: AiProfile, b: AiProfile, seeds: &[u64]) -> SeriesStats {
    let mut s = SeriesStats::default();
    for &seed in seeds {
        for a_home in [true, false] {
            let (home, away) = if a_home { (a, b) } else { (b, a) };
            gym.set_profiles(home, away);
            gym.release_control();
            gym.reset(seed);
            let r = gym.play_out(|_, _| Action::noop(), MATCH_CAP);
            let (mine, theirs) = if a_home {
                (r.home, r.away)
            } else {
                (r.away, r.home)
            };
            s.add(&mine, &theirs);
        }
    }
    s
}

/// `cand` as the opposition (Away) against the scripted human proxy driving
/// home slot 0 with PRO teammates — what the player actually meets.
pub fn vs_human_proxy(gym: &mut Gym, cand: AiProfile, seeds: &[u64]) -> SeriesStats {
    let mut s = SeriesStats::default();
    for &seed in seeds {
        gym.set_profiles(PRO, cand);
        gym.select(Side::Home, 0);
        gym.reset(seed);
        let mut rng = GymRng(seed ^ 0xA5A5_5A5A);
        let r = gym.play_out(|obs, step| scripted_driver(obs, step, &mut rng), MATCH_CAP);
        s.add(&r.away, &r.home);
    }
    s
}

/// Everything one fitness evaluation measured.
#[derive(Clone, Copy, Debug, Default)]
pub struct Eval {
    pub vs_rookie: SeriesStats,
    pub vs_pro: SeriesStats,
    pub vs_human: SeriesStats,
    pub fitness: f32,
}

/// Points-differential plus defensive terms, minus a penalty for not scoring.
pub fn score_series(s: &SeriesStats) -> f32 {
    s.diff()
        + 20.0 * (0.45 - s.opp_fg_pct())
        + 0.4 * s.stl_per_match().min(5.0)
        + 0.6 * s.blk_per_match().min(4.0)
        - 1.5 * (14.0 - s.avg_pts()).max(0.0)
        // A turnover fest is not fun to play against.
        - 1.0 * (s.stl_per_match() - 6.0).max(0.0)
        - 1.0 * (s.blk_per_match() - 5.0).max(0.0)
}

pub fn evaluate(gym: &mut Gym, cand: AiProfile, seeds: &[u64]) -> Eval {
    let vs_rookie = series(gym, cand, ROOKIE, seeds);
    let vs_pro = series(gym, cand, PRO, seeds);
    let vs_human = vs_human_proxy(gym, cand, seeds);
    let fitness =
        (score_series(&vs_rookie) + score_series(&vs_pro) + 1.5 * score_series(&vs_human)) / 3.5;
    Eval {
        vs_rookie,
        vs_pro,
        vs_human,
        fitness,
    }
}

fn gaussian(rng: &mut GymRng) -> f32 {
    // Box–Muller; `f32()` is in [0, 1) so shift away from log(0).
    let u1 = (1.0 - rng.f32()).max(1e-7);
    let u2 = rng.f32();
    (-2.0 * u1.ln()).sqrt() * (std::f32::consts::TAU * u2).cos()
}

/// Gaussian mutation of `p` in the normalised box; each coordinate moves with
/// probability 0.7.
pub fn mutate(p: &AiProfile, sigma: f32, rng: &mut GymRng) -> AiProfile {
    let mut a = p.to_array();
    for (v, (lo, hi)) in a.iter_mut().zip(AiProfile::BOUNDS.iter()) {
        if rng.f32() < 0.7 {
            let u = (*v - lo) / (hi - lo);
            let u = (u + gaussian(rng) * sigma).clamp(0.0, 1.0);
            *v = lo + u * (hi - lo);
        }
    }
    AiProfile::from_array(a)
}

#[derive(Clone, Copy, Debug)]
pub struct EsConfig {
    pub gens: usize,
    pub lambda: usize,
    pub seeds_per_gen: usize,
    pub sigma0: f32,
    pub threads: usize,
    pub rng_seed: u64,
}

impl Default for EsConfig {
    fn default() -> Self {
        Self {
            gens: 40,
            lambda: 12,
            seeds_per_gen: 2,
            sigma0: 0.12,
            threads: std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1)
                .max(1),
            rng_seed: 7,
        }
    }
}

/// Evaluates every candidate on `seeds`, one `Gym` per worker thread.
pub fn evaluate_all(cands: &[AiProfile], seeds: &[u64], threads: usize) -> Vec<Eval> {
    let threads = threads.clamp(1, cands.len().max(1));
    let chunk = cands.len().div_ceil(threads);
    let mut out: Vec<Vec<Eval>> = Vec::new();
    std::thread::scope(|s| {
        let handles: Vec<_> = cands
            .chunks(chunk)
            .map(|c| {
                s.spawn(move || {
                    let mut gym = Gym::ai_vs_ai(PRO, PRO, 1);
                    c.iter()
                        .map(|p| evaluate(&mut gym, *p, seeds))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        for h in handles {
            out.push(h.join().expect("tuner worker panicked"));
        }
    });
    out.into_iter().flatten().collect()
}

/// (1+λ)-ES from `start`. Returns the best profile, its last evaluation and the
/// per-generation log lines.
pub fn run_es(start: AiProfile, cfg: EsConfig, mut log: impl FnMut(&str)) -> (AiProfile, Eval) {
    let mut rng = GymRng(cfg.rng_seed);
    let mut elite = start.clamped();
    let mut sigma = cfg.sigma0;
    let mut elite_eval = Eval::default();
    for g in 0..cfg.gens {
        let seeds: Vec<u64> = (0..cfg.seeds_per_gen)
            .map(|i| 1_000 + g as u64 * 100 + i as u64)
            .collect();
        let mut cands = vec![elite];
        for _ in 0..cfg.lambda {
            cands.push(mutate(&elite, sigma, &mut rng));
        }
        let evals = evaluate_all(&cands, &seeds, cfg.threads);
        let (bi, best) = evals
            .iter()
            .enumerate()
            .max_by(|a, b| {
                a.1.fitness
                    .partial_cmp(&b.1.fitness)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(i, e)| (i, *e))
            .unwrap();
        let improved = bi != 0;
        if improved {
            elite = cands[bi];
            sigma = (sigma * 1.2).min(0.5);
        } else {
            sigma = (sigma * 0.85).max(0.02);
        }
        elite_eval = if improved { best } else { evals[0] };
        log(&format!(
            "gen {g:>3}  fit {:>+7.2} (elite {:>+7.2}) sigma {sigma:.3}  {}  | vsRookie {:+.1} vsPro {:+.1} vsHuman {:+.1} oppFG {:.0}/{:.0}/{:.0}%",
            best.fitness,
            evals[0].fitness,
            if improved { "IMPROVED" } else { "kept    " },
            elite_eval.vs_rookie.diff(),
            elite_eval.vs_pro.diff(),
            elite_eval.vs_human.diff(),
            elite_eval.vs_rookie.opp_fg_pct() * 100.0,
            elite_eval.vs_pro.opp_fg_pct() * 100.0,
            elite_eval.vs_human.opp_fg_pct() * 100.0,
        ));
    }
    (elite, elite_eval)
}

/// Rust source for a profile constant, ready to paste into `ai.rs`.
pub fn rust_literal(name: &str, p: &AiProfile) -> String {
    let names = [
        "reaction",
        "def_lag",
        "speed",
        "skill",
        "meter_err",
        "windup",
        "pressure_dist",
        "sag",
        "closeout_dist",
        "closeout_range",
        "help_threshold",
        "help_beaten",
        "deny_t",
        "deny_sag",
        "steal_rate",
        "steal_cooldown",
        "block_aggr",
        "lane_jump",
        "shot_ev_min",
        "late_clock",
        "drive_gap",
        "kick_dist",
        "cut_gap",
        "screen_rate",
        "pass_open_w",
        "juke_rate",
    ];
    let mut s = format!("pub const {name}: AiProfile = AiProfile {{\n");
    for (n, v) in names.iter().zip(p.to_array().iter()) {
        s.push_str(&format!("    {n}: {v:.2},\n"));
    }
    s.push_str("};");
    s
}

/// LEGEND = a tuned PRO with instant reactions, elite touch and more aggression.
pub fn legend_from(pro: &AiProfile) -> AiProfile {
    AiProfile {
        reaction: 0.12,
        def_lag: 0.0,
        speed: 1.0,
        skill: 1.12,
        meter_err: 0.02,
        windup: (pro.windup * 0.6).max(0.08),
        pressure_dist: (pro.pressure_dist * 0.9).max(0.6),
        closeout_range: (pro.closeout_range + 1.5).min(6.0),
        help_beaten: (pro.help_beaten * 0.7).max(0.2),
        deny_t: (pro.deny_t + 0.1).min(0.8),
        steal_rate: (pro.steal_rate * 1.4).min(2.0),
        steal_cooldown: (pro.steal_cooldown * 0.7).max(0.3),
        block_aggr: (pro.block_aggr * 1.3).min(1.0),
        lane_jump: (pro.lane_jump + 0.5).min(3.0),
        screen_rate: (pro.screen_rate * 1.5).min(2.0),
        juke_rate: (pro.juke_rate + 0.2).min(1.0),
        ..*pro
    }
    .clamped()
}

/// Before/after table for the three shipped presets.
pub fn report(seeds: &[u64]) -> String {
    let mut gym = Gym::ai_vs_ai(PRO, PRO, 1);
    let mut out = String::new();
    out.push_str(&format!(
        "{:<28} {}\n",
        "matchup (row = subject)", "subject stats"
    ));
    for (name, p) in [("ROOKIE", ROOKIE), ("PRO", PRO), ("LEGEND", LEGEND)] {
        let s = series(&mut gym, p, ROOKIE, seeds);
        out.push_str(&format!("{:<28} {}\n", format!("{name} vs ROOKIE"), s.line()));
    }
    for (name, p) in [("PRO", PRO), ("LEGEND", LEGEND)] {
        let s = series(&mut gym, p, PRO, seeds);
        out.push_str(&format!("{:<28} {}\n", format!("{name} vs PRO"), s.line()));
    }
    for (name, p) in [("ROOKIE", ROOKIE), ("PRO", PRO), ("LEGEND", LEGEND)] {
        let s = vs_human_proxy(&mut gym, p, seeds);
        out.push_str(&format!(
            "{:<28} {}\n",
            format!("{name} vs human proxy"),
            s.line()
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_usize(key: &str, default: usize) -> usize {
        std::env::var(key)
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(default)
    }

    /// The search that produced `ai::PRO` / `ai::LEGEND`. Heavy: run in release,
    /// ignored by default.
    ///
    /// `FINN_TUNE_GENS`, `FINN_TUNE_LAMBDA`, `FINN_TUNE_SEEDS`, `FINN_TUNE_THREADS`
    /// override the defaults.
    #[test]
    #[ignore]
    fn tune_pro_profile() {
        let cfg = EsConfig {
            gens: env_usize("FINN_TUNE_GENS", 40),
            lambda: env_usize("FINN_TUNE_LAMBDA", 12),
            seeds_per_gen: env_usize("FINN_TUNE_SEEDS", 2),
            threads: env_usize("FINN_TUNE_THREADS", EsConfig::default().threads),
            ..EsConfig::default()
        };
        eprintln!("tune: {cfg:?}");
        let (best, eval) = run_es(PRO, cfg, |l| eprintln!("{l}"));
        eprintln!("tune: best fitness {:+.2}", eval.fitness);
        eprintln!("{}", rust_literal("PRO", &best));
        eprintln!("{}", rust_literal("LEGEND", &legend_from(&best)));
    }

    /// Prints the before/after table (`FINN_REPORT_SEEDS` seeds, default 5).
    #[test]
    #[ignore]
    fn report_profiles() {
        let n = env_usize("FINN_REPORT_SEEDS", 5) as u64;
        let seeds: Vec<u64> = (1..=n).collect();
        eprintln!("\n{}", report(&seeds));
    }

    #[test]
    fn mutation_stays_inside_the_box_and_moves() {
        let mut rng = GymRng(3);
        let mut moved = false;
        for _ in 0..20 {
            let m = mutate(&PRO, 0.2, &mut rng);
            assert_eq!(m.clamped(), m);
            moved |= m != PRO;
        }
        assert!(moved);
        assert_eq!(legend_from(&PRO).clamped(), legend_from(&PRO));
    }

    #[test]
    fn series_stats_lines_up() {
        let mut s = SeriesStats::default();
        s.add(
            &TeamStats {
                pts: 20,
                fga: 10,
                fgm: 5,
                stl: 2,
                blk: 1,
                ..Default::default()
            },
            &TeamStats {
                pts: 10,
                fga: 20,
                fgm: 5,
                ..Default::default()
            },
        );
        assert_eq!(s.wins, 1);
        assert!((s.diff() - 10.0).abs() < 1e-5);
        assert!((s.fg_pct() - 0.5).abs() < 1e-5);
        assert!((s.opp_fg_pct() - 0.25).abs() < 1e-5);
        assert!(score_series(&s) > 10.0);
    }
}
