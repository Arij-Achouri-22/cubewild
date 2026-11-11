//! Sound. Every effect is synthesised in code at start-up (there are no sound files) and
//! mixed on the audio device's own thread, so sound costs the render loop nothing but a
//! channel send. Without a sound device the game runs silent.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use glam::Vec3;
use std::f32::consts::TAU;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

/// Every sound effect. Steps and digs come in one flavour per `Surface`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Snd {
    Step(Surface),
    Dig(Surface),
    Break(Surface),
    Place(Surface),
    Pop,
    Hit,
    Hurt,
    Deer,
    Boar,
    Hare,
    Wolf,
    Zombie,
    Skeleton,
    Spider,
    Slime,
    Villager,
    MobHurt,
    Splash,
    Swim,
    Bow,
    ArrowHit,
    Eat,
    Door,
    Chest,
    Click,
    Xp,
    LevelUp,
    Thunder,
    Fizz,
    Crackle,
}

/// What a block sounds like under a foot or a pick.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Surface {
    Grass,
    Stone,
    Wood,
    Sand,
    Snow,
    Glass,
    Cloth,
}
const SURFACES: usize = 7;

/// the looping backgrounds: rain, cave wind, under water
const BEDS: usize = 3;

enum Cmd {
    Play { buf: Arc<[f32]>, l: f32, r: f32, rate: f32 },
    Bed([f32; BEDS]),
    Volume { master: f32, music: f32 },
}

struct Bank {
    step: [Arc<[f32]>; SURFACES],
    dig: [Arc<[f32]>; SURFACES],
    brk: [Arc<[f32]>; SURFACES],
    place: [Arc<[f32]>; SURFACES],
    other: Vec<(Snd, Arc<[f32]>)>,
}

impl Bank {
    fn get(&self, s: Snd) -> Option<&Arc<[f32]>> {
        match s {
            Snd::Step(m) => Some(&self.step[m as usize]),
            Snd::Dig(m) => Some(&self.dig[m as usize]),
            Snd::Break(m) => Some(&self.brk[m as usize]),
            Snd::Place(m) => Some(&self.place[m as usize]),
            s => self.other.iter().find(|o| o.0 == s).map(|o| &o.1),
        }
    }
}

pub struct Audio {
    tx: Option<Sender<Cmd>>,
    bank: Option<Bank>,
    /// keeps the device stream alive
    _stream: Option<cpal::Stream>,
    rng: u32,
    /// listener: ear position and the direction to its right
    ear: Vec3,
    right: Vec3,
    pub master: f32,
    pub music: f32,
}

impl Audio {
    /// Opens the default output device. Never fails: without one the game is silent.
    pub fn new(master: f32, music: f32) -> Audio {
        let mut a = Audio { tx: None, bank: None, _stream: None, rng: 0x9e37_79b9, ear: Vec3::ZERO, right: Vec3::X, master, music };
        match open() {
            Ok((stream, tx, rate)) => {
                let t0 = std::time::Instant::now();
                a.bank = Some(build_bank(rate));
                eprintln!("audio: {rate} Hz, sounds made in {:.0} ms", t0.elapsed().as_secs_f32() * 1000.0);
                let _ = tx.send(Cmd::Volume { master, music });
                a.tx = Some(tx);
                a._stream = Some(stream);
            }
            Err(e) => eprintln!("audio: no sound ({e})"),
        }
        a
    }

    pub fn set_volume(&mut self, master: f32, music: f32) {
        self.master = master;
        self.music = music;
        if let Some(tx) = &self.tx {
            let _ = tx.send(Cmd::Volume { master, music });
        }
    }

    /// Where the player hears from, and which way they face.
    pub fn listen(&mut self, ear: Vec3, yaw: f32) {
        self.ear = ear;
        let (s, c) = yaw.sin_cos();
        self.right = Vec3::new(-c, 0.0, s);
    }

    fn rand(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        (self.rng >> 8) as f32 / (1u32 << 24) as f32
    }

    /// A sound with no place in the world (the player's own, and the menus').
    pub fn play(&mut self, s: Snd, vol: f32, pitch: f32) {
        self.send(s, vol, vol, pitch);
    }

