#!/usr/bin/env python3
"""Generate original FINNBALL audio assets.

16-bit stereo 44100 Hz WAV files, Python stdlib only
(wave, math, struct, array, os). No numpy.
"""

from __future__ import annotations

import array
import math
import os
import struct
import wave

SR = 44100
PEAK = 0.70
TWO_PI = math.pi * 2.0
NYQUIST = SR * 0.45

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
AUDIO = os.path.join(ROOT, "assets", "audio")


# ---------------------------------------------------------------------------
# PRNG / buffers
# ---------------------------------------------------------------------------


class Rng:
    """Numerical Recipes LCG — deterministic noise, no `random` module."""

    def __init__(self, seed: int) -> None:
        self.s = seed & 0xFFFFFFFF

    def u32(self) -> int:
        self.s = (1664525 * self.s + 1013904223) & 0xFFFFFFFF
        return self.s

    def uniform(self, a: float = -1.0, b: float = 1.0) -> float:
        return a + (self.u32() / 4294967295.0) * (b - a)


def zeros(n: int) -> list[float]:
    return [0.0] * n


def n_samples(seconds: float) -> int:
    return max(1, int(round(seconds * SR)))


# ---------------------------------------------------------------------------
# Oscillators (band-limited additive — less aliasing than naive square)
# ---------------------------------------------------------------------------


def sine(freq: float, t: float, phase: float = 0.0) -> float:
    return math.sin(TWO_PI * freq * t + phase)


def bl_square(freq: float, t: float, phase: float = 0.0) -> float:
    if freq <= 0.0:
        return 0.0
    acc = 0.0
    h = 1
    while freq * h < NYQUIST and h <= 15:
        acc += math.sin(TWO_PI * freq * h * t + phase) / h
        h += 2
    return acc * (4.0 / math.pi)


def bl_triangle(freq: float, t: float, phase: float = 0.0) -> float:
    if freq <= 0.0:
        return 0.0
    acc = 0.0
    k = 0
    h = 1
    while freq * h < NYQUIST and k < 8:
        sign = -1.0 if (k & 1) else 1.0
        acc += sign * math.sin(TWO_PI * freq * h * t + phase) / (h * h)
        k += 1
        h += 2
    return acc * (8.0 / (math.pi * math.pi))


# ---------------------------------------------------------------------------
# Envelopes / filters / mix
# ---------------------------------------------------------------------------


def exp_decay(t: float, rate: float) -> float:
    if t < 0.0:
        return 0.0
    return math.exp(-t * rate)


def adsr(t: float, dur: float, a: float, d: float, s: float, r: float) -> float:
    if t < 0.0 or t > dur:
        return 0.0
    if t < a:
        return t / a if a > 0.0 else 1.0
    if t < a + d:
        return 1.0 - (1.0 - s) * ((t - a) / d if d > 0.0 else 1.0)
    rel_start = dur - r
    if t < rel_start:
        return s
    if r <= 0.0:
        return 0.0
    return s * max(0.0, 1.0 - (t - rel_start) / r)


def one_pole_lp(src: list[float], cutoff: float) -> list[float]:
    x = math.exp(-TWO_PI * cutoff / SR)
    a0 = 1.0 - x
    y = 0.0
    out = zeros(len(src))
    for i, s in enumerate(src):
        y = a0 * s + x * y
        out[i] = y
    return out


def one_pole_hp(src: list[float], cutoff: float) -> list[float]:
    x = math.exp(-TWO_PI * cutoff / SR)
    a0 = (1.0 + x) * 0.5
    y = 0.0
    prev = 0.0
    out = zeros(len(src))
    for i, s in enumerate(src):
        y = a0 * (s - prev) + x * y
        prev = s
        out[i] = y
    return out


def band_limit_noise(rng: Rng, n: int, hp: float, lp: float) -> list[float]:
    raw = [rng.uniform(-1.0, 1.0) for _ in range(n)]
    return one_pole_lp(one_pole_hp(raw, hp), lp)


def pinkish(rng: Rng, n: int, taps: int = 6) -> list[float]:
    """Average successive white samples for a cheap pink-ish tilt."""
    white = [rng.uniform(-1.0, 1.0) for _ in range(n + taps)]
    out = zeros(n)
    inv = 1.0 / taps
    acc = 0.0
    for i in range(taps):
        acc += white[i]
    for i in range(n):
        out[i] = acc * inv
        acc += white[i + taps] - white[i]
    return out


