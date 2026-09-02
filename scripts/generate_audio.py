#!/usr/bin/env python3
"""Generate every FINNBALL audio asset from scratch (numpy DSP, no samples).

All sounds are synthesized — there are no licensed recordings anywhere in the
pack. The bake is deterministic (seeded RNG), so re-running it reproduces the
exact same bytes.

Formats (kept small for the WASM download):
    * SFX / foley / broadcast cues  : 22050 Hz, mono,   16-bit
    * crowd beds, reactions, stingers: 22050 Hz, stereo, 16-bit
    * music (menu + in-game stems)   : 32000 Hz, stereo, 16-bit

Run:  python3 scripts/generate_audio.py
Writes into assets/audio/** and removes stale .wav files that are no longer
part of the pack. Requires numpy.
"""

from __future__ import annotations

import math
import os
import struct
import sys
import wave

import numpy as np

SR_SFX = 22050
SR_MUS = 32000
TWO_PI = 2.0 * math.pi
PEAK = 0.82
BUDGET_MB = 12.0

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
AUDIO = os.path.join(ROOT, "assets", "audio")


# ---------------------------------------------------------------------------
# Basics
# ---------------------------------------------------------------------------


def rng_for(seed: int) -> np.random.Generator:
    return np.random.default_rng(seed)


def n_of(dur: float, sr: int) -> int:
    return max(1, int(round(dur * sr)))


def t_axis(dur: float, sr: int) -> np.ndarray:
    return np.arange(n_of(dur, sr), dtype=np.float64) / sr


def zeros(dur: float, sr: int) -> np.ndarray:
    return np.zeros(n_of(dur, sr), dtype=np.float64)


def db(x: float) -> float:
    return 10.0 ** (x / 20.0)


def add_at(dst: np.ndarray, src: np.ndarray, at: int, gain: float = 1.0) -> None:
    """Mix `src` into `dst` starting at sample `at`, clipping to bounds."""
    if at >= dst.shape[-1]:
        return
    if at < 0:
        src = src[..., -at:]
        at = 0
    n = min(src.shape[-1], dst.shape[-1] - at)
    if n <= 0:
        return
    dst[..., at : at + n] += src[..., :n] * gain


def add_sec(dst: np.ndarray, src: np.ndarray, at_s: float, sr: int, gain: float = 1.0) -> None:
    add_at(dst, src, int(round(at_s * sr)), gain)


def fade(x: np.ndarray, sr: int, fin: float = 0.0, fout: float = 0.0) -> np.ndarray:
    y = x.copy()
    n = y.shape[-1]
    a = min(n, n_of(fin, sr)) if fin > 0 else 0
    b = min(n, n_of(fout, sr)) if fout > 0 else 0
    if a > 0:
        y[..., :a] *= np.linspace(0.0, 1.0, a)
    if b > 0:
        y[..., n - b :] *= np.linspace(1.0, 0.0, b)
    return y