    /// A sound at a place in the world: quieter with distance, panned left or right.
    pub fn play_at(&mut self, s: Snd, at: Vec3, vol: f32, pitch: f32) {
        let d = at - self.ear;
        let dist = d.length();
        let range = 28.0;
        if dist >= range || self.tx.is_none() {
            return;
        }
        let fall = (1.0 - dist / range).powi(2);
        let pan = if dist > 0.01 { (d / dist).dot(self.right) * 0.6 } else { 0.0 };
        self.send(s, vol * fall * (1.0 - pan).min(1.0), vol * fall * (1.0 + pan).min(1.0), pitch);
    }

    /// `play_at` with a little random pitch, so repeats do not sound mechanical.
    pub fn play_var(&mut self, s: Snd, at: Vec3, vol: f32) {
        let p = 0.9 + self.rand() * 0.2;
        self.play_at(s, at, vol, p);
    }

    fn send(&mut self, s: Snd, l: f32, r: f32, pitch: f32) {
        let (Some(tx), Some(bank)) = (&self.tx, &self.bank) else { return };
        if l.max(r) < 0.004 {
            return;
        }
        if let Some(buf) = bank.get(s) {
            let _ = tx.send(Cmd::Play { buf: buf.clone(), l, r, rate: pitch });
        }
    }

    /// Loudness of the looping backgrounds (0..1 each).
    pub fn beds(&mut self, rain: f32, cave: f32, water: f32) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(Cmd::Bed([rain, cave, water]));
        }
    }
}

type Opened = (cpal::Stream, Sender<Cmd>, f32);

fn open() -> Result<Opened, String> {
    let host = cpal::default_host();
    let dev = host.default_output_device().ok_or("no output device")?;
    let cfg = dev.default_output_config().map_err(|e| e.to_string())?;
    let rate = cfg.sample_rate() as f32;
    let channels = cfg.channels() as usize;
    let (tx, rx) = channel();
    let mut mix = Mixer::new(rx, rate);
    let err = |e| eprintln!("audio: {e}");
    let mut sc = cfg.config();
    // small buffers keep sounds in step with the picture
    sc.buffer_size = match cfg.buffer_size() {
        cpal::SupportedBufferSize::Range { min, max } => cpal::BufferSize::Fixed(1024.clamp(*min, *max)),
        _ => cpal::BufferSize::Default,
    };
    let stream = match cfg.sample_format() {
        cpal::SampleFormat::F32 => dev.build_output_stream(sc.clone(), move |out: &mut [f32], _| mix.fill(out, channels, |v| v), err, None),
        cpal::SampleFormat::I16 => dev.build_output_stream(sc.clone(), move |out: &mut [i16], _| mix.fill(out, channels, |v| (v * 32767.0) as i16), err, None),
        f => return Err(format!("sample format {f:?}")),
    }
    .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok((stream, tx, rate))
}

struct Voice {
    buf: Arc<[f32]>,
    pos: f32,
    rate: f32,
    l: f32,
    r: f32,
}

/// Lives on the audio thread.
struct Mixer {
    rx: Receiver<Cmd>,
    rate: f32,
    voices: Vec<Voice>,
    master: f32,
    music_vol: f32,
    bed_want: [f32; BEDS],
    bed: [f32; BEDS],
    noise: u32,
    /// filter states of the beds
    lp: [f32; 4],
    music: Music,
}

impl Mixer {
    fn new(rx: Receiver<Cmd>, rate: f32) -> Mixer {
        Mixer { rx, rate, voices: Vec::with_capacity(48), master: 1.0, music_vol: 0.5, bed_want: [0.0; BEDS], bed: [0.0; BEDS], noise: 12345, lp: [0.0; 4], music: Music::new(rate) }
    }