def add_at(dst: list[float], src: list[float], at: int, gain: float = 1.0) -> None:
    n = len(dst)
    for i, s in enumerate(src):
        j = at + i
        if 0 <= j < n:
            dst[j] += s * gain


def mix_into(dst: list[float], src: list[float], gain: float = 1.0) -> None:
    for i, s in enumerate(src):
        if i < len(dst):
            dst[i] += s * gain


def scale(buf: list[float], g: float) -> list[float]:
    return [s * g for s in buf]


def apply_loop_fade(buf: list[float], fade_s: float = 0.080) -> None:
    n = min(len(buf) // 2, int(round(fade_s * SR)))
    if n <= 0:
        return
    for i in range(n):
        g = i / n
        buf[i] *= g
        buf[len(buf) - 1 - i] *= g


def peak_abs(bufs: list[list[float]]) -> float:
    p = 0.0
    for b in bufs:
        for s in b:
            a = s if s >= 0.0 else -s
            if a > p:
                p = a
    return p


def normalize_stereo(left: list[float], right: list[float], peak: float = PEAK) -> None:
    p = peak_abs([left, right])
    if p <= 1e-9:
        return
    g = peak / p
    for i in range(len(left)):
        left[i] *= g
    for i in range(len(right)):
        right[i] *= g


def stereo_from_mono(mono: list[float], width: float = 0.12, delay: int = 7) -> tuple[list[float], list[float]]:
    """Slight delay + opposite-side bleed for width without collapsing to mono."""
    n = len(mono)
    left = zeros(n)
    right = zeros(n)
    w = max(0.0, min(0.45, width))
    for i, s in enumerate(mono):
        j = i - delay
        d = mono[j] if j >= 0 else 0.0
        left[i] = s * (1.0 - w) + d * w
        right[i] = s * (1.0 - w * 0.35) + d * (w * 0.65)
    return left, right


def am_modulate(buf: list[float], *lfos: tuple[float, float]) -> list[float]:
    """Multiply by (1 + sum(depth * sin(2π f t))) — depths should keep gain > 0."""
    out = zeros(len(buf))
    for i, s in enumerate(buf):
        t = i / SR
        m = 1.0
        for freq, depth in lfos:
            m += depth * math.sin(TWO_PI * freq * t)
        out[i] = s * max(0.05, m)
    return out


def formant_burst(
    n: int,
    rng: Rng,
    formants: list[tuple[float, float]],
    env: list[float] | None = None,
) -> list[float]:
    """Noise through parallel resonant-ish bandpasses (vowel-like, not speech)."""
    noise = [rng.uniform(-1.0, 1.0) for _ in range(n)]
    acc = zeros(n)
    for center, gain in formants:
        hp = max(80.0, center * 0.55)
        lp = min(NYQUIST * 0.95, center * 1.7)
        band = one_pole_lp(one_pole_hp(noise, hp), lp)
        mix_into(acc, band, gain)
    if env is not None:
        for i in range(n):
            acc[i] *= env[i]
    return acc


# ---------------------------------------------------------------------------
# WAV I/O
# ---------------------------------------------------------------------------


def write_wav(path: str, left: list[float], right: list[float]) -> int:
    os.makedirs(os.path.dirname(path), exist_ok=True)
    n = min(len(left), len(right))
    # Soft-clip kicks/clangs so body level survives the 0.7 peak normalize.
    for buf in (left, right):
        for i, s in enumerate(buf):
            buf[i] = math.tanh(s * 1.35)
    normalize_stereo(left, right, PEAK)
    frames = array.array("h")
    scale_i = 32767.0
    raw = bytearray()
    for i in range(n):
        l = left[i]
        r = right[i]
        if l > 1.0:
            l = 1.0
        elif l < -1.0:
            l = -1.0
        if r > 1.0:
            r = 1.0
        elif r < -1.0:
            r = -1.0
        li = int(l * scale_i)
        ri = int(r * scale_i)
        frames.append(li)
        frames.append(ri)
        raw += struct.pack("<hh", li, ri)
    payload = frames.tobytes()
    if payload != bytes(raw):
        raise RuntimeError("pcm pack mismatch")
    with wave.open(path, "w") as wf:
        wf.setnchannels(2)
        wf.setsampwidth(2)
        wf.setframerate(SR)
        wf.writeframes(payload)
    return os.path.getsize(path)


# ---------------------------------------------------------------------------
# Drum / instrument recipes
# ---------------------------------------------------------------------------


def synth_kick_808(dur: float = 0.32, start_hz: float = 170.0, end_hz: float = 48.0) -> list[float]:
    n = n_samples(dur)
    out = zeros(n)
    phase = 0.0
    click_n = n_samples(0.008)
    for i in range(n):
        t = i / SR
        # exponential pitch sweep
        env_p = math.exp(-t * 22.0)
        freq = end_hz + (start_hz - end_hz) * env_p
        phase += TWO_PI * freq / SR
        body = math.sin(phase) * math.exp(-t * 7.5)
        click = 0.0
        if i < click_n:
            click = math.sin(TWO_PI * 2100.0 * t) * (1.0 - i / click_n) * 0.45
        out[i] = body + click
    return out


def synth_hat(dur: float = 0.055, rng: Rng | None = None, bright: float = 6000.0) -> list[float]:
    rng = rng or Rng(7)
    n = n_samples(dur)
    raw = [rng.uniform(-1.0, 1.0) for _ in range(n)]
    hp = one_pole_hp(raw, bright)
    for i in range(n):
        t = i / SR
        hp[i] *= math.exp(-t * 68.0)
    return hp


def synth_snare(dur: float = 0.18, rng: Rng | None = None) -> list[float]:
    rng = rng or Rng(11)
    n = n_samples(dur)
    out = zeros(n)
    noise = one_pole_hp([rng.uniform(-1.0, 1.0) for _ in range(n)], 1200.0)
    for i in range(n):
        t = i / SR
        tone = sine(190.0, t) * 0.45 + sine(330.0, t) * 0.18
        body = tone * math.exp(-t * 18.0)
        air = noise[i] * math.exp(-t * 14.0) * 0.85
        out[i] = body + air
    return out


def synth_rim_clang(dur: float = 0.22) -> list[float]:
    n = n_samples(dur)
    out = zeros(n)
    partials = (800.0, 1200.0, 1800.0, 2400.0)
    amps = (1.0, 0.72, 0.48, 0.32)
    decays = (18.0, 22.0, 28.0, 34.0)
    for i in range(n):
        t = i / SR
        s = 0.0
        for f, a, d in zip(partials, amps, decays):
            s += a * sine(f, t) * math.exp(-t * d)
        out[i] = s
    return out


def synth_bounce(dur: float, hz: float, decay: float, click: float = 0.35) -> list[float]:
    n = n_samples(dur)
    out = zeros(n)
    for i in range(n):
        t = i / SR
        body = sine(hz, t) * math.exp(-t * decay)
        clk = sine(2400.0, t) * math.exp(-t * 90.0) * click
        out[i] = body + clk
    return out


# ---------------------------------------------------------------------------
# MUSIC
# ---------------------------------------------------------------------------

# C minor arp
C4, EB4, G4, BB4 = 261.63, 311.13, 392.00, 466.16
C2, EB2, G2, BB2, C3, G1 = 65.41, 77.78, 98.00, 116.54, 130.81, 49.00


def _step_time(bpm: float, steps_per_beat: int = 4) -> float:
    return (60.0 / bpm) / steps_per_beat


def render_menu_synthwave() -> tuple[list[float], list[float]]:
    """~8s, 110 BPM, C minor square+triangle arp, 808 on 1/3, hats offbeat."""
    bpm = 110.0
    bars = 3
    beats = bars * 4
    step = _step_time(bpm, 4)  # 16th
    n = n_samples(beats * 60.0 / bpm)
    mix = zeros(n)

    arp = (C4, EB4, G4, BB4)
    # two-bar bass: C / G
    bass_notes = (C2, C2, C2, C2, G1, G1, EB2, G1)

    kick = synth_kick_808(0.30)
    hat = synth_hat(0.048, Rng(110), 6500.0)

    for st in range(beats * 4):
        at = int(round(st * step * SR))
        # 16th arp
        note = arp[st % 4]
        length = n_samples(step * 1.05)
        osc = zeros(length)
        for i in range(length):
            t = i / SR
            e = adsr(t, length / SR, 0.004, 0.03, 0.35, 0.04)
            sq = bl_square(note, t) * 0.38
            tr = bl_triangle(note * 0.5, t) * 0.28
            osc[i] = (sq + tr) * e
        add_at(mix, osc, at, 1.0)

        # pad ghost on downbeats
        if st % 4 == 0:
            pad_n = n_samples(step * 3.6)
            pad = zeros(pad_n)
            for i in range(pad_n):
                t = i / SR
                e = adsr(t, pad_n / SR, 0.02, 0.08, 0.45, 0.12)
                pad[i] = (
                    bl_triangle(C3, t)
                    + bl_triangle(EB4 * 0.5, t) * 0.7
                    + bl_triangle(G4 * 0.5, t) * 0.5
                ) * e * 0.16
            add_at(mix, pad, at, 1.0)

        # kick on beats 1 and 3 (16th steps 0 and 8 of each bar)
        if st % 16 in (0, 8):
            add_at(mix, kick, at, 1.05)

        # hats on offbeats (the '&' of each beat → 16th steps 2,6,10,14)
        if st % 4 == 2:
            add_at(mix, hat, at, 0.55)

        # chip-adjacent bass on 8ths
        if st % 2 == 0:
            bn = bass_notes[(st // 2) % len(bass_notes)]
            bn_n = n_samples(step * 1.8)
            bass = zeros(bn_n)
            for i in range(bn_n):
                t = i / SR
                e = adsr(t, bn_n / SR, 0.006, 0.04, 0.4, 0.05)
                bass[i] = bl_square(bn, t) * 0.22 * e + bl_triangle(bn, t) * 0.18 * e
            add_at(mix, bass, at, 1.0)

    apply_loop_fade(mix, 0.080)
    return stereo_from_mono(mix, width=0.16, delay=9)


def render_ingame_arcade() -> tuple[list[float], list[float]]:
    """~8s, 95 BPM boom-bap kick/snare + chip bass."""
    bpm = 95.0
    bars = 3
    beats = bars * 4
    step = _step_time(bpm, 4)
    n = n_samples(beats * 60.0 / bpm)
    mix = zeros(n)

    kick = synth_kick_808(0.28, start_hz=150.0, end_hz=50.0)
    snare = synth_snare(0.16, Rng(95))
    hat_closed = synth_hat(0.040, Rng(96), 7000.0)
    hat_open = synth_hat(0.11, Rng(97), 5200.0)

    # chip bass riff in C minor (8th notes, 2-bar cell)
    f2 = 87.31
    bass_riff = (C2, C2, EB2, C2, G2, f2, EB2, G1, C2, BB2 * 0.5, EB2, C2, G2, G2, EB2, C2)

    for st in range(beats * 4):
        at = int(round(st * step * SR))
        bar_st = st % 16

        # boom-bap: kick on 1 and the '&' of 2; extra kick on 4e of bars 2/3
        if bar_st in (0, 6) or (st >= 16 and bar_st == 14):
            add_at(mix, kick, at, 1.1)
        # snare on 2 and 4
        if bar_st in (4, 12):
            add_at(mix, snare, at, 0.95)
        # hats: 8ths, slightly louder on offbeats
        if st % 2 == 0:
            h = hat_open if bar_st == 14 else hat_closed
            add_at(mix, h, at, 0.42 if st % 4 == 2 else 0.28)

        # chip bass on 8ths
        if st % 2 == 0:
            note = bass_riff[(st // 2) % len(bass_riff)]
            length = n_samples(step * 1.7)
            bass = zeros(length)
            for i in range(length):
                t = i / SR
                e = adsr(t, length / SR, 0.003, 0.025, 0.3, 0.04)
                # pulse-ish: square + a touch of octave
                bass[i] = (bl_square(note, t) * 0.55 + bl_square(note * 2.0, t) * 0.12) * e
            add_at(mix, bass, at, 0.85)

        # tiny arp sparkle every other bar
        if st % 32 < 16 and st % 4 == 0:
            spark_n = n_samples(step * 0.95)
            spark = zeros(spark_n)
            note = (C4, EB4, G4, BB4)[(st // 4) % 4]
            for i in range(spark_n):
                t = i / SR
                spark[i] = bl_triangle(note, t) * adsr(t, spark_n / SR, 0.002, 0.02, 0.2, 0.03) * 0.18
            add_at(mix, spark, at, 1.0)

    apply_loop_fade(mix, 0.080)
    return stereo_from_mono(mix, width=0.14, delay=8)


# ---------------------------------------------------------------------------
# CROWD
# ---------------------------------------------------------------------------


def render_crowd_bed() -> tuple[list[float], list[float]]:
    """4s loop, pink-ish averaged noise, AM murmur."""
    n = n_samples(4.0)
    left_src = pinkish(Rng(2024), n, taps=8)
    right_src = pinkish(Rng(909), n, taps=8)
    # extra averaging pass + mild lowpass for arena muffling
    left = one_pole_lp(pinkish(Rng(3), n, taps=4), 1800.0)
    right = one_pole_lp(pinkish(Rng(5), n, taps=4), 1900.0)
    mix_into(left, left_src, 0.65)
    mix_into(right, right_src, 0.65)
    left = am_modulate(left, (0.7, 0.22), (1.9, 0.16), (3.4, 0.08))
    right = am_modulate(right, (0.55, 0.20), (2.3, 0.14), (4.1, 0.07))
    apply_loop_fade(left, 0.080)
    apply_loop_fade(right, 0.080)
    return left, right


def render_cheer() -> tuple[list[float], list[float]]:
    """1.2s swell — rising crowd, formant-ish, not voices."""
    n = n_samples(1.2)
    env = zeros(n)
    for i in range(n):
        t = i / SR
        # swell to ~0.55s, hold, gentle drop
        if t < 0.55:
            env[i] = (t / 0.55) ** 1.15
        else:
            env[i] = 1.0 - 0.25 * ((t - 0.55) / 0.65)
    rng_l = Rng(42)
    rng_r = Rng(84)
    formants = [(420.0, 0.7), (780.0, 0.9), (1250.0, 0.55), (1900.0, 0.3)]
    left = formant_burst(n, rng_l, formants, env)
    right = formant_burst(n, rng_r, [(400.0, 0.65), (820.0, 0.85), (1320.0, 0.5), (2100.0, 0.28)], env)
    left = am_modulate(left, (6.5, 0.12), (11.0, 0.07))
    right = am_modulate(right, (7.2, 0.11), (13.0, 0.06))
    return left, right


def render_gasp() -> tuple[list[float], list[float]]:
    """0.7s inhale-like gasp."""
    n = n_samples(0.7)
    rng = Rng(13)
    noise = band_limit_noise(rng, n, hp=900.0, lp=6500.0)
    out = zeros(n)
    for i in range(n):
        t = i / SR
        # fast rise, peak ~0.12s, then fall
        if t < 0.08:
            e = t / 0.08
        elif t < 0.16:
            e = 1.0
        else:
            e = math.exp(-(t - 0.16) * 7.0)
        # slight downward brightness sweep via extra HP leftover
        out[i] = noise[i] * e
    out = one_pole_hp(out, 700.0)
    return stereo_from_mono(out, width=0.22, delay=11)


# ---------------------------------------------------------------------------
# BALL / PLAYER / GAME / UI / STINGERS
# ---------------------------------------------------------------------------


def render_swish() -> tuple[list[float], list[float]]:
    """Band-limited noise * exp(-t*12), highpassed."""
    n = n_samples(0.30)
    rng = Rng(21)
    noise = band_limit_noise(rng, n, hp=1800.0, lp=9000.0)
    out = zeros(n)
    for i in range(n):
        t = i / SR
        out[i] = noise[i] * math.exp(-t * 12.0)
    out = one_pole_hp(out, 1600.0)
    return stereo_from_mono(out, width=0.18, delay=5)


def render_rim() -> tuple[list[float], list[float]]:
    clang = synth_rim_clang(0.24)
    return stereo_from_mono(clang, width=0.10, delay=4)


def render_backboard() -> tuple[list[float], list[float]]:
    n = n_samples(0.22)
    rng = Rng(33)
    out = zeros(n)
    noise = one_pole_lp([rng.uniform(-1.0, 1.0) for _ in range(n)], 2400.0)
    for i in range(n):
        t = i / SR
        thud = (sine(210.0, t) + sine(340.0, t) * 0.55 + sine(95.0, t) * 0.4) * math.exp(-t * 16.0)
        wood = noise[i] * math.exp(-t * 22.0) * 0.55
        out[i] = thud + wood
    return stereo_from_mono(out, width=0.08, delay=3)


def render_bounce() -> tuple[list[float], list[float]]:
    return stereo_from_mono(synth_bounce(0.22, 140.0, 20.0, click=0.40), width=0.06, delay=2)


def render_dribble() -> tuple[list[float], list[float]]:
    return stereo_from_mono(synth_bounce(0.12, 190.0, 28.0, click=0.50), width=0.05, delay=2)


def render_dunk() -> tuple[list[float], list[float]]:
    n = n_samples(0.30)
    out = zeros(n)
    clang = synth_rim_clang(0.22)
    for i in range(n):
        t = i / SR
        sub = sine(60.0, t) * math.exp(-t * 9.0) + sine(42.0, t) * math.exp(-t * 7.0) * 0.7
        out[i] = sub * 1.15
    add_at(out, clang, int(0.02 * SR), 0.85)
    # floor thud
    thud = synth_kick_808(0.22, start_hz=110.0, end_hz=40.0)
    add_at(out, thud, 0, 0.55)
    return stereo_from_mono(out, width=0.12, delay=6)


def render_squeak() -> tuple[list[float], list[float]]:
    n = n_samples(0.11)
    out = zeros(n)
    for i in range(n):
        t = i / SR
        # rubber chirp 2.4k → 1.3k
        freq = 2400.0 - 1100.0 * (t / 0.11)
        e = adsr(t, 0.11, 0.004, 0.03, 0.25, 0.04)
        out[i] = sine(freq, t) * e + sine(freq * 1.03, t) * e * 0.35
    return stereo_from_mono(out, width=0.20, delay=4)


def render_whistle() -> tuple[list[float], list[float]]:
    n = n_samples(0.28)
    out = zeros(n)
    for i in range(n):
        t = i / SR
        e = adsr(t, 0.28, 0.02, 0.04, 0.85, 0.05)
        fund = sine(2800.0, t)
        harm = sine(5600.0, t) * 0.28
        # tiny vibrato
        vib = sine(2800.0 + 18.0 * math.sin(TWO_PI * 12.0 * t), t) * 0.15
        out[i] = (fund + harm + vib) * e
    return stereo_from_mono(out, width=0.08, delay=3)


def render_buzzer() -> tuple[list[float], list[float]]:
    n = n_samples(0.90)
    out = zeros(n)
    for i in range(n):
        t = i / SR
        e = adsr(t, 0.90, 0.008, 0.02, 0.92, 0.04)
        out[i] = (bl_square(440.0, t) * 0.55 + bl_square(880.0, t) * 0.40) * e
    return stereo_from_mono(out, width=0.04, delay=2)


def render_blip() -> tuple[list[float], list[float]]:
    """Short FM click."""
    n = n_samples(0.07)
    out = zeros(n)
    fc, fm, idx = 880.0, 440.0, 4.5
    for i in range(n):
        t = i / SR
        e = math.exp(-t * 42.0)
        mod = math.sin(TWO_PI * fm * t)
        out[i] = math.sin(TWO_PI * fc * t + idx * mod * e) * e
    return stereo_from_mono(out, width=0.05, delay=2)


def render_confirm() -> tuple[list[float], list[float]]:
    n = n_samples(0.16)
    out = zeros(n)
    # two-note up: G5 then C6, light FM
    for i in range(n):
        t = i / SR
        freq = 784.0 if t < 0.07 else 1046.5
        e = adsr(t, 0.16, 0.004, 0.03, 0.4, 0.05)
        mod = math.sin(TWO_PI * freq * 0.5 * t)
        out[i] = math.sin(TWO_PI * freq * t + 1.8 * mod) * e
    return stereo_from_mono(out, width=0.06, delay=2)


def _stinger_env(n: int, attack: float, hold: float) -> list[float]:
    env = zeros(n)
    dur = n / SR
    for i in range(n):
        t = i / SR
        env[i] = adsr(t, dur, attack, 0.06, 0.75, max(0.04, dur - attack - hold))
    return env


def render_downtown() -> tuple[list[float], list[float]]:
    """Rising formant-ish burst — logo-three sting, not speech."""
    n = n_samples(0.42)
    env = _stinger_env(n, 0.035, 0.16)
    # rising centers
    left = zeros(n)
    right = zeros(n)
    rng = Rng(77)
    noise = [rng.uniform(-1.0, 1.0) for _ in range(n)]
    y1 = y2 = 0.0
    for i in range(n):
        t = i / SR
        climb = t / 0.42
        c1 = 480.0 + 520.0 * climb
        c2 = 1100.0 + 800.0 * climb
        c3 = 1900.0 + 600.0 * climb
        # cheap parallel bands via one-pole cascade on the fly
        # tone stack + noise
        tone = (
            sine(c1, t) * 0.45
            + sine(c2, t) * 0.32
            + sine(c3, t) * 0.18
            + bl_triangle(c1 * 0.5, t) * 0.22
        )
        left[i] = (tone + noise[i] * 0.22) * env[i]
    # band-limit the sting so it isn't harsh
    left = one_pole_lp(one_pole_hp(left, 180.0), 4200.0)
    # rising whoosh noise layer
    whoosh = formant_burst(
        n,
        Rng(78),
        [(600.0, 0.5), (1400.0, 0.7), (2400.0, 0.35)],
        env,
    )
    mix_into(left, whoosh, 0.55)
    for i in range(n):
        right[i] = left[i]
        if i >= 6:
            right[i] = left[i] * 0.88 + left[i - 6] * 0.12
    return left, right


def render_poster() -> tuple[list[float], list[float]]:
    """Punchy formant slam — poster dunk sting, not speech."""
    n = n_samples(0.40)
    env = zeros(n)
    for i in range(n):
        t = i / SR
        env[i] = adsr(t, 0.40, 0.008, 0.07, 0.55, 0.14)
    body = formant_burst(
        n,
        Rng(55),
        [(280.0, 0.9), (650.0, 0.7), (1200.0, 0.4), (2100.0, 0.22)],
        env,
    )
    clang = synth_rim_clang(0.20)
    sub = zeros(n)
    for i in range(n):
        t = i / SR
        sub[i] = sine(55.0, t) * math.exp(-t * 8.0) * 0.9
    mix_into(body, sub, 1.0)
    add_at(body, clang, int(0.015 * SR), 0.7)
    # mid punch
    punch = synth_kick_808(0.18, start_hz=90.0, end_hz=46.0)
    add_at(body, punch, 0, 0.65)
    return stereo_from_mono(body, width=0.15, delay=7)


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------


JOBS = (
    ("music/menu_synthwave.wav", render_menu_synthwave),
    ("music/ingame_arcade.wav", render_ingame_arcade),
    ("crowd/bed.wav", render_crowd_bed),
    ("crowd/cheer.wav", render_cheer),
    ("crowd/gasp.wav", render_gasp),
    ("ball/swish.wav", render_swish),
    ("ball/rim.wav", render_rim),
    ("ball/backboard.wav", render_backboard),
    ("ball/bounce.wav", render_bounce),
    ("ball/dribble.wav", render_dribble),
    ("ball/dunk.wav", render_dunk),
    ("player/squeak.wav", render_squeak),
    ("game/whistle.wav", render_whistle),
    ("game/buzzer.wav", render_buzzer),
    ("ui/blip.wav", render_blip),
    ("ui/confirm.wav", render_confirm),
    ("stingers/downtown.wav", render_downtown),
    ("stingers/poster.wav", render_poster),
)


def main() -> None:
    print("FINNBALL audio bake — 16-bit stereo 44100 Hz")
    total = 0
    for rel, fn in JOBS:
        path = os.path.join(AUDIO, rel)
        print("  rendering", rel, "...")
        left, right = fn()
        size = write_wav(path, left, right)
        total += size
        print("    %s  %6.1f KB" % (path, size / 1024.0))
    print("total  %.2f KB  (%.2f MB)" % (total / 1024.0, total / (1024.0 * 1024.0)))
    if total > 4 * 1024 * 1024:
        raise SystemExit("audio pack exceeds 4 MB budget")


if __name__ == "__main__":
    main()