def make_loop(x: np.ndarray, sr: int, xfade: float = 0.25) -> np.ndarray:
    """Seamless loop: equal-power crossfade of the tail into the head, then trim."""
    n = x.shape[-1]
    k = min(n // 3, n_of(xfade, sr))
    if k <= 0:
        return x
    head = x[..., :k]
    tail = x[..., n - k :]
    w = np.linspace(0.0, 1.0, k)
    a = np.cos(w * math.pi / 2.0)
    b = np.sin(w * math.pi / 2.0)
    blended = tail * a + head * b
    out = x[..., : n - k].copy()
    out[..., :k] = blended
    return out


def resample(x: np.ndarray, factor: float) -> np.ndarray:
    """Playback-rate change (factor > 1 = higher pitch / shorter)."""
    n = x.shape[-1]
    m = max(1, int(n / factor))
    src_idx = np.arange(m) * factor
    base = np.arange(n)
    if x.ndim == 1:
        return np.interp(src_idx, base, x)
    return np.stack([np.interp(src_idx, base, ch) for ch in x])


def soft_clip(x: np.ndarray, drive: float = 1.3) -> np.ndarray:
    return np.tanh(x * drive) / math.tanh(drive)


def normalize(x: np.ndarray, peak: float = PEAK) -> np.ndarray:
    p = float(np.max(np.abs(x))) if x.size else 0.0
    if p < 1e-9:
        return x
    return x * (peak / p)


# ---------------------------------------------------------------------------
# Envelopes
# ---------------------------------------------------------------------------


def exp_env(t: np.ndarray, rate: float, t0: float = 0.0) -> np.ndarray:
    e = np.exp(-(t - t0) * rate)
    e[t < t0] = 0.0
    return e


def adsr(t: np.ndarray, dur: float, a: float, d: float, s: float, r: float) -> np.ndarray:
    env = np.zeros_like(t)
    a = max(a, 1e-4)
    d = max(d, 1e-4)
    r = max(r, 1e-4)
    rel = max(a + d, dur - r)
    m = t < a
    env[m] = t[m] / a
    m = (t >= a) & (t < a + d)
    env[m] = 1.0 - (1.0 - s) * (t[m] - a) / d
    m = (t >= a + d) & (t < rel)
    env[m] = s
    m = (t >= rel) & (t < dur)
    env[m] = s * np.clip(1.0 - (t[m] - rel) / r, 0.0, 1.0)
    return env


def ramp(t: np.ndarray, t0: float, t1: float) -> np.ndarray:
    return np.clip((t - t0) / max(t1 - t0, 1e-6), 0.0, 1.0)


def hump(t: np.ndarray, t0: float, t1: float, power: float = 1.0) -> np.ndarray:
    """Raised-cosine bump between t0 and t1."""
    w = ramp(t, t0, t1)
    return np.sin(w * math.pi) ** power


# ---------------------------------------------------------------------------
# Filters — zero-phase FFT magnitude shaping (no scipy needed)
# ---------------------------------------------------------------------------


def _fft_filter(x: np.ndarray, mag_fn, sr: int) -> np.ndarray:
    if x.ndim == 2:
        return np.stack([_fft_filter(ch, mag_fn, sr) for ch in x])
    n = x.shape[0]
    X = np.fft.rfft(x)
    f = np.fft.rfftfreq(n, 1.0 / sr)
    return np.fft.irfft(X * mag_fn(f), n=n)


def lp(x: np.ndarray, fc: float, sr: int, order: float = 2.0) -> np.ndarray:
    return _fft_filter(x, lambda f: 1.0 / np.sqrt(1.0 + (f / fc) ** (2.0 * order)), sr)


def hp(x: np.ndarray, fc: float, sr: int, order: float = 2.0) -> np.ndarray:
    def mag(f):
        with np.errstate(divide="ignore"):
            r = np.where(f > 0, fc / np.maximum(f, 1e-9), 1e9)
        return 1.0 / np.sqrt(1.0 + r ** (2.0 * order))

    return _fft_filter(x, mag, sr)


def bp(x: np.ndarray, fc: float, q: float, sr: int) -> np.ndarray:
    def mag(f):
        with np.errstate(divide="ignore"):
            fs = np.maximum(f, 1e-9)
            d = q * (fs / fc - fc / fs)
        return 1.0 / np.sqrt(1.0 + d * d)

    return _fft_filter(x, mag, sr)


def peak_eq(x: np.ndarray, fc: float, q: float, gain_db: float, sr: int) -> np.ndarray:
    g = db(gain_db) - 1.0

    def mag(f):
        with np.errstate(divide="ignore"):
            fs = np.maximum(f, 1e-9)
            d = q * (fs / fc - fc / fs)
        return 1.0 + g / np.sqrt(1.0 + d * d)

    return _fft_filter(x, mag, sr)


def formants(x: np.ndarray, bands: list[tuple[float, float, float]], sr: int) -> np.ndarray:
    """Parallel resonant bands (centre Hz, Q, gain)."""
    out = np.zeros_like(x)
    for fc, q, g in bands:
        out += bp(x, fc, q, sr) * g
    return out


def white(n: int, rng: np.random.Generator) -> np.ndarray:
    return rng.uniform(-1.0, 1.0, n)


def pink(n: int, rng: np.random.Generator, sr: int) -> np.ndarray:
    w = white(n, rng)

    def mag(f):
        return np.where(f > 0, 1.0 / np.sqrt(np.maximum(f, 1.0)), 0.0)

    return normalize(_fft_filter(w, mag, sr), 1.0)


def brown(n: int, rng: np.random.Generator, sr: int) -> np.ndarray:
    w = white(n, rng)

    def mag(f):
        return np.where(f > 0, 1.0 / np.maximum(f, 1.0), 0.0)

    return normalize(_fft_filter(w, mag, sr), 1.0)


def sweep_noise(
    t: np.ndarray, rng: np.random.Generator, sr: int, f_start: float, f_end: float, q: float = 2.0
) -> np.ndarray:
    """Noise through a band-pass whose centre glides f_start → f_end (log), via band blending."""
    n = t.shape[0]
    w = white(n, rng)
    bands = 10
    centres = np.geomspace(min(f_start, f_end), max(f_start, f_end), bands)
    filt = [bp(w, fc, q, sr) for fc in centres]
    pos = np.log(f_start) + (np.log(f_end) - np.log(f_start)) * (t / t[-1])
    idx = (pos - np.log(centres[0])) / (np.log(centres[-1]) - np.log(centres[0])) * (bands - 1)
    idx = np.clip(idx, 0, bands - 1)
    lo = np.floor(idx).astype(int)
    hi = np.minimum(lo + 1, bands - 1)
    frac = idx - lo
    stack = np.stack(filt)
    out = stack[lo, np.arange(n)] * (1.0 - frac) + stack[hi, np.arange(n)] * frac
    return out


# ---------------------------------------------------------------------------
# Effects
# ---------------------------------------------------------------------------


def comb_reverb(
    x: np.ndarray,
    sr: int,
    wet: float = 0.25,
    decay: float = 0.6,
    delays_ms=(29.7, 37.1, 41.1, 43.7),
    damp_fc: float = 3200.0,
    predelay_ms: float = 8.0,
) -> np.ndarray:
    """Tiny arena tail: 4 parallel feedback combs, damped, mixed under the dry."""
    if x.ndim == 2:
        return np.stack(
            [
                comb_reverb(x[0], sr, wet, decay, delays_ms, damp_fc, predelay_ms),
                comb_reverb(x[1], sr, wet, decay, tuple(d * 1.013 for d in delays_ms), damp_fc, predelay_ms),
            ]
        )
    n = x.shape[0]
    tail = int(sr * 0.9)
    src = np.concatenate([np.zeros(int(sr * predelay_ms / 1000.0)), x, np.zeros(tail)])
    acc = np.zeros_like(src)
    for d_ms in delays_ms:
        d = max(1, int(sr * d_ms / 1000.0))
        g = decay ** (d_ms / 50.0)
        y = np.zeros_like(src)
        for start in range(0, src.shape[0], d):
            end = min(start + d, src.shape[0])
            seg = src[start:end]
            if start >= d:
                seg = seg + g * y[start - d : start - d + (end - start)]
            y[start:end] = seg
        acc += y
    acc = lp(acc, damp_fc, sr, 1.0) / len(delays_ms)
    out = np.zeros(n)
    out += x
    add_at(out, acc, 0, wet)
    return out


def echo(x: np.ndarray, sr: int, time_s: float, fb: float, taps: int = 4, damp: float = 2600.0) -> np.ndarray:
    out = np.zeros(x.shape[-1] + int(sr * time_s * taps))
    if x.ndim == 2:
        out = np.zeros((2, out.shape[0]))
    add_at(out, x, 0)
    cur = x.copy()
    for k in range(1, taps + 1):
        cur = lp(cur, damp, sr, 1.0) * fb
        add_at(out, cur, int(sr * time_s * k))
    return out


def stereo(mono: np.ndarray, sr: int, width: float = 0.15, delay_ms: float = 0.4) -> np.ndarray:
    """Cheap widener: haas delay + opposite-side bleed. Mono-compatible."""
    d = max(1, int(sr * delay_ms / 1000.0))
    delayed = np.concatenate([np.zeros(d), mono[:-d]]) if d < mono.shape[0] else mono
    w = min(max(width, 0.0), 0.45)
    left = mono * (1.0 - w) + delayed * w
    right = mono * (1.0 - w * 0.4) + delayed * (w * 0.6) * -1.0 + delayed * w * 0.4
    return np.stack([left, right])


def pan_pair(a: np.ndarray, b: np.ndarray, spread: float = 0.8) -> np.ndarray:
    """Two decorrelated mono sources into an L/R field."""
    left = a * (0.5 + spread * 0.5) + b * (0.5 - spread * 0.5)
    right = b * (0.5 + spread * 0.5) + a * (0.5 - spread * 0.5)
    return np.stack([left, right])


# ---------------------------------------------------------------------------
# Oscillators
# ---------------------------------------------------------------------------


def phase_of(freq: np.ndarray | float, t: np.ndarray, sr: int) -> np.ndarray:
    if np.isscalar(freq):
        return TWO_PI * float(freq) * t
    return TWO_PI * np.cumsum(np.asarray(freq)) / sr


def sine(freq, t: np.ndarray, sr: int, phase: float = 0.0) -> np.ndarray:
    return np.sin(phase_of(freq, t, sr) + phase)


def additive(freq, t: np.ndarray, sr: int, weights, phase: float = 0.0) -> np.ndarray:
    """sum_k w_k sin(k * phase); band-limited by dropping harmonics above 0.45 sr."""
    ph = phase_of(freq, t, sr)
    f0 = float(freq) if np.isscalar(freq) else float(np.max(freq))
    out = np.zeros_like(t)
    for k, w in enumerate(weights, start=1):
        if f0 * k >= sr * 0.45 or w == 0.0:
            continue
        out += w * np.sin(k * ph + phase * k)
    return out


def saw(freq, t: np.ndarray, sr: int, harmonics: int = 24) -> np.ndarray:
    return additive(freq, t, sr, [1.0 / k for k in range(1, harmonics + 1)]) * (2.0 / math.pi)


def square(freq, t: np.ndarray, sr: int, harmonics: int = 20) -> np.ndarray:
    return additive(freq, t, sr, [(1.0 / k) if k % 2 else 0.0 for k in range(1, harmonics + 1)]) * (
        4.0 / math.pi
    )


def triangle(freq, t: np.ndarray, sr: int, harmonics: int = 12) -> np.ndarray:
    w = []
    for k in range(1, harmonics + 1):
        if k % 2 == 0:
            w.append(0.0)
        else:
            sign = 1.0 if ((k - 1) // 2) % 2 == 0 else -1.0
            w.append(sign / (k * k))
    return additive(freq, t, sr, w) * (8.0 / (math.pi * math.pi))


def supersaw(freq: float, t: np.ndarray, sr: int, voices: int = 5, detune: float = 0.012) -> np.ndarray:
    out = np.zeros_like(t)
    for i in range(voices):
        off = (i - (voices - 1) / 2.0) / max(1, (voices - 1) / 2.0)
        out += saw(freq * (1.0 + off * detune), t, sr, 18) / voices
    return out


def fm(fc: float, fmod: float, index: float, t: np.ndarray, sr: int, idx_env=None) -> np.ndarray:
    ie = index if idx_env is None else index * idx_env
    return np.sin(TWO_PI * fc * t + ie * np.sin(TWO_PI * fmod * t))


def modal(t: np.ndarray, sr: int, partials: list[tuple[float, float, float]], strike: float = 0.0) -> np.ndarray:
    """Struck resonator: sum of decaying sines (freq, amp, decay/s)."""
    out = np.zeros_like(t)
    for f, a, d in partials:
        if f >= sr * 0.45:
            continue
        out += a * np.sin(TWO_PI * f * (t - strike)) * exp_env(t, d, strike)
    return out


def voice_chorus(
    t: np.ndarray,
    sr: int,
    rng: np.random.Generator,
    n_voices: int,
    f0_lo: float,
    f0_hi: float,
    contour: np.ndarray | None = None,
    spread_s: float = 0.08,
    vib_hz: float = 5.5,
    vib_depth: float = 0.008,
    breath: float = 0.12,
) -> np.ndarray:
    """A pack of sawtooth 'throats' with random pitch, onset scatter and vibrato.
    Returned raw (pre-formant). `contour` multiplies the pitch over time."""
    out = np.zeros_like(t)
    n = t.shape[0]
    for _ in range(n_voices):
        f0 = math.exp(rng.uniform(math.log(f0_lo), math.log(f0_hi)))
        onset = rng.uniform(0.0, spread_s)
        vib = 1.0 + vib_depth * np.sin(TWO_PI * (vib_hz * rng.uniform(0.8, 1.25)) * t + rng.uniform(0, TWO_PI))
        freq = f0 * vib
        if contour is not None:
            freq = freq * contour
        harm = int(min(28, (sr * 0.42) // max(f0 * 2.0, 40.0)))
        v = saw(freq, t, sr, max(6, harm))
        v = v * ramp(t, onset, onset + 0.05)
        out += v * rng.uniform(0.7, 1.0)
    out /= math.sqrt(n_voices)
    if breath > 0:
        out += bp(white(n, rng), 1800.0, 0.8, sr) * breath
    return out


VOWELS = {
    "u": [(300.0, 9.0, 1.0), (870.0, 10.0, 0.45), (2240.0, 12.0, 0.12)],
    "o": [(570.0, 8.0, 1.0), (840.0, 10.0, 0.55), (2410.0, 12.0, 0.14)],
    "a": [(730.0, 7.0, 1.0), (1090.0, 9.0, 0.7), (2440.0, 12.0, 0.22)],
    "e": [(530.0, 8.0, 1.0), (1840.0, 10.0, 0.5), (2480.0, 12.0, 0.25)],
    "i": [(270.0, 9.0, 1.0), (2290.0, 11.0, 0.35), (3010.0, 12.0, 0.2)],
}


def vowel(x: np.ndarray, name: str, sr: int) -> np.ndarray:
    return formants(x, VOWELS[name], sr)


def vowel_morph(x: np.ndarray, a: str, b: str, blend: np.ndarray, sr: int) -> np.ndarray:
    va = vowel(x, a, sr)
    vb = vowel(x, b, sr)
    return va * (1.0 - blend) + vb * blend


# ---------------------------------------------------------------------------
# Drum kit
# ---------------------------------------------------------------------------


def kick(sr: int, dur: float = 0.34, f0: float = 165.0, f1: float = 46.0, click: float = 0.4, sweep: float = 20.0) -> np.ndarray:
    t = t_axis(dur, sr)
    freq = f1 + (f0 - f1) * np.exp(-t * sweep)
    body = sine(freq, t, sr) * exp_env(t, 7.0)
    clk = sine(2200.0, t, sr) * exp_env(t, 220.0) * click
    return soft_clip(body * 1.2 + clk, 1.6)


def snare(sr: int, rng: np.random.Generator, dur: float = 0.2, tone: float = 190.0) -> np.ndarray:
    t = t_axis(dur, sr)
    body = (sine(tone, t, sr) * 0.5 + sine(tone * 1.7, t, sr) * 0.2) * exp_env(t, 20.0)
    air = hp(white(t.shape[0], rng), 1400.0, sr) * exp_env(t, 15.0) * 0.9
    return body + air


def clap(sr: int, rng: np.random.Generator, dur: float = 0.25) -> np.ndarray:
    t = t_axis(dur, sr)
    n = t.shape[0]
    out = np.zeros(n)
    noise = bp(white(n, rng), 1500.0, 1.2, sr)
    for k, at in enumerate((0.0, 0.011, 0.023, 0.034)):
        env = exp_env(t, 55.0 if k < 3 else 16.0, at)
        out += noise * env * (0.7 if k < 3 else 1.0)
    return out


def hat(sr: int, rng: np.random.Generator, dur: float = 0.06, bright: float = 7000.0, decay: float = 70.0) -> np.ndarray:
    t = t_axis(dur, sr)
    return hp(white(t.shape[0], rng), bright, sr) * exp_env(t, decay)


def ride(sr: int, rng: np.random.Generator, dur: float = 0.45) -> np.ndarray:
    t = t_axis(dur, sr)
    ping = modal(t, sr, [(3150.0, 0.5, 9.0), (4700.0, 0.3, 12.0), (6300.0, 0.2, 14.0)])
    wash = hp(white(t.shape[0], rng), 5000.0, sr) * exp_env(t, 8.0) * 0.35
    return ping + wash


def tom(sr: int, dur: float = 0.3, f: float = 140.0) -> np.ndarray:
    t = t_axis(dur, sr)
    freq = f * (1.0 + 0.35 * np.exp(-t * 30.0))
    return sine(freq, t, sr) * exp_env(t, 12.0)


def shaker(sr: int, rng: np.random.Generator, dur: float = 0.08) -> np.ndarray:
    t = t_axis(dur, sr)
    return bp(white(t.shape[0], rng), 6000.0, 1.5, sr) * hump(t, 0.0, dur, 1.5)


def crash(sr: int, rng: np.random.Generator, dur: float = 1.6) -> np.ndarray:
    t = t_axis(dur, sr)
    return hp(white(t.shape[0], rng), 3500.0, sr, 1.0) * exp_env(t, 2.6) * 0.6


def timpani(sr: int, dur: float = 1.2, f: float = 87.0) -> np.ndarray:
    t = t_axis(dur, sr)
    return modal(t, sr, [(f, 1.0, 3.5), (f * 1.5, 0.5, 5.0), (f * 1.98, 0.3, 6.0), (f * 2.44, 0.2, 8.0)]) + sine(
        f * 0.5, t, sr
    ) * exp_env(t, 6.0) * 0.3


# ---------------------------------------------------------------------------
# Notes
# ---------------------------------------------------------------------------


def midi(n: float) -> float:
    return 440.0 * 2.0 ** ((n - 69.0) / 12.0)


C2, EB2, F2, G2, AB2, BB2 = midi(36), midi(39), midi(41), midi(43), midi(44), midi(46)
C3, D3, EB3, F3, G3, AB3, BB3 = midi(48), midi(50), midi(51), midi(53), midi(55), midi(56), midi(58)
C4, D4, EB4, F4, G4, AB4, BB4 = midi(60), midi(62), midi(63), midi(65), midi(67), midi(68), midi(70)
C5, D5, EB5, F5, G5, AB5, BB5 = midi(72), midi(74), midi(75), midi(77), midi(79), midi(80), midi(82)
C6, E5, E6, G6 = midi(84), midi(76), midi(88), midi(91)


def pluck(freq: float, dur: float, sr: int, bright: float = 0.5) -> np.ndarray:
    t = t_axis(dur, sr)
    e = adsr(t, dur, 0.003, 0.05, 0.35, 0.05)
    return (square(freq, t, sr, 14) * bright + triangle(freq, t, sr) * (1.0 - bright)) * e * 0.5


def pad_chord(freqs, dur: float, sr: int, cutoff: float = 1800.0, attack: float = 0.06) -> np.ndarray:
    t = t_axis(dur, sr)
    e = adsr(t, dur, attack, 0.1, 0.8, 0.15)
    out = np.zeros_like(t)
    for f in freqs:
        out += supersaw(f, t, sr, 5, 0.009)
    out = lp(out, cutoff, sr, 1.5) * e / max(1, len(freqs))
    return out


def brass(freqs, dur: float, sr: int) -> np.ndarray:
    t = t_axis(dur, sr)
    e = adsr(t, dur, 0.035, 0.08, 0.8, 0.12)
    out = np.zeros_like(t)
    for f in freqs:
        vib = 1.0 + 0.004 * np.sin(TWO_PI * 5.2 * t)
        out += saw(f * vib, t, sr, 20) + square(f * 0.5, t, sr, 10) * 0.25
    return lp(out, 3200.0, sr, 1.2) * e / max(1, len(freqs))


def organ(freq: float, dur: float, sr: int) -> np.ndarray:
    t = t_axis(dur, sr)
    e = adsr(t, dur, 0.008, 0.02, 0.95, 0.04)
    drawbars = [1.0, 0.75, 0.55, 0.4, 0.0, 0.3, 0.0, 0.2]
    tone = additive(freq, t, sr, drawbars)
    leslie = 1.0 + 0.12 * np.sin(TWO_PI * 6.3 * t)
    return tone * e * leslie * 0.4


def bell(freq: float, dur: float, sr: int) -> np.ndarray:
    t = t_axis(dur, sr)
    idx_env = exp_env(t, 6.0)
    return fm(freq, freq * 1.41, 2.4, t, sr, idx_env) * exp_env(t, 5.0)


# ---------------------------------------------------------------------------
# WAV I/O
# ---------------------------------------------------------------------------


def wrap_tail(buf: np.ndarray, n: int) -> np.ndarray:
    """Fold everything past sample `n` back onto the start so ringing notes,
    echoes and reverb tails continue across the loop seam."""
    out = buf[..., :n].copy()
    extra = buf[..., n:]
    while extra.shape[-1] > 0:
        m = min(n, extra.shape[-1])
        out[..., :m] += extra[..., :m]
        extra = extra[..., m:]
    return out


def write_wav(path: str, data: np.ndarray, sr: int, peak: float = PEAK, is_loop: bool = False) -> int:
    os.makedirs(os.path.dirname(path), exist_ok=True)
    if data.ndim == 1:
        data = data[None, :]
    channels = data.shape[0]
    data = np.nan_to_num(data)
    if not is_loop:
        # One-shots: 1.5 ms in / 12 ms out so no clip starts or ends on a step.
        data = fade(data, sr, 0.0015, 0.012)
    data = normalize(data, peak)
    pcm = np.clip(np.round(data * 32767.0), -32768, 32767).astype("<i2")
    interleaved = np.ascontiguousarray(pcm.T).tobytes()
    with wave.open(path, "w") as wf:
        wf.setnchannels(channels)
        wf.setsampwidth(2)
        wf.setframerate(sr)
        wf.writeframes(interleaved)
    return os.path.getsize(path)


# ---------------------------------------------------------------------------
# BALL
# ---------------------------------------------------------------------------


def _ball_hit(sr: int, rng, dur: float, hz: float, decay: float, click: float, drop: float = 1.35, bright: float = 2600.0) -> np.ndarray:
    t = t_axis(dur, sr)
    freq = hz * (1.0 + (drop - 1.0) * np.exp(-t * 60.0))
    body = sine(freq, t, sr) * exp_env(t, decay)
    mode2 = sine(hz * 2.31, t, sr) * exp_env(t, decay * 2.2) * 0.22
    n = t.shape[0]
    slap = bp(white(n, rng), bright, 1.1, sr) * exp_env(t, 110.0) * click
    leather = lp(white(n, rng), 1200.0, sr) * exp_env(t, 55.0) * click * 0.5
    return soft_clip(body + mode2 + slap + leather, 1.4)


def render_bounces() -> list[tuple[str, np.ndarray, int]]:
    sr = SR_SFX
    rng = rng_for(101)
    specs = [
        ("ball/bounce_1.wav", 0.30, 126.0, 17.0, 0.55, 1.45, 2800.0),  # hard slam
        ("ball/bounce_2.wav", 0.28, 138.0, 19.0, 0.45, 1.35, 2500.0),
        ("ball/bounce_3.wav", 0.26, 146.0, 22.0, 0.36, 1.3, 2300.0),  # medium
        ("ball/bounce_4.wav", 0.24, 158.0, 26.0, 0.26, 1.25, 2100.0),
        ("ball/bounce_5.wav", 0.20, 172.0, 32.0, 0.16, 1.2, 1900.0),  # soft tap
    ]
    out = []
    for path, dur, hz, dec, clk, drop, br in specs:
        out.append((path, _ball_hit(sr, rng, dur, hz, dec, clk, drop, br), sr))
    return out


def render_dribbles() -> list[tuple[str, np.ndarray, int]]:
    sr = SR_SFX
    rng = rng_for(102)
    out = []
    for i, (hz, dec, clk) in enumerate(((186.0, 30.0, 0.5), (198.0, 34.0, 0.42), (176.0, 28.0, 0.55))):
        out.append((f"ball/dribble_{i + 1}.wav", _ball_hit(sr, rng, 0.15, hz, dec, clk, 1.25, 2400.0), sr))
    return out


def render_catch() -> np.ndarray:
    sr = SR_SFX
    rng = rng_for(103)
    t = t_axis(0.18, sr)
    n = t.shape[0]
    slap = bp(white(n, rng), 1900.0, 0.9, sr) * exp_env(t, 90.0) * 0.9
    palm = lp(white(n, rng), 700.0, sr) * exp_env(t, 40.0) * 0.6
    thump = sine(118.0 * (1.0 + 0.3 * np.exp(-t * 80)), t, sr) * exp_env(t, 32.0) * 0.8
    return soft_clip(slap + palm + thump, 1.3)


def render_pass_whoosh() -> np.ndarray:
    sr = SR_SFX
    rng = rng_for(104)
    t = t_axis(0.38, sr)
    body = sweep_noise(t, rng, sr, 500.0, 2600.0, 2.4) * hump(t, 0.0, 0.38, 1.6)
    air = hp(white(t.shape[0], rng), 4000.0, sr) * hump(t, 0.02, 0.3, 2.0) * 0.25
    return body + air


def render_shot_flick() -> np.ndarray:
    sr = SR_SFX
    rng = rng_for(105)
    t = t_axis(0.16, sr)
    n = t.shape[0]
    snap = hp(white(n, rng), 2500.0, sr) * exp_env(t, 160.0) * 0.8
    tick = sine(3100.0, t, sr) * exp_env(t, 300.0) * 0.4
    whoosh = sweep_noise(t, rng, sr, 900.0, 3200.0, 2.0) * hump(t, 0.01, 0.16, 1.4) * 0.55
    return snap + tick + whoosh


def render_rim_front() -> np.ndarray:
    """Front iron — dead, damped clank."""
    sr = SR_SFX
    rng = rng_for(106)
    t = t_axis(0.38, sr)
    body = modal(
        t,
        sr,
        [(612.0, 1.0, 28.0), (1134.0, 0.7, 34.0), (1790.0, 0.45, 40.0), (2610.0, 0.3, 48.0), (3420.0, 0.15, 60.0)],
    )
    impact = bp(white(t.shape[0], rng), 1800.0, 0.8, sr) * exp_env(t, 120.0) * 0.8
    thud = sine(160.0, t, sr) * exp_env(t, 30.0) * 0.5
    return soft_clip(body + impact + thud, 1.5)


def render_rim_back() -> np.ndarray:
    """Back iron — bright ringing with slow beating."""
    sr = SR_SFX
    rng = rng_for(107)
    t = t_axis(0.7, sr)
    body = modal(
        t,
        sr,
        [
            (884.0, 1.0, 6.5),
            (891.0, 0.6, 7.0),
            (1322.0, 0.7, 8.5),
            (2118.0, 0.5, 10.0),
            (3150.0, 0.3, 13.0),
            (4210.0, 0.18, 16.0),
        ],
    )
    impact = bp(white(t.shape[0], rng), 2400.0, 0.9, sr) * exp_env(t, 110.0) * 0.6
    return soft_clip(body + impact, 1.3)


def render_rim_soft() -> np.ndarray:
    sr = SR_SFX
    rng = rng_for(108)
    t = t_axis(0.32, sr)
    body = modal(t, sr, [(884.0, 0.6, 14.0), (1322.0, 0.4, 16.0), (2118.0, 0.25, 20.0)])
    graze = bp(white(t.shape[0], rng), 2600.0, 1.0, sr) * exp_env(t, 70.0) * 0.5
    return body + graze


def render_backboard() -> np.ndarray:
    sr = SR_SFX
    rng = rng_for(109)
    t = t_axis(0.32, sr)
    n = t.shape[0]
    glass = modal(t, sr, [(392.0, 1.0, 18.0), (718.0, 0.6, 22.0), (1284.0, 0.4, 26.0), (2090.0, 0.2, 32.0)])
    thud = sine(96.0, t, sr) * exp_env(t, 18.0) * 0.6
    knock = lp(white(n, rng), 2600.0, sr) * exp_env(t, 40.0) * 0.6
    return soft_clip(glass + thud + knock, 1.3)


def render_backboard_hard() -> np.ndarray:
    sr = SR_SFX
    rng = rng_for(110)
    t = t_axis(0.42, sr)
    n = t.shape[0]
    glass = modal(t, sr, [(392.0, 1.0, 12.0), (718.0, 0.7, 14.0), (1284.0, 0.55, 18.0), (2090.0, 0.35, 22.0), (3300.0, 0.2, 30.0)])
    thud = sine(82.0 * (1.0 + 0.3 * np.exp(-t * 40)), t, sr) * exp_env(t, 12.0) * 0.9
    knock = lp(white(n, rng), 3200.0, sr) * exp_env(t, 30.0) * 0.8
    rattle = bp(white(n, rng), 5200.0, 2.0, sr) * exp_env(t, 9.0) * 0.12 * (1.0 + np.sin(TWO_PI * 27.0 * t))
    return soft_clip(glass + thud + knock + rattle, 1.4)


def _net(sr: int, rng, dur: float, snap: float, fizz: float, jingle: float) -> np.ndarray:
    t = t_axis(dur, sr)
    n = t.shape[0]
    net_snap = hp(white(n, rng), 1500.0, sr) * exp_env(t, 60.0) * snap
    strings = bp(white(n, rng), 4800.0, 1.4, sr) * exp_env(t, 11.0) * fizz
    strings += bp(white(n, rng), 2600.0, 1.6, sr) * exp_env(t, 14.0) * fizz * 0.5
    ping = modal(t, sr, [(1780.0, 0.25, 40.0), (2640.0, 0.18, 50.0)])
    out = net_snap + strings + ping
    if jingle > 0:
        for k in range(9):
            at = 0.03 + k * rng.uniform(0.018, 0.045)
            f = rng.uniform(3200.0, 7600.0)
            out += sine(f, t, sr) * exp_env(t, 90.0, at) * jingle * rng.uniform(0.4, 1.0)
    return out


def render_swish() -> np.ndarray:
    return _net(SR_SFX, rng_for(111), 0.5, 0.5, 1.4, 0.16)


def render_swish_soft() -> np.ndarray:
    return _net(SR_SFX, rng_for(112), 0.36, 0.35, 1.1, 0.08)


def render_rattle() -> np.ndarray:
    """In-and-out: several diminishing iron kisses while the ball dances on the rim."""
    sr = SR_SFX
    rng = rng_for(113)
    dur = 0.95
    t = t_axis(dur, sr)
    out = np.zeros_like(t)
    front = render_rim_front()
    back = render_rim_back()
    at = 0.0
    g = 1.0
    k = 0
    while at < 0.62:
        clip = front if k % 2 == 0 else back
        add_sec(out, resample(clip, rng.uniform(0.94, 1.08)), at, sr, g)
        at += rng.uniform(0.085, 0.15)
        g *= 0.72
        k += 1
    roll = bp(white(t.shape[0], rng), 1400.0, 1.0, sr) * hump(t, 0.0, 0.8, 1.2) * 0.22
    return soft_clip(out + roll, 1.3)


def render_roll_loop() -> np.ndarray:
    """Loose ball rolling: rubbery rumble + pebble grain. 1.5 s loop."""
    sr = SR_SFX
    rng = rng_for(114)
    dur = 1.5
    t = t_axis(dur, sr)
    n = t.shape[0]
    rumble = lp(brown(n, rng, sr), 260.0, sr, 1.5) * 0.9
    res = bp(white(n, rng), 92.0, 4.0, sr) * 0.5
    grain = np.zeros(n)
    at = 0.0
    while at < dur:
        i = int(at * sr)
        length = min(n - i, int(0.012 * sr))
        if length > 0:
            grain[i : i + length] += bp(white(length, rng), rng.uniform(900.0, 2400.0), 1.5, sr) * rng.uniform(0.2, 0.7)
        at += rng.uniform(0.035, 0.11)
    wobble = 1.0 + 0.18 * np.sin(TWO_PI * 3.1 * t) + 0.1 * np.sin(TWO_PI * 5.7 * t)
    return make_loop(hp((rumble + res + grain * 0.5) * wobble, 28.0, sr, 1.0), sr, 0.3)


# ---------------------------------------------------------------------------
# PLAYER
# ---------------------------------------------------------------------------


def _squeak(sr: int, rng, dur: float, f0: float, f1: float, wobble: float, noise: float) -> np.ndarray:
    t = t_axis(dur, sr)
    freq = f0 + (f1 - f0) * (t / dur)
    freq = freq * (1.0 + wobble * np.sin(TWO_PI * 43.0 * t))
    e = adsr(t, dur, 0.004, dur * 0.3, 0.35, dur * 0.35)
    tone = sine(freq, t, sr) + sine(freq * 2.02, t, sr) * 0.3 + sine(freq * 3.1, t, sr) * 0.1
    rub = bp(white(t.shape[0], rng), (f0 + f1) * 0.5, 3.0, sr) * noise
    return (tone + rub) * e


def render_squeaks() -> list[tuple[str, np.ndarray, int]]:
    sr = SR_SFX
    rng = rng_for(120)
    return [
        ("player/squeak_short_1.wav", _squeak(sr, rng, 0.12, 2500.0, 1350.0, 0.0, 0.25), sr),
        ("player/squeak_short_2.wav", _squeak(sr, rng, 0.10, 1850.0, 2450.0, 0.02, 0.2), sr),
        ("player/squeak_long.wav", _squeak(sr, rng, 0.32, 2100.0, 1500.0, 0.035, 0.45), sr),
    ]


def render_steps() -> list[tuple[str, np.ndarray, int]]:
    sr = SR_SFX
    rng = rng_for(121)
    out = []
    for i, (fc, thump) in enumerate(((900.0, 0.5), (1150.0, 0.4), (760.0, 0.6))):
        t = t_axis(0.11, sr)
        n = t.shape[0]
        pad = lp(white(n, rng), fc, sr) * exp_env(t, 60.0)
        heel = sine(84.0, t, sr) * exp_env(t, 40.0) * thump
        scuff = bp(white(n, rng), 2800.0, 1.5, sr) * exp_env(t, 90.0, 0.012) * 0.25
        out.append((f"player/step_{i + 1}.wav", pad + heel + scuff, sr))
    return out


def render_jump_grunt() -> np.ndarray:
    sr = SR_SFX
    rng = rng_for(122)
    dur = 0.3
    t = t_axis(dur, sr)
    contour = 1.18 - 0.28 * ramp(t, 0.02, 0.25)
    v = voice_chorus(t, sr, rng, 2, 118.0, 150.0, contour, spread_s=0.01, vib_depth=0.004, breath=0.3)
    blend = ramp(t, 0.05, 0.22)
    out = vowel_morph(v, "a", "u", blend, sr) * adsr(t, dur, 0.015, 0.08, 0.6, 0.1)
    out += hp(white(t.shape[0], rng), 3000.0, sr) * exp_env(t, 30.0) * 0.12
    return out


def render_land() -> np.ndarray:
    sr = SR_SFX
    rng = rng_for(123)
    t = t_axis(0.24, sr)
    n = t.shape[0]
    thump = kick(sr, 0.24, 120.0, 52.0, 0.15, 25.0) * 0.9
    floor = lp(white(n, rng), 650.0, sr) * exp_env(t, 32.0) * 0.7
    sole = bp(white(n, rng), 2200.0, 1.2, sr) * exp_env(t, 80.0, 0.006) * 0.3
    return soft_clip(thump + floor + sole, 1.3)


def render_dunk_boom() -> np.ndarray:
    sr = SR_SFX
    rng = rng_for(124)
    dur = 0.9
    t = t_axis(dur, sr)
    n = t.shape[0]
    sub = sine(48.0, t, sr) * exp_env(t, 5.5) + sine(36.0, t, sr) * exp_env(t, 4.5) * 0.7
    punch = kick(sr, 0.3, 140.0, 44.0, 0.5, 18.0)
    out = sub * 1.1
    add_at(out, punch, 0, 0.9)
    iron = render_rim_back()
    add_sec(out, iron, 0.015, sr, 0.8)
    add_sec(out, render_rim_front(), 0.0, sr, 0.6)
    shake = sine(188.0, t, sr) * exp_env(t, 4.0, 0.02) * (0.5 + 0.5 * np.sin(TWO_PI * 9.0 * t)) * 0.28
    board = lp(white(n, rng), 1800.0, sr) * exp_env(t, 9.0, 0.02) * (0.5 + 0.5 * np.sin(TWO_PI * 21.0 * t)) * 0.2
    return soft_clip(out + shake + board, 1.5)


def render_block_slap() -> np.ndarray:
    sr = SR_SFX
    rng = rng_for(125)
    t = t_axis(0.26, sr)
    n = t.shape[0]
    slap = hp(white(n, rng), 1200.0, sr) * exp_env(t, 95.0) * 1.0
    body = sine(290.0 * (1.0 + 0.4 * np.exp(-t * 90)), t, sr) * exp_env(t, 36.0) * 0.8
    ring = modal(t, sr, [(1120.0, 0.35, 45.0), (1680.0, 0.2, 60.0)])
    return soft_clip(slap + body + ring, 1.4)


def render_steal_rip() -> np.ndarray:
    sr = SR_SFX
    rng = rng_for(126)
    t = t_axis(0.32, sr)
    rip = sweep_noise(t, rng, sr, 450.0, 4200.0, 3.0) * hump(t, 0.0, 0.2, 1.2) * 1.0
    slap = hp(white(t.shape[0], rng), 1600.0, sr) * exp_env(t, 110.0, 0.09) * 0.7
    chirp = _squeak(sr, rng, 0.12, 2300.0, 2900.0, 0.0, 0.1)
    out = rip + slap
    add_sec(out, chirp, 0.1, sr, 0.5)
    return out


def render_body_thud() -> np.ndarray:
    sr = SR_SFX
    rng = rng_for(127)
    t = t_axis(0.34, sr)
    n = t.shape[0]
    thump = kick(sr, 0.34, 105.0, 48.0, 0.1, 22.0) * 0.9
    body = lp(white(n, rng), 420.0, sr) * exp_env(t, 26.0) * 0.8
    cloth = bp(white(n, rng), 2200.0, 1.0, sr) * exp_env(t, 40.0, 0.01) * 0.3
    return soft_clip(thump + body + cloth, 1.3)


# ---------------------------------------------------------------------------
# CROWD
# ---------------------------------------------------------------------------


def _crowd_layer(seed: int, dur: float, density: float, bright: float, level: float, hey: float) -> np.ndarray:
    """One stereo crowd bed: pink wash + murmur bursts + optional shouted vowels."""
    sr = SR_SFX
    rng = rng_for(seed)
    n = n_of(dur, sr)
    t = np.arange(n) / sr
    chans = []
    for ch in range(2):
        wash = lp(pink(n, rng, sr), bright, sr, 1.0)
        wash = wash * (1.0 + 0.18 * np.sin(TWO_PI * rng.uniform(0.3, 0.6) * t + ch) + 0.1 * np.sin(TWO_PI * rng.uniform(1.4, 2.3) * t))
        murmur = np.zeros(n)
        count = int(density * dur)
        for _ in range(count):
            at = rng.uniform(0.0, dur)
            length = rng.uniform(0.12, 0.42)
            i = int(at * sr)
            m = min(n - i, n_of(length, sr))
            if m <= 0:
                continue
            tt = np.arange(m) / sr
            burst = bp(white(m, rng), rng.uniform(300.0, 900.0), 1.6, sr) * hump(tt, 0.0, length, 1.3)
            burst *= 1.0 + 0.6 * np.sin(TWO_PI * rng.uniform(3.0, 6.0) * tt)
            murmur[i : i + m] += burst * rng.uniform(0.3, 1.0)
        layer = wash * 0.7 + murmur * 0.55
        if hey > 0:
            shouts = np.zeros(n)
            for _ in range(int(hey * dur)):
                at = rng.uniform(0.0, dur - 0.5)
                length = rng.uniform(0.25, 0.6)
                i = int(at * sr)
                m = min(n - i, n_of(length, sr))
                tt = np.arange(m) / sr
                v = voice_chorus(tt, sr, rng, 3, 140.0, 330.0, None, 0.03, 6.0, 0.012, 0.1)
                v = vowel(v, rng.choice(["a", "o", "e"]), sr) * hump(tt, 0.0, length, 1.1)
                shouts[i : i + m] += v * rng.uniform(0.3, 0.8)
            layer += shouts * 0.35
        chans.append(layer)
    st = pan_pair(chans[0], chans[1], 0.9)
    st = comb_reverb(st, sr, wet=0.35, decay=0.7, damp_fc=2400.0)
    st = make_loop(hp(st, 40.0, sr, 1.0), sr, 0.5)
    return normalize(st, level)


def render_bed_murmur() -> np.ndarray:
    return _crowd_layer(201, 5.0, 14.0, 1500.0, 0.8, 0.0)


def render_bed_excited() -> np.ndarray:
    return _crowd_layer(202, 5.0, 40.0, 2600.0, 0.8, 1.2)


def render_bed_roar() -> np.ndarray:
    return _crowd_layer(203, 5.0, 90.0, 4200.0, 0.8, 4.0)


def _cheer(seed: int, dur: float, voices: int, rise: float, whistle: bool, claps: int) -> np.ndarray:
    sr = SR_SFX
    rng = rng_for(seed)
    t = t_axis(dur, sr)
    n = t.shape[0]
    env = ramp(t, 0.0, rise) ** 1.2 * (1.0 - 0.35 * ramp(t, rise + 0.2, dur))
    env *= 1.0 - ramp(t, dur - 0.25, dur) ** 2
    chans = []
    for ch in range(2):
        v = voice_chorus(t, sr, rng, voices, 150.0, 420.0, None, rise * 0.8, 6.0, 0.014, 0.35)
        blend = ramp(t, 0.1, 0.6)
        v = vowel_morph(v, "e", "a", blend, sr)
        roar = bp(white(n, rng), 1200.0, 0.7, sr) * 0.6 + bp(white(n, rng), 500.0, 1.0, sr) * 0.5
        layer = (v + roar) * env
        if whistle:
            for _ in range(3 + ch):
                at = rng.uniform(0.15, dur * 0.5)
                f0 = rng.uniform(2200.0, 3200.0)
                wl = rng.uniform(0.3, 0.7)
                freq = f0 * (1.0 + 0.06 * np.sin(TWO_PI * 7.0 * t) + 0.08 * ramp(t, at, at + wl))
                layer += sine(freq, t, sr) * hump(t, at, at + wl, 1.4) * 0.07
        if claps > 0:
            for _ in range(claps):
                at = rng.uniform(0.1, dur - 0.3)
                add_sec(layer, clap(sr, rng, 0.2) * rng.uniform(0.15, 0.35), at, sr)
        chans.append(layer)
    st = pan_pair(chans[0], chans[1], 0.85)
    return comb_reverb(st, sr, wet=0.4, decay=0.75, damp_fc=3000.0)


def render_cheer_small() -> np.ndarray:
    return _cheer(211, 1.1, 8, 0.3, False, 0)


def render_cheer_big() -> np.ndarray:
    return _cheer(212, 1.9, 14, 0.4, True, 6)


def render_cheer_huge() -> np.ndarray:
    return _cheer(213, 2.8, 20, 0.45, True, 14)


def render_oooh() -> np.ndarray:
    sr = SR_SFX
    rng = rng_for(214)
    dur = 1.2
    t = t_axis(dur, sr)
    contour = 1.0 + 0.16 * hump(t, 0.0, 0.7, 1.0) - 0.1 * ramp(t, 0.6, 1.2)
    chans = []
    for ch in range(2):
        v = voice_chorus(t, sr, rng, 14, 140.0, 380.0, contour, 0.12, 5.5, 0.012, 0.15)
        v = vowel_morph(v, "u", "o", ramp(t, 0.3, 0.9), sr)
        env = ramp(t, 0.0, 0.25) * (1.0 - ramp(t, 0.75, dur) ** 1.5)
        chans.append(v * env)
    st = pan_pair(chans[0], chans[1], 0.8)
    return comb_reverb(st, sr, wet=0.4, decay=0.75)


def render_gasp() -> np.ndarray:
    sr = SR_SFX
    rng = rng_for(215)
    dur = 0.75
    t = t_axis(dur, sr)
    chans = []
    for ch in range(2):
        inhale = sweep_noise(t, rng, sr, 900.0, 4800.0, 1.6) * hump(t, 0.0, 0.4, 1.0)
        breathy = hp(white(t.shape[0], rng), 2500.0, sr) * hump(t, 0.05, 0.5, 1.4) * 0.35
        v = voice_chorus(t, sr, rng, 6, 200.0, 420.0, None, 0.05, 6.0, 0.01, 0.4)
        v = vowel(v, "a", sr) * hump(t, 0.02, 0.35, 1.3) * 0.25
        chans.append(inhale + breathy + v)
    return comb_reverb(pan_pair(chans[0], chans[1], 0.75), sr, wet=0.3, decay=0.6)


def render_groan() -> np.ndarray:
    sr = SR_SFX
    rng = rng_for(216)
    dur = 1.4
    t = t_axis(dur, sr)
    contour = 1.05 - 0.22 * ramp(t, 0.1, 1.1)
    chans = []
    for ch in range(2):
        v = voice_chorus(t, sr, rng, 12, 110.0, 300.0, contour, 0.15, 5.0, 0.012, 0.18)
        v = vowel_morph(v, "o", "a", ramp(t, 0.2, 0.8), sr)
        env = ramp(t, 0.0, 0.2) * (1.0 - ramp(t, 0.7, dur) ** 1.2)
        chans.append(v * env)
    return comb_reverb(pan_pair(chans[0], chans[1], 0.8), sr, wet=0.4, decay=0.7)


def render_boo() -> np.ndarray:
    sr = SR_SFX
    rng = rng_for(217)
    dur = 1.5
    t = t_axis(dur, sr)
    chans = []
    for ch in range(2):
        v = voice_chorus(t, sr, rng, 12, 105.0, 260.0, 1.0 - 0.04 * ramp(t, 0.3, 1.4), 0.25, 4.5, 0.01, 0.12)
        v = vowel(v, "u", sr)
        env = ramp(t, 0.0, 0.35) * (1.0 - ramp(t, 0.9, dur) ** 1.4)
        chans.append(v * env)
    return comb_reverb(pan_pair(chans[0], chans[1], 0.8), sr, wet=0.4, decay=0.7)


def render_anticipation() -> np.ndarray:
    """Rising murmur — the bowl leans in before a big play."""
    sr = SR_SFX
    rng = rng_for(218)
    dur = 0.95
    t = t_axis(dur, sr)
    n = t.shape[0]
    chans = []
    for ch in range(2):
        wash = bp(white(n, rng), 700.0, 0.8, sr) * ramp(t, 0.0, 0.8) ** 1.6
        v = voice_chorus(t, sr, rng, 8, 160.0, 400.0, 1.0 + 0.1 * ramp(t, 0.0, dur), 0.3, 5.5, 0.012, 0.2)
        v = vowel(v, "o", sr) * ramp(t, 0.1, 0.9) ** 1.4
        chans.append((wash * 0.7 + v * 0.6) * (1.0 - ramp(t, 0.85, dur)))
    return comb_reverb(pan_pair(chans[0], chans[1], 0.8), sr, wet=0.35, decay=0.7)


def render_stomp_clap() -> np.ndarray:
    """2 s loop @120 BPM: DE- (stomp) FENSE (stomp) clap clap — arena defense chant."""
    sr = SR_SFX
    rng = rng_for(219)
    dur = 2.0
    n = n_of(dur, sr)
    beat = 0.5
    chans = []
    for ch in range(2):
        out = np.zeros(n)
        stomp = kick(sr, 0.3, 120.0, 55.0, 0.1, 20.0) * 0.9 + np.concatenate(
            [lp(white(n_of(0.3, sr), rng), 400.0, sr) * exp_env(t_axis(0.3, sr), 30.0) * 0.6]
        )
        for b in (0.0, 1.0):
            add_sec(out, stomp, b * beat, sr, 1.0)
        for b in (2.0, 2.5):
            cl = np.zeros(n_of(0.3, sr))
            for _ in range(6):
                add_sec(cl, clap(sr, rng, 0.22) * rng.uniform(0.3, 0.7), rng.uniform(0.0, 0.04), sr)
            add_sec(out, cl, b * beat, sr, 0.9)
        # chant syllables
        for b, vow_a, vow_b, f_lo, f_hi in ((0.0, "i", "i", 170.0, 380.0), (1.0, "e", "e", 160.0, 360.0)):
            length = 0.42
            tt = t_axis(length, sr)
            v = voice_chorus(tt, sr, rng, 12, f_lo, f_hi, None, 0.04, 5.5, 0.01, 0.1)
            if b == 0.0:
                v = vowel(v, vow_a, sr)
                # 'D' onset: short lowpassed burst
                v[: n_of(0.03, sr)] *= np.linspace(0.0, 1.0, n_of(0.03, sr))
            else:
                v = vowel(v, vow_b, sr)
                add_at(v, hp(white(n_of(0.08, sr), rng), 3000.0, sr) * exp_env(t_axis(0.08, sr), 40.0) * 0.5, 0)  # 'F'
                add_at(v, hp(white(n_of(0.1, sr), rng), 4000.0, sr) * exp_env(t_axis(0.1, sr), 35.0) * 0.4, n_of(0.3, sr))  # 'S'
            v *= adsr(tt, length, 0.02, 0.05, 0.85, 0.12)
            add_sec(out, v, b * beat, sr, 0.75)
        chans.append(out)
    st = pan_pair(chans[0], chans[1], 0.7)
    st = comb_reverb(st, sr, wet=0.3, decay=0.65)
    return make_loop(st, sr, 0.06)


def render_chant() -> np.ndarray:
    """4 s loop @120 BPM: melodic 'oh-oh-oh-ohhh' with claps — heater's anthem."""
    sr = SR_SFX
    rng = rng_for(220)
    dur = 4.0
    n = n_of(dur, sr)
    beat = 0.5
    notes = [(0.0, 0.45, G4), (1.0, 0.45, G4), (2.0, 0.45, BB4), (3.0, 1.9, C5), (5.0, 0.45, BB4), (6.0, 1.9, G4)]
    chans = []
    for ch in range(2):
        out = np.zeros(n)
        for b, length, f in notes:
            tt = t_axis(length, sr)
            v = voice_chorus(tt, sr, rng, 10, f * 0.49, f * 0.51, None, 0.03, 5.5, 0.01, 0.08)
            v += voice_chorus(tt, sr, rng, 6, f * 0.98, f * 1.02, None, 0.03, 5.5, 0.01, 0.05) * 0.6
            v = vowel(v, "o", sr) * adsr(tt, length, 0.03, 0.05, 0.85, 0.1)
            add_sec(out, v, b * beat, sr, 0.8)
        for b in (1.0, 3.0, 5.0, 7.0):
            cl = np.zeros(n_of(0.3, sr))
            for _ in range(5):
                add_sec(cl, clap(sr, rng, 0.2) * rng.uniform(0.3, 0.7), rng.uniform(0.0, 0.035), sr)
            add_sec(out, cl, b * beat, sr, 0.7)
        chans.append(out)
    st = pan_pair(chans[0], chans[1], 0.7)
    st = comb_reverb(st, sr, wet=0.35, decay=0.7)
    return make_loop(st, sr, 0.05)


def render_airhorn() -> np.ndarray:
    sr = SR_SFX
    dur = 1.3
    t = t_axis(dur, sr)
    dip = 1.0 - 0.06 * np.exp(-t * 25.0)
    tone = np.zeros_like(t)
    for f, g in ((233.1, 1.0), (311.1, 0.8), (466.2, 0.5)):
        tone += saw(f * dip * (1.0 + 0.003 * np.sin(TWO_PI * 4.0 * t)), t, sr, 26) * g
    tone = peak_eq(tone, 780.0, 1.4, 8.0, sr)
    tone = lp(tone, 5200.0, sr, 1.5)
    env = adsr(t, dur, 0.015, 0.05, 0.9, 0.12)
    st = stereo(soft_clip(tone * env, 1.8), sr, 0.2, 0.7)
    return comb_reverb(st, sr, wet=0.3, decay=0.7)


def render_whistles() -> np.ndarray:
    """Crowd whistling — several fingers-in-mouth sweeps."""
    sr = SR_SFX
    rng = rng_for(221)
    dur = 1.1
    t = t_axis(dur, sr)
    chans = []
    for ch in range(2):
        out = np.zeros_like(t)
        for _ in range(4):
            at = rng.uniform(0.0, 0.45)
            wl = rng.uniform(0.35, 0.6)
            f0 = rng.uniform(2000.0, 3100.0)
            freq = f0 * (1.0 + 0.25 * ramp(t, at, at + wl * 0.5) - 0.15 * ramp(t, at + wl * 0.5, at + wl))
            freq *= 1.0 + 0.02 * np.sin(TWO_PI * 9.0 * t)
            out += (sine(freq, t, sr) + sine(freq * 2.0, t, sr) * 0.12) * hump(t, at, at + wl, 1.2) * 0.3
        chans.append(out)
    return comb_reverb(pan_pair(chans[0], chans[1], 0.9), sr, wet=0.35, decay=0.7)


# ---------------------------------------------------------------------------
# GAME / BROADCAST
# ---------------------------------------------------------------------------


def render_shot_tick() -> np.ndarray:
    sr = SR_SFX
    t = t_axis(0.13, sr)
    tone = sine(1180.0, t, sr) * exp_env(t, 45.0) + sine(2360.0, t, sr) * exp_env(t, 70.0) * 0.4
    click = sine(4200.0, t, sr) * exp_env(t, 400.0) * 0.5
    return tone + click


def _buzzer(dur: float) -> np.ndarray:
    sr = SR_SFX
    t = t_axis(dur, sr)
    tone = square(415.0, t, sr, 18) * 0.6 + square(622.0, t, sr, 14) * 0.45 + saw(207.5 * 1.004, t, sr, 20) * 0.5
    tone *= 1.0 + 0.08 * np.sign(np.sin(TWO_PI * 48.0 * t))
    tone = lp(tone, 4200.0, sr, 1.5)
    env = adsr(t, dur, 0.006, 0.02, 0.95, 0.05)
    return soft_clip(tone * env, 2.0)


def render_buzzer_long() -> np.ndarray:
    return _buzzer(1.4)


def render_buzzer_short() -> np.ndarray:
    return _buzzer(0.6)


def _whistle(dur: float, f: float = 2900.0) -> np.ndarray:
    sr = SR_SFX
    t = t_axis(dur, sr)
    pea = 1.0 + 0.32 * np.sin(TWO_PI * 38.0 * t + 0.3) * ramp(t, 0.02, 0.05)
    vib = 1.0 + 0.006 * np.sin(TWO_PI * 14.0 * t)
    tone = sine(f * vib, t, sr) + sine(f * 2.0 * vib, t, sr) * 0.22 + sine(f * 1.5 * vib, t, sr) * 0.08
    env = adsr(t, dur, 0.012, 0.03, 0.88, 0.05)
    air = hp(white(t.shape[0], rng_for(int(dur * 1000))), 5000.0, sr) * env * 0.08
    return tone * env * pea + air


def render_whistle_short() -> np.ndarray:
    return _whistle(0.24)


def render_whistle_long() -> np.ndarray:
    return _whistle(0.6)


def render_whistle_double() -> np.ndarray:
    sr = SR_SFX
    out = zeros(0.55, sr)
    add_sec(out, _whistle(0.2), 0.0, sr)
    add_sec(out, _whistle(0.26, 2960.0), 0.27, sr)
    return out


def render_possession_chime() -> np.ndarray:
    sr = SR_SFX
    out = zeros(0.55, sr)
    add_sec(out, bell(G5, 0.5, sr), 0.0, sr, 0.8)
    add_sec(out, bell(D5 * 2.0, 0.42, sr), 0.11, sr, 0.7)
    return out


def render_organ_charge() -> np.ndarray:
    """Jumbotron 'CHARGE!' riff on a drawbar organ."""
    sr = SR_SFX
    seq = [(0.0, 0.16, G4), (0.17, 0.16, C5), (0.34, 0.16, E5), (0.51, 0.42, G5), (0.95, 0.16, E5), (1.12, 0.75, G5)]
    out = zeros(2.3, sr)
    for at, length, f in seq:
        add_sec(out, organ(f, length, sr), at, sr)
        add_sec(out, organ(f * 0.5, length, sr), at, sr, 0.45)
    st = stereo(out, sr, 0.25, 0.9)
    return comb_reverb(st, sr, wet=0.4, decay=0.7)


# ---------------------------------------------------------------------------
# STINGERS
# ---------------------------------------------------------------------------


def _hit(sr: int, rng, dur: float = 0.9) -> np.ndarray:
    t = t_axis(dur, sr)
    boom = kick(sr, dur, 130.0, 42.0, 0.6, 16.0)
    sub = sine(45.0, t, sr) * exp_env(t, 5.0) * 0.8
    crack = hp(white(t.shape[0], rng), 1500.0, sr) * exp_env(t, 60.0) * 0.7
    return soft_clip(boom + sub + crack, 1.4)


def render_on_fire() -> np.ndarray:
    sr = SR_SFX
    rng = rng_for(301)
    dur = 1.3
    t = t_axis(dur, sr)
    rise = sweep_noise(t, rng, sr, 300.0, 6000.0, 2.0) * ramp(t, 0.0, 0.42) ** 2 * (t < 0.44)
    tone_f = 220.0 * 2.0 ** (2.0 * ramp(t, 0.0, 0.42))
    riser = saw(tone_f, t, sr, 12) * ramp(t, 0.0, 0.4) ** 1.5 * (t < 0.44) * 0.5
    out = rise * 0.8 + riser
    add_sec(out, _hit(sr, rng, 0.85), 0.44, sr, 1.0)
    sizzle = hp(white(t.shape[0], rng), 4500.0, sr) * exp_env(t, 4.0, 0.44) * (t >= 0.44) * 0.45
    sizzle *= 1.0 + 0.5 * np.sin(TWO_PI * 33.0 * t)
    chord = brass([C4, EB4, G4, BB4], 0.8, sr)
    add_sec(out, chord, 0.45, sr, 0.5)
    return comb_reverb(stereo(soft_clip(out + sizzle, 1.3), sr, 0.25, 0.8), sr, wet=0.3, decay=0.7)


def render_lead_change() -> np.ndarray:
    sr = SR_SFX
    rng = rng_for(302)
    out = zeros(1.1, sr)
    add_sec(out, brass([AB3, C4, EB4], 0.3, sr), 0.0, sr, 0.9)
    add_sec(out, brass([BB3, D4, F4, BB4], 0.7, sr), 0.28, sr, 1.0)
    add_sec(out, _hit(sr, rng, 0.7), 0.28, sr, 0.7)
    add_sec(out, hp(white(n_of(0.5, sr), rng), 3000.0, sr) * exp_env(t_axis(0.5, sr), 12.0), 0.28, sr, 0.2)
    return comb_reverb(stereo(out, sr, 0.2, 0.7), sr, wet=0.35, decay=0.7)


def render_clutch() -> np.ndarray:
    """Game-point / clutch-time cue: drum roll into a low hit and a tense chord."""
    sr = SR_SFX
    rng = rng_for(303)
    dur = 1.5
    out = zeros(dur, sr)
    at = 0.0
    g = 0.25
    while at < 0.55:
        add_sec(out, tom(sr, 0.12, 165.0), at, sr, g)
        at += 0.05
        g += 0.06
    add_sec(out, _hit(sr, rng, 0.9), 0.58, sr, 1.0)
    add_sec(out, pad_chord([C3, EB3, G3, D4], 0.9, sr, 1400.0, 0.02), 0.58, sr, 0.8)
    t = t_axis(dur, sr)
    tick = np.zeros_like(t)
    for k in range(4):
        tick += sine(1500.0, t, sr) * exp_env(t, 80.0, 0.7 + k * 0.18) * (t >= 0.7 + k * 0.18) * 0.18
    return comb_reverb(stereo(out + tick, sr, 0.2, 0.7), sr, wet=0.3, decay=0.7)


def render_final_minute() -> np.ndarray:
    sr = SR_SFX
    rng = rng_for(304)
    out = zeros(1.3, sr)
    add_sec(out, brass([C3, G3, C4], 0.35, sr), 0.0, sr, 0.9)
    add_sec(out, brass([AB2, EB3, AB3, C4], 0.8, sr), 0.4, sr, 1.0)
    add_sec(out, _hit(sr, rng, 0.8), 0.4, sr, 0.6)
    t = t_axis(1.3, sr)
    ticks = np.zeros_like(t)
    for k in range(5):
        at = 0.45 + k * 0.16
        ticks += sine(1200.0, t, sr) * exp_env(t, 90.0, at) * (t >= at) * 0.15
    return comb_reverb(stereo(out + ticks, sr, 0.2, 0.7), sr, wet=0.3, decay=0.7)


def render_anthem() -> np.ndarray:
    """Tip-off anthem sting — bright brass fanfare over timpani."""
    sr = SR_SFX
    rng = rng_for(305)
    out = zeros(3.1, sr)
    seq = [
        (0.0, 0.22, [C4, G4, C5]),
        (0.24, 0.22, [F4, AB4, C5]),
        (0.48, 0.22, [G4, BB4, D5]),
        (0.72, 0.9, [C4, EB4, G4, C5, EB5]),
        (1.7, 0.3, [BB3, D4, F4, BB4]),
        (2.05, 1.0, [C4, EB4, G4, C5, G5]),
    ]
    for at, length, notes in seq:
        add_sec(out, brass(notes, length, sr), at, sr, 0.9)
    for at, f in ((0.0, 65.4), (0.72, 65.4), (1.7, 58.3), (2.05, 65.4)):
        add_sec(out, timpani(sr, 1.0, f), at, sr, 0.8)
    add_sec(out, crash(sr, rng, 1.6), 2.05, sr, 0.5)
    return comb_reverb(stereo(soft_clip(out, 1.3), sr, 0.25, 0.9), sr, wet=0.4, decay=0.78)


def render_fanfare_win() -> np.ndarray:
    sr = SR_SFX
    rng = rng_for(306)
    out = zeros(3.6, sr)
    arp = [(0.0, C4), (0.12, EB4), (0.24, G4), (0.36, C5), (0.48, EB5), (0.6, G5)]
    for at, f in arp:
        add_sec(out, pluck(f, 0.3, sr, 0.6), at, sr, 0.7)
    add_sec(out, brass([C4, EB4, G4, C5], 0.5, sr), 0.72, sr, 0.9)
    add_sec(out, brass([F4, AB4, C5, F5], 0.5, sr), 1.25, sr, 0.9)
    add_sec(out, brass([G4, BB4, D5, G5], 0.4, sr), 1.78, sr, 0.9)
    add_sec(out, brass([C4, EB4, G4, C5, EB5, G5], 1.5, sr), 2.2, sr, 1.0)
    add_sec(out, pad_chord([C3, G3, C4], 1.5, sr, 2400.0, 0.05), 2.2, sr, 0.5)
    for at in (0.72, 1.25, 1.78, 2.2):
        add_sec(out, kick(sr, 0.3, 150.0, 45.0, 0.5), at, sr, 0.8)
    add_sec(out, crash(sr, rng, 1.7), 2.2, sr, 0.55)
    add_sec(out, timpani(sr, 1.3, 65.4), 2.2, sr, 0.8)
    return comb_reverb(stereo(soft_clip(out, 1.3), sr, 0.25, 0.9), sr, wet=0.4, decay=0.78)


def render_fanfare_loss() -> np.ndarray:
    sr = SR_SFX
    out = zeros(2.7, sr)
    add_sec(out, pad_chord([C3, EB3, G3], 0.8, sr, 1400.0, 0.06), 0.0, sr, 0.9)
    add_sec(out, pad_chord([BB2, D3, F3], 0.8, sr, 1300.0, 0.06), 0.8, sr, 0.9)
    add_sec(out, pad_chord([AB2, C3, EB3, G3], 1.6, sr, 1100.0, 0.08), 1.6, sr, 1.0)
    t = t_axis(2.7, sr)
    drone = sine(C2, t, sr) * adsr(t, 2.7, 0.2, 0.3, 0.7, 0.6) * 0.5
    add_sec(out, timpani(sr, 1.4, 52.0), 1.6, sr, 0.7)
    return comb_reverb(stereo(out + drone, sr, 0.25, 0.9), sr, wet=0.45, decay=0.8)


def render_downtown() -> np.ndarray:
    """Three-ball sting: rising tone + whoosh into a sparkle."""
    sr = SR_SFX
    rng = rng_for(307)
    dur = 0.55
    t = t_axis(dur, sr)
    f = 440.0 * 2.0 ** (1.5 * ramp(t, 0.0, 0.3))
    tone = (saw(f, t, sr, 10) * 0.5 + sine(f * 2.0, t, sr) * 0.3) * adsr(t, dur, 0.02, 0.1, 0.6, 0.2)
    whoosh = sweep_noise(t, rng, sr, 600.0, 5000.0, 2.0) * hump(t, 0.0, 0.35, 1.5) * 0.7
    out = tone + whoosh
    for k, note in enumerate((C6, E6, G6)):
        add_sec(out, bell(note, 0.3, sr), 0.28 + k * 0.05, sr, 0.35)
    return comb_reverb(stereo(out, sr, 0.25, 0.8), sr, wet=0.3, decay=0.7)


def render_poster() -> np.ndarray:
    sr = SR_SFX
    rng = rng_for(308)
    out = zeros(0.6, sr)
    add_sec(out, _hit(sr, rng, 0.6), 0.0, sr, 1.0)
    add_sec(out, brass([C3, G3, C4], 0.35, sr), 0.02, sr, 0.5)
    t = t_axis(0.6, sr)
    shock = lp(white(t.shape[0], rng), 900.0, sr) * exp_env(t, 12.0) * 0.5
    return comb_reverb(stereo(soft_clip(out + shock, 1.4), sr, 0.2, 0.8), sr, wet=0.3, decay=0.7)


# ---------------------------------------------------------------------------
# UI
# ---------------------------------------------------------------------------


def render_blip() -> np.ndarray:
    sr = SR_SFX
    t = t_axis(0.08, sr)
    return fm(880.0, 440.0, 4.5, t, sr, exp_env(t, 42.0)) * exp_env(t, 42.0)


def render_confirm() -> np.ndarray:
    sr = SR_SFX
    out = zeros(0.2, sr)
    add_sec(out, fm(784.0, 392.0, 1.8, t_axis(0.09, sr), sr) * adsr(t_axis(0.09, sr), 0.09, 0.004, 0.02, 0.5, 0.03), 0.0, sr)
    add_sec(out, fm(1046.5, 523.0, 1.8, t_axis(0.12, sr), sr) * adsr(t_axis(0.12, sr), 0.12, 0.004, 0.03, 0.5, 0.05), 0.07, sr)
    return out


def render_pause() -> np.ndarray:
    sr = SR_SFX
    out = zeros(0.24, sr)
    add_sec(out, bell(C5, 0.2, sr), 0.0, sr, 0.8)
    add_sec(out, bell(G4, 0.2, sr), 0.1, sr, 0.8)
    return lp(out, 4000.0, sr)


def render_unpause() -> np.ndarray:
    sr = SR_SFX
    out = zeros(0.24, sr)
    add_sec(out, bell(G4, 0.2, sr), 0.0, sr, 0.8)
    add_sec(out, bell(C5, 0.2, sr), 0.1, sr, 0.8)
    return lp(out, 4000.0, sr)


# ---------------------------------------------------------------------------
# MUSIC
# ---------------------------------------------------------------------------


def _sidechain(n: int, sr: int, times_s: list[float], depth: float = 0.6, rel: float = 9.0) -> np.ndarray:
    t = np.arange(n) / sr
    env = np.ones(n)
    for at in times_s:
        env -= depth * exp_env(t, rel, at) * (t >= at)
    return np.clip(env, 0.05, 1.0)


def render_menu_synthwave() -> np.ndarray:
    """110 BPM, 4 bars, Cm – Ab – Eb – Bb. Pumping pad, octave bass, plucky arp, lead."""
    sr = SR_MUS
    rng = rng_for(401)
    bpm = 110.0
    beat = 60.0 / bpm
    step = beat / 4.0
    bars = 4
    n = n_of(bars * 4 * beat, sr)
    mix = np.zeros(n + n_of(2.0, sr))
    chords = [
        ([C3, EB3, G3, C4], C2),
        ([AB3, C4, EB4, AB3 * 0.5], AB2),
        ([EB3, G3, BB3, EB4], EB2),
        ([BB3, D4, F4, BB4 * 0.5], BB2),
    ]
    arp_sets = [[C4, EB4, G4, BB4], [AB4, C5, EB5, C5], [EB4, G4, BB4, D5], [BB4, D5, F5, D5]]
    kick_s = kick(sr, 0.36, 170.0, 46.0, 0.45)
    clap_s = clap(sr, rng, 0.25)
    snare_s = snare(sr, rng, 0.2, 185.0)
    hat_c = hat(sr, rng, 0.05, 7000.0, 75.0)
    hat_o = hat(sr, rng, 0.14, 5200.0, 22.0)
    kick_times = []
    for bar in range(bars):
        bar_t = bar * 4 * beat
        chord, bass_root = chords[bar]
        add_sec(mix, pad_chord(chord, 4 * beat * 1.02, sr, 1900.0, 0.08), bar_t, sr, 0.55)
        for st_i in range(16):
            at = bar_t + st_i * step
            if st_i % 4 == 0 or st_i in (6, 14):
                add_sec(mix, kick_s, at, sr, 1.0)
                kick_times.append(at)
            if st_i in (4, 12):
                add_sec(mix, clap_s, at, sr, 0.55)
                add_sec(mix, snare_s, at, sr, 0.45)
            add_sec(mix, hat_o if st_i == 14 else hat_c, at, sr, 0.28 if st_i % 2 else 0.18)
            # octave bass on 8ths
            if st_i % 2 == 0:
                f = bass_root if (st_i // 2) % 2 == 0 else bass_root * 2.0
                tt = t_axis(step * 1.9, sr)
                bass = (saw(f, tt, sr, 16) * 0.6 + square(f, tt, sr, 8) * 0.25) * adsr(tt, step * 1.9, 0.004, 0.06, 0.45, 0.05)
                add_sec(mix, lp(bass, 900.0, sr, 1.5), at, sr, 0.55)
            # 16th arp
            note = arp_sets[bar][st_i % 4]
            add_sec(mix, pluck(note, step * 1.1, sr, 0.55), at, sr, 0.42)
    # lead line on bars 3-4 with echo
    lead = np.zeros(mix.shape[0])
    melody = [(8.0, 1.0, G5), (9.0, 0.5, F5), (9.5, 0.5, EB5), (10.0, 1.0, F5), (11.0, 1.0, D5), (12.0, 2.0, EB5), (14.0, 1.0, BB4), (15.0, 1.0, C5)]
    for b, length, f in melody:
        tt = t_axis(length * beat * 0.95, sr)
        vib = 1.0 + 0.005 * np.sin(TWO_PI * 5.5 * tt) * ramp(tt, 0.1, 0.4)
        tone = (saw(f * vib, tt, sr, 14) * 0.5 + sine(f * vib, tt, sr) * 0.5) * adsr(tt, length * beat * 0.95, 0.02, 0.08, 0.7, 0.12)
        add_sec(lead, lp(tone, 3800.0, sr, 1.0), b * beat, sr, 0.32)
    lead = echo(lead, sr, beat * 0.75, 0.38, 3)
    add_at(mix, lead, 0)
    # sidechain pump on everything but the kick reads as synthwave
    mix *= _sidechain(mix.shape[0], sr, kick_times, 0.35, 10.0)
    for at in kick_times:
        add_sec(mix, kick_s, at, sr, 0.35)
    st = stereo(soft_clip(mix, 1.25), sr, 0.2, 0.55)
    st = comb_reverb(st, sr, wet=0.12, decay=0.6, damp_fc=4500.0)
    return wrap_tail(st, n)


def _ingame_grid():
    bpm = 96.0
    beat = 60.0 / bpm
    step = beat / 4.0
    bars = 4
    n = n_of(bars * 4 * beat, SR_MUS)
    return bpm, beat, step, bars, n


def _stem_buffer(n: int) -> np.ndarray:
    return np.zeros(n + n_of(1.5, SR_MUS))


def render_ingame_base() -> np.ndarray:
    """Stem 1 — always on: chip bass, off-beat stabs, light hats, shaker, sparkle arp."""
    sr = SR_MUS
    rng = rng_for(402)
    _, beat, step, bars, n = _ingame_grid()
    mix = _stem_buffer(n)
    riff = [C2, C2, EB2, C2, G2, F2, EB2, G2 * 0.5, C2, BB2 * 0.5, EB2, C2, G2, G2, EB2, C2]
    riff_b = [F2, F2, AB2, F2, C3, BB2, AB2, C2, F2, EB2, AB2, F2, C3, C3, AB2, F2]
    stabs = [[C4, EB4, G4], [F4, AB4, C5]]
    hat_c = hat(sr, rng, 0.045, 7200.0, 80.0)
    shk = shaker(sr, rng, 0.07)
    for bar in range(bars):
        bar_t = bar * 4 * beat
        seq = riff if bar < 2 else riff_b
        stab = stabs[0] if bar < 2 else stabs[1]
        for st_i in range(16):
            at = bar_t + st_i * step
            if st_i % 2 == 0:
                f = seq[(st_i // 2) % len(seq)]
                tt = t_axis(step * 1.7, sr)
                bass = (square(f, tt, sr, 12) * 0.55 + square(f * 2.0, tt, sr, 6) * 0.12 + sine(f, tt, sr) * 0.4) * adsr(
                    tt, step * 1.7, 0.003, 0.03, 0.35, 0.04
                )
                add_sec(mix, lp(bass, 1400.0, sr, 1.0), at, sr, 0.7)
            if st_i % 2 == 0:
                add_sec(mix, hat_c, at, sr, 0.16 if st_i % 4 else 0.1)
            if st_i % 2 == 1:
                add_sec(mix, shk, at, sr, 0.12)
            if st_i in (2, 7, 10, 15):
                add_sec(mix, pad_chord(stab, step * 1.4, sr, 2200.0, 0.005), at, sr, 0.28)
            if st_i % 4 == 0 and bar % 2 == 0:
                note = [C5, EB5, G5, BB5][(st_i // 4) % 4]
                add_sec(mix, pluck(note, step * 0.9, sr, 0.3), at, sr, 0.16)
    st = stereo(soft_clip(mix, 1.2), sr, 0.16, 0.5)
    return wrap_tail(st, n)


def render_ingame_drums() -> np.ndarray:
    """Stem 2 — hype layer: boom-bap kick/snare, claps, sub 808, crash on the top."""
    sr = SR_MUS
    rng = rng_for(403)
    _, beat, step, bars, n = _ingame_grid()
    mix = _stem_buffer(n)
    kick_s = kick(sr, 0.3, 150.0, 48.0, 0.5, 18.0)
    snare_s = snare(sr, rng, 0.18, 195.0)
    clap_s = clap(sr, rng, 0.22)
    hat_o = hat(sr, rng, 0.12, 5000.0, 24.0)
    for bar in range(bars):
        bar_t = bar * 4 * beat
        for st_i in range(16):
            at = bar_t + st_i * step
            if st_i in (0, 6) or (bar % 2 == 1 and st_i == 14) or (bar == 3 and st_i == 11):
                add_sec(mix, kick_s, at, sr, 1.05)
                tt = t_axis(0.3, sr)
                add_sec(mix, sine(C2 * 0.5, tt, sr) * exp_env(tt, 7.0), at, sr, 0.5)
            if st_i in (4, 12):
                add_sec(mix, snare_s, at, sr, 0.9)
                add_sec(mix, clap_s, at, sr, 0.5)
            if st_i == 14:
                add_sec(mix, hat_o, at, sr, 0.4)
            if bar == 3 and st_i in (13, 15):
                add_sec(mix, snare_s, at, sr, 0.45)
    add_sec(mix, crash(sr, rng, 1.4), 0.0, sr, 0.35)
    st = stereo(soft_clip(mix, 1.2), sr, 0.12, 0.4)
    return wrap_tail(st, n)


def render_ingame_rush() -> np.ndarray:
    """Stem 3 — last-minute layer: 16th arps, ride, riser, tom fill on bar 4."""
    sr = SR_MUS
    rng = rng_for(404)
    _, beat, step, bars, n = _ingame_grid()
    mix = _stem_buffer(n)
    ride_s = ride(sr, rng, 0.4)
    arps = [[C5, EB5, G5, C6, G5, EB5], [F5, AB5, C6, F5 * 2.0, C6, AB5]]
    for bar in range(bars):
        bar_t = bar * 4 * beat
        arp = arps[0] if bar < 2 else arps[1]
        for st_i in range(16):
            at = bar_t + st_i * step
            add_sec(mix, pluck(arp[st_i % len(arp)], step * 0.95, sr, 0.7), at, sr, 0.28)
            if st_i % 2 == 0:
                add_sec(mix, ride_s, at, sr, 0.22 if st_i % 4 else 0.3)
        if bar == 3:
            for k, st_i in enumerate((8, 9, 10, 11, 12, 13, 14, 15)):
                add_sec(mix, tom(sr, 0.2, 200.0 - k * 14.0), bar_t + st_i * step, sr, 0.45)
    t = np.arange(n) / sr
    riser = sweep_noise(t, rng, sr, 400.0, 9000.0, 1.6) * ramp(t, 0.0, t[-1]) ** 2.2 * 0.22
    riser *= 1.0 - ramp(t, t[-1] - 0.02, t[-1])
    add_at(mix, riser, 0)
    for bar in range(bars):
        bar_t = bar * 4 * beat
        for st_i in (0, 3, 6, 10, 12):
            add_sec(mix, brass([G4, BB4, D5] if bar < 2 else [C5, EB5, G5], step * 0.8, sr), bar_t + st_i * step, sr, 0.16)
    st = stereo(soft_clip(mix, 1.2), sr, 0.2, 0.6)
    return wrap_tail(st, n)


# ---------------------------------------------------------------------------
# Job list
# ---------------------------------------------------------------------------

SINGLE_JOBS = (
    # ball
    ("ball/catch.wav", render_catch, SR_SFX),
    ("ball/pass_whoosh.wav", render_pass_whoosh, SR_SFX),
    ("ball/shot_flick.wav", render_shot_flick, SR_SFX),
    ("ball/rim_front.wav", render_rim_front, SR_SFX),
    ("ball/rim_back.wav", render_rim_back, SR_SFX),
    ("ball/rim_soft.wav", render_rim_soft, SR_SFX),
    ("ball/backboard.wav", render_backboard, SR_SFX),
    ("ball/backboard_hard.wav", render_backboard_hard, SR_SFX),
    ("ball/swish.wav", render_swish, SR_SFX),
    ("ball/swish_soft.wav", render_swish_soft, SR_SFX),
    ("ball/rattle.wav", render_rattle, SR_SFX),
    ("ball/roll.wav", render_roll_loop, SR_SFX),
    # player
    ("player/jump_grunt.wav", render_jump_grunt, SR_SFX),
    ("player/land.wav", render_land, SR_SFX),
    ("player/dunk_boom.wav", render_dunk_boom, SR_SFX),
    ("player/block_slap.wav", render_block_slap, SR_SFX),
    ("player/steal_rip.wav", render_steal_rip, SR_SFX),
    ("player/body_thud.wav", render_body_thud, SR_SFX),
    # crowd
    ("crowd/bed_murmur.wav", render_bed_murmur, SR_SFX),
    ("crowd/bed_excited.wav", render_bed_excited, SR_SFX),
    ("crowd/bed_roar.wav", render_bed_roar, SR_SFX),
    ("crowd/cheer_small.wav", render_cheer_small, SR_SFX),
    ("crowd/cheer_big.wav", render_cheer_big, SR_SFX),
    ("crowd/cheer_huge.wav", render_cheer_huge, SR_SFX),
    ("crowd/oooh.wav", render_oooh, SR_SFX),
    ("crowd/gasp.wav", render_gasp, SR_SFX),
    ("crowd/groan.wav", render_groan, SR_SFX),
    ("crowd/boo.wav", render_boo, SR_SFX),
    ("crowd/anticipation.wav", render_anticipation, SR_SFX),
    ("crowd/stomp_clap.wav", render_stomp_clap, SR_SFX),
    ("crowd/chant.wav", render_chant, SR_SFX),
    ("crowd/airhorn.wav", render_airhorn, SR_SFX),
    ("crowd/whistles.wav", render_whistles, SR_SFX),
    # game / broadcast
    ("game/shot_tick.wav", render_shot_tick, SR_SFX),
    ("game/buzzer_long.wav", render_buzzer_long, SR_SFX),
    ("game/buzzer_short.wav", render_buzzer_short, SR_SFX),
    ("game/whistle_short.wav", render_whistle_short, SR_SFX),
    ("game/whistle_long.wav", render_whistle_long, SR_SFX),
    ("game/whistle_double.wav", render_whistle_double, SR_SFX),
    ("game/possession_chime.wav", render_possession_chime, SR_SFX),
    ("game/organ_charge.wav", render_organ_charge, SR_SFX),
    # stingers
    ("stingers/on_fire.wav", render_on_fire, SR_SFX),
    ("stingers/lead_change.wav", render_lead_change, SR_SFX),
    ("stingers/clutch.wav", render_clutch, SR_SFX),
    ("stingers/final_minute.wav", render_final_minute, SR_SFX),
    ("stingers/anthem.wav", render_anthem, SR_SFX),
    ("stingers/fanfare_win.wav", render_fanfare_win, SR_SFX),
    ("stingers/fanfare_loss.wav", render_fanfare_loss, SR_SFX),
    ("stingers/downtown.wav", render_downtown, SR_SFX),
    ("stingers/poster.wav", render_poster, SR_SFX),
    # ui
    ("ui/blip.wav", render_blip, SR_SFX),
    ("ui/confirm.wav", render_confirm, SR_SFX),
    ("ui/pause.wav", render_pause, SR_SFX),
    ("ui/unpause.wav", render_unpause, SR_SFX),
    # music
    ("music/menu_synthwave.wav", render_menu_synthwave, SR_MUS),
    ("music/ingame_base.wav", render_ingame_base, SR_MUS),
    ("music/ingame_drums.wav", render_ingame_drums, SR_MUS),
    ("music/ingame_rush.wav", render_ingame_rush, SR_MUS),
)

MULTI_JOBS = (render_bounces, render_dribbles, render_squeaks, render_steps)

LOOPS = {
    "ball/roll.wav",
    "crowd/bed_murmur.wav",
    "crowd/bed_excited.wav",
    "crowd/bed_roar.wav",
    "crowd/stomp_clap.wav",
    "crowd/chant.wav",
    "music/menu_synthwave.wav",
    "music/ingame_base.wav",
    "music/ingame_drums.wav",
    "music/ingame_rush.wav",
}


def all_jobs():
    for rel, fn, sr in SINGLE_JOBS:
        yield rel, (lambda fn=fn: fn()), sr
    for fn in MULTI_JOBS:
        for rel, data, sr in fn():
            yield rel, (lambda data=data: data), sr


def main() -> None:
    only = set(sys.argv[1:])
    print("FINNBALL audio bake — numpy DSP")
    print("  SFX 22050 Hz mono/stereo · music 32000 Hz stereo · 16-bit")
    written: set[str] = set()
    total = 0
    for rel, fn, sr in all_jobs():
        written.add(rel)
        if only and not any(rel.startswith(o) for o in only):
            continue
        path = os.path.join(AUDIO, rel)
        data = fn()
        size = write_wav(path, data, sr, is_loop=rel in LOOPS)
        total += size
        secs = data.shape[-1] / sr
        ch = "st" if data.ndim == 2 else "mo"
        print("  %-30s %5.2fs %s %6.1f KB" % (rel, secs, ch, size / 1024.0))
    if not only:
        for dirpath, _, files in os.walk(AUDIO):
            for f in files:
                if not f.endswith(".wav"):
                    continue
                rel = os.path.relpath(os.path.join(dirpath, f), AUDIO).replace(os.sep, "/")
                if rel not in written:
                    os.remove(os.path.join(dirpath, f))
                    print("  removed stale", rel)
    grand = 0
    for dirpath, _, files in os.walk(AUDIO):
        for f in files:
            grand += os.path.getsize(os.path.join(dirpath, f))
    print("pack total  %.2f MB (%d files)" % (grand / (1024.0 * 1024.0), len(written)))
    if grand > BUDGET_MB * 1024 * 1024:
        raise SystemExit("audio pack exceeds %.0f MB budget" % BUDGET_MB)


if __name__ == "__main__":
    main()