    fn fill<T: Copy>(&mut self, out: &mut [T], channels: usize, conv: impl Fn(f32) -> T) {
        while let Ok(c) = self.rx.try_recv() {
            match c {
                Cmd::Play { buf, l, r, rate } => {
                    if self.voices.len() >= 40 {
                        self.voices.remove(0);
                    }
                    self.voices.push(Voice { buf, pos: 0.0, rate, l, r });
                }
                Cmd::Bed(b) => self.bed_want = b,
                Cmd::Volume { master, music } => (self.master, self.music_vol) = (master, music),
            }
        }
        let ease = 1.0 / (self.rate * 0.4);
        for frame in out.chunks_mut(channels.max(1)) {
            let (mut l, mut r) = (0.0f32, 0.0f32);
            for v in &mut self.voices {
                let i = v.pos as usize;
                if i + 1 < v.buf.len() {
                    let f = v.pos - i as f32;
                    let s = v.buf[i] + (v.buf[i + 1] - v.buf[i]) * f;
                    l += s * v.l;
                    r += s * v.r;
                }
                v.pos += v.rate;
            }
            // backgrounds: filtered noise made on the fly
            for b in 0..BEDS {
                self.bed[b] += (self.bed_want[b] - self.bed[b]) * ease;
            }
            if self.bed.iter().any(|&b| b > 0.001) {
                self.noise ^= self.noise << 13;
                self.noise ^= self.noise >> 17;
                self.noise ^= self.noise << 5;
                let n = (self.noise as f32 / u32::MAX as f32) * 2.0 - 1.0;
                // rain: a hiss with a little body
                self.lp[0] += (n - self.lp[0]) * 0.35;
                self.lp[1] += (self.lp[0] - self.lp[1]) * 0.05;
                let rain = (self.lp[0] - self.lp[1]) * 0.55 + self.lp[1] * 0.6;
                // cave: a low, slow wind
                self.lp[2] += (n - self.lp[2]) * 0.004;
                let cave = self.lp[2] * 5.0;
                // under water: a muffled rumble
                self.lp[3] += (n - self.lp[3]) * 0.012;
                let water = self.lp[3] * 3.0;
                let s = rain * self.bed[0] * 0.5 + cave * self.bed[1] * 0.5 + water * self.bed[2] * 0.6;
                l += s;
                r += s;
            }
            let m = self.music.next() * self.music_vol * 0.5;
            l = soft_clip((l + m) * self.master);
            r = soft_clip((r + m) * self.master);
            frame[0] = conv(l);
            if frame.len() > 1 {
                frame[1] = conv(r);
                for x in &mut frame[2..] {
                    *x = conv(0.0);
                }
            }
        }
        self.voices.retain(|v| (v.pos as usize) + 1 < v.buf.len());
    }
}

#[inline]
fn soft_clip(x: f32) -> f32 {
    if x.abs() < 0.7 {
        x
    } else {
        x.signum() * (0.7 + 0.3 * ((x.abs() - 0.7) / 0.3).tanh())
    }
}

/// Quiet music: a few soft notes now and then, with long silences between, made up as it
/// plays from a pentatonic scale.
struct Music {
    rate: f32,
    rng: u32,
    /// samples until the next phrase
    wait: u64,
    notes: Vec<(f32, u64, f32)>, // (frequency, start sample, loudness)
    t: u64,
    lp: f32,
}

impl Music {
    fn new(rate: f32) -> Music {
        Music { rate, rng: 777, wait: (rate * 20.0) as u64, notes: Vec::new(), t: 0, lp: 0.0 }
    }
    fn rand(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        (self.rng >> 8) as f32 / (1u32 << 24) as f32
    }
    fn next(&mut self) -> f32 {
        self.t += 1;
        if self.wait > 0 {
            self.wait -= 1;
        } else {
            self.phrase();
        }
        if self.notes.is_empty() {
            return 0.0;
        }
        let mut s = 0.0;
        let rate = self.rate;
        let t = self.t;
        self.notes.retain(|&(f, start, amp)| {
            if t < start {
                return true;
            }
            let x = (t - start) as f32 / rate;
            if x > 5.0 {
                return false;
            }
            let env = (x * 60.0).min(1.0) * (-x * 0.9).exp();
            let ph = TAU * f * x;
            s += amp * env * (ph.sin() + 0.25 * (2.0 * ph).sin() * (-x * 2.0).exp() + 0.08 * (3.0 * ph).sin());
            true
        });
        self.lp += (s - self.lp) * 0.3;
        self.lp * 0.16
    }
    fn phrase(&mut self) {
        const SCALE: [f32; 10] = [0.0, 2.0, 4.0, 7.0, 9.0, 12.0, 14.0, 16.0, 19.0, 21.0];
        let root = 196.0 * if self.rand() < 0.5 { 1.0 } else { 1.122 }; // G3 or A3
        let n = 4 + (self.rand() * 9.0) as usize;
        let mut at = self.t;
        let mut step = (self.rand() * 6.0) as usize;
        let beat = (self.rate * (0.55 + self.rand() * 0.35)) as u64;
        // a held low note under the phrase
        self.notes.push((root * 0.5, at, 0.5));
        for i in 0..n {
            let mv = (self.rand() * 5.0) as isize - 2;
            step = (step as isize + mv).clamp(0, SCALE.len() as isize - 1) as usize;
            let f = root * 2f32.powf(SCALE[step] / 12.0);
            let amp = 0.55 + self.rand() * 0.3;
            self.notes.push((f, at, amp));
            if self.rand() < 0.3 {
                // a third or fifth below, now and then
                let below = SCALE[step.saturating_sub(2)];
                self.notes.push((root * 2f32.powf(below / 12.0), at, 0.3));
            }
            at += beat * if self.rand() < 0.25 { 2 } else { 1 };
            if i == n - 1 && self.rand() < 0.5 {
                self.notes.push((root * 0.5, at, 0.4));
            }
        }
        // then a long rest: half a minute to two and a half
        self.wait = (at - self.t) + (self.rate * (30.0 + self.rand() * 120.0)) as u64;
    }
}

// --- synthesis -----------------------------------------------------------------------

struct Synth {
    rate: f32,
    rng: u32,
}

impl Synth {
    fn noise(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        (self.rng as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
    fn len(&self, secs: f32) -> usize {
        (secs * self.rate) as usize
    }
    /// One-pole low-pass coefficient for a cut-off in Hz.
    fn k(&self, hz: f32) -> f32 {
        1.0 - (-TAU * hz / self.rate).exp()
    }

    /// Filtered noise with an attack and an exponential fall: the base of most hits.
    /// `lo`/`hi` are the band in Hz; `hi_end` lets the top of the band slide.
    fn burst(&mut self, secs: f32, attack: f32, decay: f32, lo: f32, hi: f32, hi_end: f32) -> Vec<f32> {
        let n = self.len(secs);
        let mut v = Vec::with_capacity(n);
        let (mut a, mut b) = (0.0, 0.0);
        for i in 0..n {
            let x = i as f32 / self.rate;
            let top = hi + (hi_end - hi) * (x / secs);
            let kh = self.k(top);
            let kl = self.k(lo);
            a += (self.noise() - a) * kh;
            b += (a - b) * kl;
            let env = (x / attack.max(1e-4)).min(1.0) * (-x / decay).exp();
            v.push((a - b) * env);
        }
        v
    }
    /// A tone sliding from `f0` to `f1` Hz, `harm` adding a little edge.
    fn tone(&mut self, secs: f32, f0: f32, f1: f32, attack: f32, decay: f32, harm: f32) -> Vec<f32> {
        let n = self.len(secs);
        let mut v = Vec::with_capacity(n);
        let mut ph = 0.0f32;
        for i in 0..n {
            let x = i as f32 / self.rate;
            let f = f0 * (f1 / f0).powf(x / secs);
            ph += TAU * f / self.rate;
            let env = (x / attack.max(1e-4)).min(1.0) * (-x / decay).exp();
            v.push((ph.sin() + harm * (2.0 * ph).sin() + harm * 0.5 * (3.0 * ph).sin()) * env);
        }
        v
    }
    /// A buzzy voice (a saw through a low-pass) for animals, with vibrato.
    fn voice(&mut self, secs: f32, f0: f32, f1: f32, cutoff: f32, vib: f32, attack: f32, release: f32) -> Vec<f32> {
        let n = self.len(secs);
        let mut v = Vec::with_capacity(n);
        let (mut ph, mut a, mut b) = (0.0f32, 0.0, 0.0);
        let k = self.k(cutoff);
        for i in 0..n {
            let x = i as f32 / self.rate;
            let f = (f0 + (f1 - f0) * (x / secs)) * (1.0 + vib * (TAU * 6.0 * x).sin());
            ph = (ph + f / self.rate).fract();
            let saw = ph * 2.0 - 1.0 + self.noise() * 0.08;
            a += (saw - a) * k;
            b += (a - b) * k;
            let env = (x / attack).min(1.0) * ((secs - x) / release).clamp(0.0, 1.0);
            v.push(b * env);
        }
        v
    }
    /// A plucked string (Karplus–Strong).
    fn pluck(&mut self, secs: f32, f: f32, damp: f32) -> Vec<f32> {
        let p = (self.rate / f) as usize;
        let mut ring: Vec<f32> = (0..p).map(|_| self.noise()).collect();
        let n = self.len(secs);
        let mut v = Vec::with_capacity(n);
        for i in 0..n {
            let j = i % p;
            let s = ring[j];
            ring[j] = (s + ring[(j + 1) % p]) * 0.5 * damp;
            v.push(s);
        }
        v
    }
}

fn mix(a: &mut Vec<f32>, b: &[f32], gain: f32, at: usize) {
    if a.len() < at + b.len() {
        a.resize(at + b.len(), 0.0);
    }
    for (i, &s) in b.iter().enumerate() {
        a[at + i] += s * gain;
    }
}

fn scaled(mut v: Vec<f32>, peak: f32) -> Arc<[f32]> {
    let m = v.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
    if m > 0.0 {
        let k = peak / m;
        for s in &mut v {
            *s *= k;
        }
    }
    v.into()
}

fn build_bank(rate: f32) -> Bank {
    let mut s = Synth { rate, rng: 0x1234_5678 };
    let sec = |s: &Synth, x: f32| s.len(x);

    // footsteps: short, soft
    let step = |s: &mut Synth, m: Surface| -> Arc<[f32]> {
        let v = match m {
            Surface::Grass => s.burst(0.09, 0.006, 0.03, 200.0, 1800.0, 900.0),
            Surface::Stone => {
                let mut v = s.burst(0.06, 0.001, 0.012, 900.0, 5000.0, 3000.0);
                let t = s.tone(0.06, 140.0, 90.0, 0.001, 0.015, 0.2);
                mix(&mut v, &t, 0.5, 0);
                v
            }
            Surface::Wood => {
                let mut v = s.tone(0.09, 210.0, 170.0, 0.001, 0.025, 0.5);
                let c = s.burst(0.03, 0.0005, 0.006, 1200.0, 6000.0, 6000.0);
                mix(&mut v, &c, 0.4, 0);
                v
            }
            Surface::Sand => s.burst(0.11, 0.01, 0.04, 1200.0, 6000.0, 3500.0),
            Surface::Snow => {
                // crunchy: grains of noise
                let mut v = Vec::new();
                for g in 0..6 {
                    let b = s.burst(0.025, 0.001, 0.006, 600.0, 4000.0, 2500.0);
                    let j = 0.004 * s.noise().abs();
                    let at = sec(s, 0.012 * g as f32 + j);
                    mix(&mut v, &b, 1.0 - g as f32 * 0.12, at);
                }
                v
            }
            Surface::Glass => {
                let mut v = s.tone(0.12, 1900.0, 1850.0, 0.001, 0.03, 0.3);
                let c = s.burst(0.03, 0.0005, 0.006, 2000.0, 8000.0, 8000.0);
                mix(&mut v, &c, 0.5, 0);
                v
            }
            Surface::Cloth => s.burst(0.1, 0.01, 0.035, 80.0, 700.0, 400.0),
        };
        scaled(v, 0.22)
    };
    // digging: the step, harder
    let dig = |s: &mut Synth, m: Surface| -> Arc<[f32]> {
        let v = match m {
            Surface::Grass => s.burst(0.14, 0.004, 0.045, 150.0, 2400.0, 1000.0),
            Surface::Stone => {
                let mut v = s.burst(0.1, 0.0005, 0.02, 800.0, 7000.0, 4000.0);
                let t = s.tone(0.08, 260.0, 180.0, 0.0005, 0.02, 0.4);
                mix(&mut v, &t, 0.6, 0);
                v
            }
            Surface::Wood => {
                let mut v = s.tone(0.12, 260.0, 200.0, 0.0005, 0.03, 0.6);
                let c = s.burst(0.04, 0.0005, 0.01, 900.0, 5000.0, 5000.0);
                mix(&mut v, &c, 0.6, 0);
                v
            }
            Surface::Sand => s.burst(0.14, 0.006, 0.05, 900.0, 5000.0, 2500.0),
            Surface::Snow => s.burst(0.12, 0.004, 0.04, 500.0, 3500.0, 2000.0),
            Surface::Glass => {
                let mut v = s.tone(0.1, 2400.0, 2300.0, 0.0005, 0.02, 0.4);
                let c = s.burst(0.05, 0.0005, 0.01, 2500.0, 9000.0, 9000.0);
                mix(&mut v, &c, 0.7, 0);
                v
            }
            Surface::Cloth => s.burst(0.12, 0.006, 0.04, 80.0, 900.0, 500.0),
        };
        scaled(v, 0.32)
    };
    // breaking: a longer crumble
    let brk = |s: &mut Synth, m: Surface| -> Arc<[f32]> {
        let v = match m {
            Surface::Glass => {
                let mut v = Vec::new();
                for g in 0..9 {
                    let f = 2200.0 + 2400.0 * s.noise().abs();
                    let t = s.tone(0.18, f, f * 0.97, 0.0005, 0.035, 0.2);
                    let at = sec(s, 0.018 * g as f32);
                    mix(&mut v, &t, 0.5, at);
                }
                let c = s.burst(0.25, 0.0005, 0.06, 2000.0, 10000.0, 6000.0);
                mix(&mut v, &c, 0.7, 0);
                v
            }
            m => {
                let (lo, hi, end) = match m {
                    Surface::Grass => (120.0, 2200.0, 600.0),
                    Surface::Stone => (500.0, 6000.0, 1500.0),
                    Surface::Wood => (200.0, 3000.0, 900.0),
                    Surface::Sand => (700.0, 5000.0, 1800.0),
                    Surface::Snow => (400.0, 3500.0, 1200.0),
                    _ => (80.0, 900.0, 300.0),
                };
                let mut v = s.burst(0.28, 0.002, 0.07, lo, hi, end);
                let thump = s.tone(0.15, 120.0, 70.0, 0.001, 0.04, 0.3);
                mix(&mut v, &thump, 0.35, 0);
                if m == Surface::Wood {
                    let k = s.tone(0.15, 190.0, 150.0, 0.001, 0.04, 0.6);
                    mix(&mut v, &k, 0.6, 0);
                }
                v
            }
        };
        scaled(v, 0.42)
    };
    let place = |s: &mut Synth, m: Surface| -> Arc<[f32]> {
        let mut v = s.tone(0.1, 150.0, 95.0, 0.001, 0.025, 0.3);
        let (lo, hi) = match m {
            Surface::Stone => (600.0, 5000.0),
            Surface::Wood => (300.0, 3000.0),
            Surface::Glass => (2000.0, 9000.0),
            Surface::Cloth => (80.0, 600.0),
            _ => (200.0, 2500.0),
        };
        let c = s.burst(0.08, 0.001, 0.018, lo, hi, hi * 0.6);
        mix(&mut v, &c, 0.8, 0);
        scaled(v, 0.36)
    };
    let all = [Surface::Grass, Surface::Stone, Surface::Wood, Surface::Sand, Surface::Snow, Surface::Glass, Surface::Cloth];
    let step_b = all.map(|m| step(&mut s, m));
    let dig_b = all.map(|m| dig(&mut s, m));
    let brk_b = all.map(|m| brk(&mut s, m));
    let place_b = all.map(|m| place(&mut s, m));

    let mut other: Vec<(Snd, Arc<[f32]>)> = Vec::new();
    other.push((Snd::Pop, scaled(s.tone(0.07, 520.0, 1250.0, 0.002, 0.03, 0.1), 0.3)));
    other.push((Snd::Click, scaled(s.tone(0.03, 1400.0, 1100.0, 0.0005, 0.008, 0.3), 0.18)));
    {
        let mut v = s.tone(0.14, 110.0, 60.0, 0.001, 0.035, 0.4);
        let c = s.burst(0.1, 0.001, 0.025, 100.0, 1500.0, 500.0);
        mix(&mut v, &c, 0.9, 0);
        other.push((Snd::Hit, scaled(v, 0.4)));
    }
    {
        // the player's own wince: a short falling grunt
        let mut v = s.voice(0.22, 260.0, 170.0, 900.0, 0.01, 0.01, 0.12);
        let c = s.burst(0.1, 0.001, 0.02, 100.0, 1200.0, 500.0);
        mix(&mut v, &c, 0.6, 0);
        other.push((Snd::Hurt, scaled(v, 0.42)));
    }
    other.push((Snd::MobHurt, scaled(s.voice(0.18, 330.0, 240.0, 1400.0, 0.02, 0.005, 0.1), 0.35)));
    other.push((Snd::Deer, scaled(s.voice(0.45, 620.0, 520.0, 2200.0, 0.035, 0.03, 0.25), 0.25)));
    other.push((Snd::Boar, {
        let mut v = Vec::new();
        for g in 0..3 {
            let b = s.voice(0.13, 95.0, 80.0, 500.0, 0.05, 0.01, 0.06);
            mix(&mut v, &b, 1.0, sec(&s, 0.12 * g as f32));
        }
        scaled(v, 0.32)
    }));
    other.push((Snd::Hare, scaled(s.tone(0.08, 1500.0, 1900.0, 0.003, 0.02, 0.2), 0.12)));
    other.push((Snd::Wolf, {
        let mut v = s.voice(0.8, 105.0, 95.0, 420.0, 0.06, 0.08, 0.25);
        let n = s.burst(0.8, 0.08, 0.5, 80.0, 600.0, 400.0);
        mix(&mut v, &n, 0.5, 0);
        scaled(v, 0.36)
    }));
    other.push((Snd::Zombie, scaled(s.voice(1.1, 88.0, 66.0, 380.0, 0.04, 0.15, 0.4), 0.4)));
    other.push((Snd::Skeleton, {
        let mut v = Vec::new();
        for g in 0..7 {
            let f = 700.0 + 500.0 * s.noise().abs();
            let t = s.tone(0.05, f, 500.0, 0.0005, 0.01, 0.6);
            let j = 0.01 * s.noise().abs();
            mix(&mut v, &t, 1.0, sec(&s, 0.045 * g as f32 + j));
        }
        scaled(v, 0.28)
    }));
    other.push((Snd::Spider, scaled(s.burst(0.55, 0.05, 0.2, 2500.0, 9000.0, 7000.0), 0.22)));
    other.push((Snd::Slime, {
        let mut v = s.tone(0.18, 210.0, 70.0, 0.003, 0.05, 0.2);
        let n = s.burst(0.15, 0.003, 0.04, 100.0, 900.0, 300.0);
        mix(&mut v, &n, 0.7, 0);
        scaled(v, 0.36)
    }));
    other.push((Snd::Villager, scaled(s.voice(0.42, 150.0, 175.0, 700.0, 0.02, 0.04, 0.18), 0.32)));
    other.push((Snd::Splash, scaled(s.burst(0.6, 0.004, 0.18, 150.0, 6000.0, 700.0), 0.42)));
    other.push((Snd::Swim, scaled(s.burst(0.35, 0.06, 0.1, 100.0, 1600.0, 600.0), 0.2)));
    other.push((Snd::Bow, {
        let mut v = s.pluck(0.5, 110.0, 0.992);
        let w = s.burst(0.15, 0.002, 0.05, 300.0, 3000.0, 1000.0);
        mix(&mut v, &w, 0.5, 0);
        scaled(v, 0.36)
    }));
    other.push((Snd::ArrowHit, {
        let mut v = s.tone(0.08, 320.0, 200.0, 0.0005, 0.018, 0.5);
        let c = s.burst(0.05, 0.0005, 0.01, 800.0, 5000.0, 3000.0);
        mix(&mut v, &c, 0.7, 0);
        scaled(v, 0.32)
    }));
    other.push((Snd::Eat, {
        let mut v = Vec::new();
        for g in 0..4 {
            let b = s.burst(0.06, 0.002, 0.016, 400.0, 3500.0, 1800.0);
            mix(&mut v, &b, 1.0, sec(&s, 0.075 * g as f32));
        }
        scaled(v, 0.3)
    }));
    other.push((Snd::Door, {
        let mut v = s.tone(0.12, 180.0, 140.0, 0.001, 0.03, 0.6);
        let k = s.tone(0.12, 150.0, 120.0, 0.001, 0.03, 0.6);
        mix(&mut v, &k, 0.8, sec(&s, 0.07));
        scaled(v, 0.34)
    }));
    other.push((Snd::Chest, {
        let mut v = s.voice(0.35, 300.0, 420.0, 1200.0, 0.03, 0.05, 0.1);
        let k = s.tone(0.12, 130.0, 100.0, 0.001, 0.03, 0.6);
        mix(&mut v, &k, 1.2, sec(&s, 0.25));
        scaled(v, 0.3)
    }));
    other.push((Snd::Xp, {
        let mut v = s.tone(0.35, 1320.0, 1320.0, 0.002, 0.09, 0.05);
        let b = s.tone(0.35, 1980.0, 1980.0, 0.002, 0.06, 0.0);
        mix(&mut v, &b, 0.4, 0);
        scaled(v, 0.16)
    }));
    other.push((Snd::LevelUp, {
        let mut v = Vec::new();
        for (i, f) in [523.0, 659.0, 784.0, 1046.0].into_iter().enumerate() {
            let t = s.tone(0.6, f, f, 0.003, 0.18, 0.15);
            mix(&mut v, &t, 1.0, sec(&s, 0.09 * i as f32));
        }
        scaled(v, 0.3)
    }));
    other.push((Snd::Thunder, {
        // a crack, then a long rolling rumble
        let mut v = s.burst(0.4, 0.001, 0.08, 300.0, 6000.0, 1500.0);
        let n = s.len(4.0);
        let (mut a, mut b, mut m) = (0.0f32, 0.0f32, 0.0f32);
        let k1 = s.k(220.0);
        let k2 = s.k(35.0);
        let mut roll = Vec::with_capacity(n);
        for i in 0..n {
            let x = i as f32 / rate;
            a += (s.noise() - a) * k1;
            b += (a - b) * k1;
            m += (s.noise() - m) * s.k(3.0);
            let env = (x / 0.15).min(1.0) * (-x / 1.3).exp() * (0.6 + m.abs() * 6.0).min(1.6);
            roll.push((b - b * k2) * env);
        }
        mix(&mut v, &roll, 2.2, 0);
        scaled(v, 0.7)
    }));
    other.push((Snd::Fizz, scaled(s.burst(0.3, 0.005, 0.08, 2000.0, 9000.0, 4000.0), 0.18)));
    other.push((Snd::Crackle, {
        let mut v = Vec::new();
        for g in 0..5 {
            let c = s.burst(0.02, 0.0005, 0.004, 1500.0, 7000.0, 7000.0);
            let (g1, j) = (0.5 + 0.5 * s.noise().abs(), 0.05 * s.noise().abs());
            mix(&mut v, &c, g1, sec(&s, 0.03 * g as f32 + j));
        }
        scaled(v, 0.14)
    }));
    Bank { step: step_b, dig: dig_b, brk: brk_b, place: place_b, other }
}

/// The sound a block makes.
pub fn surface(b: crate::world::Block) -> Surface {
    use crate::world::*;
    match b {
        b if is_leaves(b) || is_plant(b) || is_grassy(b) => Surface::Grass,
        DIRT | FARMLAND | CLAY | GRAVEL | MUSHROOM | BERRY_BUSH | CACTUS | THATCH => Surface::Grass,
        SAND | RED_SAND | SANDSTONE => Surface::Sand,
        SNOW | ICE => Surface::Snow,
        GLASS | PANE | LAMP => Surface::Glass,
        LEATHER | CLOTH | BED => Surface::Cloth,
        LOG | BIRCH_LOG | PLANKS | BIRCH_PLANKS | WORKBENCH | CHEST | LOOT_CHEST | FENCE | BIRCH_FENCE => Surface::Wood,
        b if is_door(b) || is_trapdoor(b) || is_ladder(b) || is_torch(b) => Surface::Wood,
        b if is_slab(b) => surface(SHAPE_FULL[slab_mat(b) as usize]),
        b if is_stairs(b) => surface(SHAPE_FULL[stair_mat(b) as usize]),
        _ => Surface::Stone,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_sound_is_made_and_in_range() {
        let b = build_bank(48000.0);
        let all = b.step.iter().chain(&b.dig).chain(&b.brk).chain(&b.place).chain(b.other.iter().map(|o| &o.1));
        for (i, s) in all.enumerate() {
            assert!(s.len() > 100, "sound {i} is empty");
            let peak = s.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            assert!(peak.is_finite() && peak > 0.05 && peak <= 1.0, "sound {i} peak {peak}");
        }
        assert_eq!(b.other.len(), 26);
    }
}
