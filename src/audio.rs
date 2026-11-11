//! Sound. Every effect is synthesised in code at start-up (there are no sound files) and
//! mixed on the audio device's own thread, so sound costs the render loop nothing but a
//! channel send. Without a sound device the game runs silent.

use crate::mobs::Kind;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use glam::Vec3;
use std::f32::consts::TAU;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

/// Every sound effect. Steps and digs come in one flavour per `Surface`, creatures'
/// calls one per kind and call.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Snd {
    Step(Surface),
    Dig(Surface),
    Break(Surface),
    Place(Surface),
    Pop,
    Hit,
    Hurt,
    Voice(Kind, Call),
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
    Gravel,
    Stone,
    Wood,
    Sand,
    Snow,
    Glass,
    Cloth,
}

/// the looping backgrounds: rain, cave wind, under water
const BEDS: usize = 3;

enum Cmd {
    Play { buf: Arc<[f32]>, l: f32, r: f32, rate: f32 },
    Bed([f32; BEDS]),
    Volume { master: f32, music: f32 },
}

/// Each sound with a few takes of it, one picked at random each time.
struct Bank {
    sounds: Vec<(Snd, Vec<Arc<[f32]>>)>,
}

impl Bank {
    fn get(&self, s: Snd, pick: f32) -> Option<&Arc<[f32]>> {
        let takes = &self.sounds.iter().find(|o| o.0 == s)?.1;
        takes.get(((pick * takes.len() as f32) as usize).min(takes.len().saturating_sub(1)))
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
                if let Some(dir) = std::env::var_os("CUBEWILD_DUMP_SOUNDS") {
                    dump(a.bank.as_ref().unwrap(), std::path::Path::new(&dir), rate);
                }
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
        let pick = self.rand();
        let (Some(tx), Some(bank)) = (&self.tx, &self.bank) else { return };
        if l.max(r) < 0.004 {
            return;
        }
        if let Some(buf) = bank.get(s, pick) {
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

/// Writes every take of every sound as a WAV file, for listening to them outside the
/// game (CUBEWILD_DUMP_SOUNDS=<folder>).
fn dump(bank: &Bank, dir: &std::path::Path, rate: f32) {
    let _ = std::fs::create_dir_all(dir);
    for (snd, takes) in &bank.sounds {
        let name: String = format!("{snd:?}").to_lowercase().chars().filter(|c| c.is_alphanumeric() || *c == ',').map(|c| if c == ',' { '-' } else { c }).collect();
        for (i, t) in takes.iter().enumerate() {
            let mut b = Vec::with_capacity(44 + t.len() * 2);
            let n = (t.len() * 2) as u32;
            b.extend_from_slice(b"RIFF");
            b.extend_from_slice(&(36 + n).to_le_bytes());
            b.extend_from_slice(b"WAVEfmt ");
            b.extend_from_slice(&16u32.to_le_bytes());
            b.extend_from_slice(&1u16.to_le_bytes());
            b.extend_from_slice(&1u16.to_le_bytes());
            b.extend_from_slice(&(rate as u32).to_le_bytes());
            b.extend_from_slice(&(rate as u32 * 2).to_le_bytes());
            b.extend_from_slice(&2u16.to_le_bytes());
            b.extend_from_slice(&16u16.to_le_bytes());
            b.extend_from_slice(b"data");
            b.extend_from_slice(&n.to_le_bytes());
            for v in t.iter() {
                b.extend_from_slice(&((v * 32767.0) as i16).to_le_bytes());
            }
            let _ = std::fs::write(dir.join(format!("{name}-{}.wav", i + 1)), b);
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

/// Brought to an even loudness (root mean square `rms`), but never past `peak`.
fn leveled(mut v: Vec<f32>, rms: f32, peak: f32) -> Arc<[f32]> {
    let n = v.len().max(1) as f32;
    let r = (v.iter().map(|s| s * s).sum::<f32>() / n).sqrt();
    let m = v.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
    if r > 0.0 && m > 0.0 {
        let k = (rms / r).min(peak / m);
        for s in &mut v {
            *s *= k;
        }
    }
    v.into()
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

    let mut sounds: Vec<(Snd, Vec<Arc<[f32]>>)> = Vec::new();
    // steps (heel, then toe), digging, breaking and placing, a few takes of each
    let all = [Surface::Grass, Surface::Gravel, Surface::Stone, Surface::Wood, Surface::Sand, Surface::Snow, Surface::Glass, Surface::Cloth];
    for m in all {
        let step = (0..4).map(|_| leveled(s.strike(m, 0.3, true), 0.04, 0.32)).collect();
        let dig = (0..3).map(|_| leveled(s.strike(m, 0.75, false), 0.06, 0.45)).collect();
        let brk = (0..2)
            .map(|_| {
                let mut v = s.strike(m, 1.0, false);
                // and the pieces falling
                let g = s.grains(14, 0.18, 0.02, 400.0, 3000.0, 1.0, 0.0);
                let at = s.len(0.03);
                mix(&mut v, &g, 0.5, at);
                leveled(v, 0.07, 0.55)
            })
            .collect();
        let place = (0..2).map(|_| leveled(s.strike(m, 0.6, false), 0.06, 0.45)).collect();
        sounds.push((Snd::Step(m), step));
        sounds.push((Snd::Dig(m), dig));
        sounds.push((Snd::Break(m), brk));
        sounds.push((Snd::Place(m), place));
    }
    // creatures: three takes of their idle calls, two of a hurt cry, one death
    for k in KINDS {
        for (c, takes, rms) in [(Call::Idle, 3, 0.09), (Call::Hurt, 2, 0.12), (Call::Death, 1, 0.11)] {
            let v = (0..takes).map(|t| leveled(creature(&mut s, k, c, t), rms, 0.7)).collect();
            sounds.push((Snd::Voice(k, c), v));
        }
    }

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
    sounds.extend(other.into_iter().map(|(k, v)| (k, vec![v])));
    Bank { sounds }
}

/// The sound a block makes.
pub fn surface(b: crate::world::Block) -> Surface {
    use crate::world::*;
    match b {
        b if is_leaves(b) || is_plant(b) || is_grassy(b) => Surface::Grass,
        GRAVEL => Surface::Gravel,
        DIRT | FARMLAND | CLAY | MUSHROOM | BERRY_BUSH | CACTUS | THATCH => Surface::Grass,
        SAND | RED_SAND | SANDSTONE => Surface::Sand,
        SNOW => Surface::Snow,
        GLASS | PANE | LAMP | ICE => Surface::Glass,
        LEATHER | CLOTH | BED => Surface::Cloth,
        LOG | BIRCH_LOG | PLANKS | BIRCH_PLANKS | WORKBENCH | CHEST | LOOT_CHEST | FENCE | BIRCH_FENCE => Surface::Wood,
        b if is_door(b) || is_trapdoor(b) || is_ladder(b) || is_torch(b) => Surface::Wood,
        b if is_slab(b) => surface(SHAPE_FULL[slab_mat(b) as usize]),
        b if is_stairs(b) => surface(SHAPE_FULL[stair_mat(b) as usize]),
        _ => Surface::Stone,
    }
}


// --- better building blocks: resonant filters, grains, strikes, voices ---------------

/// A two-pole filter (the usual audio cookbook forms).
#[derive(Clone, Copy, Default)]
struct Bq {
    b: [f32; 3],
    a: [f32; 2],
    x: [f32; 2],
    y: [f32; 2],
}

#[derive(Clone, Copy)]
enum Fk {
    Band,
    Low,
    High,
}

impl Bq {
    fn new(kind: Fk, rate: f32, f: f32, q: f32) -> Bq {
        let mut b = Bq::default();
        b.set(kind, rate, f, q);
        b
    }
    /// New coefficients, keeping what is ringing (for sweeps).
    fn set(&mut self, kind: Fk, rate: f32, f: f32, q: f32) {
        let w = TAU * f.clamp(20.0, rate * 0.45) / rate;
        let (sw, cw) = w.sin_cos();
        let al = sw / (2.0 * q.max(0.1));
        let a0 = 1.0 + al;
        let b = match kind {
            Fk::Band => [al, 0.0, -al],
            Fk::Low => [(1.0 - cw) * 0.5, 1.0 - cw, (1.0 - cw) * 0.5],
            Fk::High => [(1.0 + cw) * 0.5, -(1.0 + cw), (1.0 + cw) * 0.5],
        };
        self.b = b.map(|v| v / a0);
        self.a = [-2.0 * cw / a0, (1.0 - al) / a0];
    }
    #[inline]
    fn run(&mut self, x: f32) -> f32 {
        let y = self.b[0] * x + self.b[1] * self.x[0] + self.b[2] * self.x[1] - self.a[0] * self.y[0] - self.a[1] * self.y[1];
        self.x = [x, self.x[0]];
        self.y = [y, self.y[0]];
        y
    }
}

impl Synth {
    fn r(&mut self) -> f32 {
        self.noise() * 0.5 + 0.5
    }
    fn between(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.r()
    }

    /// Little crackles of filtered noise scattered over `spread` seconds: the rustle of
    /// grass, the crunch of gravel and snow, the hiss of sand. `shape` 0 = all at the
    /// start, 1 = swelling then fading.
    fn grains(&mut self, n: usize, spread: f32, len: f32, lo: f32, hi: f32, q: f32, shape: f32) -> Vec<f32> {
        let mut v = vec![0.0; self.len(spread + len * 3.0)];
        for g in 0..n {
            let u = self.r();
            let at = if shape > 0.5 { u * spread } else { u * u * spread };
            let amp = if shape > 0.5 { (1.0 - (at / spread * 2.0 - 1.0).abs()) * 0.8 + 0.2 } else { 1.0 - 0.6 * at / spread } * self.between(0.4, 1.0);
            let f = self.between(lo, hi);
            let mut bq = Bq::new(Fk::Band, self.rate, f, q);
            let k = self.between(0.6, 1.4);
            let gl = self.len(len * k);
            let start = self.len(at);
            let _ = g;
            for i in 0..gl {
                let x = i as f32 / gl as f32;
                let env = (1.0 - x).powi(2) * (x * 12.0).min(1.0);
                if let Some(s) = v.get_mut(start + i) {
                    *s += bq.run(self.noise()) * env * amp;
                }
            }
        }
        v
    }

    /// A low, dull impact: the weight of a foot coming down.
    fn thud(&mut self, secs: f32, cutoff: f32, f: f32) -> Vec<f32> {
        let n = self.len(secs);
        let mut lp = Bq::new(Fk::Low, self.rate, cutoff, 0.7);
        let mut ph = 0.0f32;
        (0..n)
            .map(|i| {
                let x = i as f32 / self.rate;
                let env = (x / 0.002).min(1.0) * (-x / (secs * 0.25)).exp();
                ph += TAU * f * (1.0 - x / secs * 0.4) / self.rate;
                (lp.run(self.noise()) * 1.4 + ph.sin() * 0.6) * env
            })
            .collect()
    }

    /// A knock that rings: a short tap through resonances (`(Hz, Q, gain)`), for wood,
    /// stone and glass.
    fn ring(&mut self, secs: f32, modes: &[(f32, f32, f32)], tap: f32) -> Vec<f32> {
        let n = self.len(secs);
        let mut bank: Vec<(Bq, f32)> = modes.iter().map(|&(f, q, g)| (Bq::new(Fk::Band, self.rate, f * self.between(0.96, 1.04), q), g)).collect();
        let tap_n = self.len(tap).max(2);
        let mut v = Vec::with_capacity(n);
        for i in 0..n {
            let x = if i < tap_n { self.noise() * (1.0 - i as f32 / tap_n as f32) } else { 0.0 };
            let y: f32 = bank.iter_mut().map(|(b, g)| b.run(x) * *g).sum();
            let t = i as f32 / n as f32;
            v.push(y * (1.0 - t).powi(2));
        }
        v
    }

    /// A sharp click (the edge of a hard sole, a tool's point).
    fn tick(&mut self, f: f32) -> Vec<f32> {
        let n = self.len(0.006);
        let mut bq = Bq::new(Fk::Band, self.rate, f, 1.5);
        (0..n).map(|i| bq.run(self.noise()) * (1.0 - i as f32 / n as f32)).collect()
    }

    /// One strike on a surface. `force` 0..1 runs from a soft step to a hard blow;
    /// `toe` adds the second, lighter contact of a step.
    fn strike(&mut self, m: Surface, force: f32, toe: bool) -> Vec<f32> {
        let f = force;
        let mut v = match m {
            Surface::Grass => {
                let mut v = self.thud(0.06, 260.0, 70.0);
                let g = self.grains(8 + (f * 14.0) as usize, 0.06 + f * 0.05, 0.014, 1500.0, 4200.0, 1.1, 0.0);
                mix(&mut v, &g, 0.55, 0);
                v
            }
            Surface::Gravel => {
                let mut v = self.thud(0.05, 300.0, 80.0);
                let g = self.grains(16 + (f * 18.0) as usize, 0.08 + f * 0.05, 0.009, 1100.0, 3600.0, 2.4, 0.0);
                mix(&mut v, &g, 0.9, 0);
                v
            }
            Surface::Stone => {
                let mut v = self.thud(0.04, 220.0, 110.0);
                for x in &mut v {
                    *x *= 0.45;
                }
                let r = self.ring(0.06 + f * 0.04, &[(900.0, 6.0, 1.0), (1700.0, 8.0, 0.7), (3100.0, 10.0, 0.45)], 0.002);
                mix(&mut v, &r, 1.4 + f * 0.6, 0);
                let t = self.tick(3600.0 + f * 1500.0);
                mix(&mut v, &t, 0.8 + f * 0.5, 0);
                let g = self.grains(4, 0.02, 0.006, 2500.0, 5000.0, 1.5, 0.0);
                mix(&mut v, &g, 0.5, 0);
                v
            }
            Surface::Wood => {
                let mut v = self.ring(0.1 + f * 0.05, &[(165.0, 9.0, 1.0), (355.0, 11.0, 0.7), (640.0, 13.0, 0.4), (1120.0, 16.0, 0.2)], 0.003);
                let t = self.tick(2400.0);
                mix(&mut v, &t, 0.25 + f * 0.3, 0);
                v
            }
            Surface::Sand => {
                let mut v = self.thud(0.06, 180.0, 60.0);
                let g = self.grains(26 + (f * 20.0) as usize, 0.1 + f * 0.04, 0.012, 2800.0, 7500.0, 0.8, 0.0);
                mix(&mut v, &g, 0.4, 0);
                v
            }
            Surface::Snow => {
                // pressed snow: a squeaky crunch that swells and fades
                let mut v = self.thud(0.07, 220.0, 65.0);
                let g = self.grains(22 + (f * 16.0) as usize, 0.11 + f * 0.04, 0.012, 800.0, 2100.0, 3.5, 1.0);
                mix(&mut v, &g, 0.75, 0);
                v
            }
            Surface::Glass => {
                let mut v = self.ring(0.09 + f * 0.05, &[(2300.0, 40.0, 1.0), (3750.0, 55.0, 0.6), (5200.0, 60.0, 0.35)], 0.0015);
                let t = self.tick(5000.0);
                mix(&mut v, &t, 0.5, 0);
                v
            }
            Surface::Cloth => {
                let mut v = self.thud(0.07, 320.0, 60.0);
                let g = self.grains(6, 0.05, 0.02, 300.0, 900.0, 0.7, 0.0);
                mix(&mut v, &g, 0.4, 0);
                v
            }
        };
        if toe {
            // the ball of the foot follows the heel
            let k = self.between(0.028, 0.045);
            let gap = self.len(k);
            let t = self.strike(m, f * 0.6, false);
            mix(&mut v, &t, 0.45, gap);
        }
        v
    }

    /// A voiced call: a buzzing source at pitch `f0(t)` (t 0..1 through the call) shaped
    /// by three formants that glide from `fa` to `fb` (each (Hz, bandwidth)), with breath
    /// (noise in the voice) and roughness (a rasp).
    #[allow(clippy::too_many_arguments)]
    fn call(&mut self, secs: f32, f0: &dyn Fn(f32) -> f32, fa: [(f32, f32); 3], fb: [(f32, f32); 3], breath: f32, rough: f32, attack: f32, release: f32) -> Vec<f32> {
        let n = self.len(secs);
        let mut v = Vec::with_capacity(n);
        let mut ph = 0.0f32;
        let mut form = [Bq::default(); 3];
        let gains = [1.0, 0.55, 0.28];
        let mut src_lp = Bq::new(Fk::Low, self.rate, 3500.0, 0.7);
        let (mut jit, mut amp_n) = (0.0f32, 0.0f32);
        for i in 0..n {
            let t = i as f32 / n as f32;
            if i % 32 == 0 {
                for k in 0..3 {
                    let f = fa[k].0 + (fb[k].0 - fa[k].0) * t;
                    let bw = fa[k].1 + (fb[k].1 - fa[k].1) * t;
                    form[k].set(Fk::Band, self.rate, f, f / bw);
                }
            }
            // slow random wander of pitch and loudness: the rasp
            jit += (self.noise() - jit) * 0.002;
            amp_n += (self.noise() - amp_n) * 0.02;
            let f = f0(t) * (1.0 + jit * 0.08 * (0.3 + rough));
            ph = (ph + f / self.rate).fract();
            let pulse = (1.0 - 2.0 * ph) + self.noise() * breath;
            let src = src_lp.run(pulse) * (1.0 + amp_n * rough * 3.0).max(0.0);
            let y: f32 = form.iter_mut().zip(gains).map(|(b, g)| b.run(src) * g).sum();
            let x = t * secs;
            let env = (x / attack).min(1.0) * ((secs - x) / release).clamp(0.0, 1.0);
            v.push(y * env);
        }
        v
    }
}

/// A creature's call: idle, hurt or dying.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Call {
    Idle,
    Hurt,
    Death,
}

/// The voices of the creatures: a few takes of each call.
fn creature(s: &mut Synth, k: Kind, c: Call, take: usize) -> Vec<f32> {
    let j = s.between(0.92, 1.08) * (1.0 + take as f32 * 0.03);
    // vowels: (formant Hz, bandwidth)
    const UH: [(f32, f32); 3] = [(620.0, 120.0), (1180.0, 150.0), (2400.0, 250.0)];
    const OH: [(f32, f32); 3] = [(450.0, 100.0), (820.0, 130.0), (2300.0, 250.0)];
    const EH: [(f32, f32); 3] = [(560.0, 110.0), (1800.0, 160.0), (2600.0, 260.0)];
    const AA: [(f32, f32); 3] = [(800.0, 140.0), (1300.0, 170.0), (2500.0, 260.0)];
    const MM: [(f32, f32); 3] = [(280.0, 80.0), (950.0, 200.0), (2200.0, 300.0)];
    const EE: [(f32, f32); 3] = [(330.0, 90.0), (2300.0, 200.0), (3000.0, 300.0)];
    let up = |a: [(f32, f32); 3], k: f32| a.map(|(f, b)| (f * k, b * k));
    match (k, c) {
        (Kind::Zombie, Call::Idle) => {
            let len = s.between(0.9, 1.4);
            let wob = s.between(2.0, 4.0);
            s.call(len, &|t| (92.0 - 22.0 * t) * j * (1.0 + 0.05 * (t * wob * TAU).sin()), UH, OH, 0.35, 0.45, 0.18, 0.4)
        }
        (Kind::Zombie, Call::Hurt) => s.call(0.38, &|t| (150.0 - 50.0 * t) * j, AA, UH, 0.4, 0.6, 0.02, 0.2),
        (Kind::Zombie, Call::Death) => s.call(1.5, &|t| (115.0 - 60.0 * t) * j, AA, OH, 0.45, 0.5, 0.05, 0.9),
        (Kind::Skeleton, c) => {
            // bones clattering: hollow little knocks in uneven bursts
            let (n, spread, len) = match c {
                Call::Idle => (7 + take * 2, 0.45, 0.6),
                Call::Hurt => (12, 0.22, 0.35),
                Call::Death => (24, 1.1, 1.3),
            };
            let mut v = vec![0.0; s.len(len)];
            for i in 0..n {
                let u = s.r();
                let at = if c == Call::Death { (i as f32 / n as f32).powf(1.6) * spread } else { u * spread };
                let f = s.between(900.0, 2200.0) * j;
                let r = s.ring(0.08, &[(f, 12.0, 1.0), (f * 1.9, 16.0, 0.5), (f * 0.55, 9.0, 0.5)], 0.0015);
                let g = if c == Call::Death { 1.0 - 0.6 * i as f32 / n as f32 } else { s.between(0.5, 1.0) };
                let at = s.len(at);
                mix(&mut v, &r, g, at);
            }
            if c == Call::Hurt {
                let h = s.call(0.25, &|t| (380.0 - 120.0 * t) * j, EH, UH, 0.8, 0.3, 0.01, 0.15);
                mix(&mut v, &h, 0.5, 0);
            }
            v
        }
        (Kind::Spider, c) => {
            // a hiss with fast chittering clicks in it
            let len = match c {
                Call::Idle => s.between(0.5, 0.8),
                Call::Hurt => 0.3,
                Call::Death => 1.0,
            };
            let n = s.len(len);
            let mut hp = Bq::new(Fk::High, s.rate, 2200.0, 0.7);
            let mut lp = Bq::new(Fk::Low, s.rate, 5500.0, 0.7);
            let rate = s.rate;
            let chit = s.between(18.0, 26.0);
            let mut v: Vec<f32> = (0..n)
                .map(|i| {
                    let x = i as f32 / rate;
                    let t = x / len;
                    let trem = 0.5 + 0.5 * (x * TAU * chit * if c == Call::Death { 1.0 - 0.6 * t } else { 1.0 }).sin().max(0.0);
                    let env = (x / 0.04).min(1.0) * ((len - x) / 0.15).clamp(0.0, 1.0);
                    lp.run(hp.run(s.noise())) * env * (0.35 + 0.65 * trem)
                })
                .collect();
            if c != Call::Idle {
                let squeal = s.call(0.25, &|t| (1900.0 - 700.0 * t) * j, EE, EH, 0.5, 0.3, 0.01, 0.12);
                mix(&mut v, &squeal, 0.6, 0);
            }
            v
        }
        (Kind::Slime, c) => {
            // a wet squelch and a bloop
            let (len, top, bottom) = match c {
                Call::Idle => (0.3, 300.0, 90.0),
                Call::Hurt => (0.25, 480.0, 160.0),
                Call::Death => (0.6, 360.0, 60.0),
            };
            let n = s.len(len);
            let mut bp = Bq::new(Fk::Band, s.rate, 1500.0, 3.0);
            let rate = s.rate;
            let mut ph = 0.0f32;
            let mut v = Vec::with_capacity(n);
            for i in 0..n {
                let t = i as f32 / n as f32;
                if i % 32 == 0 {
                    bp.set(Fk::Band, rate, 1600.0 - 1200.0 * t, 4.0);
                }
                ph += TAU * (top * j * (bottom / top).powf(t)) / rate;
                let env = (t * 30.0).min(1.0) * (1.0 - t).powi(2);
                v.push((bp.run(s.noise()) * 0.8 + ph.sin() * 0.7) * env);
            }
            if c == Call::Death {
                let b = s.tone(0.2, 260.0, 80.0, 0.002, 0.06, 0.1);
                let at = s.len(0.3);
                mix(&mut v, &b, 0.7, at);
            }
            v
        }
        (Kind::Wolf, Call::Idle) => {
            // a low rasping growl
            let len = s.between(0.8, 1.2);
            s.call(len, &|t| 82.0 * j * (1.0 + 0.04 * (t * 9.0).sin()), up(UH, 0.8), up(OH, 0.8), 0.6, 1.0, 0.12, 0.3)
        }
        (Kind::Wolf, Call::Hurt) => s.call(0.28, &|t| (720.0 + 300.0 * (t * 3.1).sin() - 300.0 * t) * j, AA, EE, 0.15, 0.1, 0.01, 0.12),
        (Kind::Wolf, Call::Death) => s.call(0.9, &|t| (620.0 - 330.0 * t) * j, EE, UH, 0.2, 0.15, 0.02, 0.5),
        (Kind::Deer, c) => {
            // a nasal bleat with a quick shake in it
            let (len, f0) = match c {
                Call::Idle => (s.between(0.4, 0.6), 400.0),
                Call::Hurt => (0.3, 600.0),
                Call::Death => (0.75, 480.0),
            };
            let fall = if c == Call::Death { 0.5 } else { 0.85 };
            s.call(len, &|t| f0 * j * (1.0 - (1.0 - fall) * t) * (1.0 + 0.07 * (t * len * TAU * 7.0).sin()), EH, up(EH, 0.9), 0.2, 0.2, 0.03, 0.15)
        }
        (Kind::Boar, Call::Idle) => {
            // two or three short grunts and a snort
            let mut v = Vec::new();
            for g in 0..(2 + take % 2) {
                let len = s.between(0.1, 0.16);
                let f = s.between(70.0, 92.0) * j;
                let gr = s.call(len, &|t| f * (1.0 - 0.2 * t), up(OH, 0.7), up(UH, 0.7), 0.5, 0.9, 0.01, 0.05);
                let at = s.len(g as f32 * 0.19);
                mix(&mut v, &gr, 1.0, at);
            }
            let snort = s.grains(10, 0.12, 0.03, 300.0, 1200.0, 0.8, 1.0);
            let at = s.len(0.45);
            mix(&mut v, &snort, 0.5, at);
            v
        }
        (Kind::Boar, Call::Hurt) => s.call(0.36, &|t| (520.0 + 180.0 * (t * 3.0).sin()) * j, AA, EE, 0.35, 0.5, 0.01, 0.12),
        (Kind::Boar, Call::Death) => s.call(0.8, &|t| (640.0 - 380.0 * t) * j, AA, UH, 0.4, 0.5, 0.01, 0.4),
        (Kind::Hare, Call::Idle) => {
            // sniffing
            let mut v = Vec::new();
            for g in 0..3 {
                let b = s.grains(4, 0.03, 0.015, 3500.0, 6000.0, 1.0, 1.0);
                let at = s.len(g as f32 * 0.09);
                mix(&mut v, &b, 1.0, at);
            }
            v
        }
        (Kind::Hare, Call::Hurt) => s.call(0.14, &|t| (1500.0 + 400.0 * t) * j, EE, EE, 0.2, 0.1, 0.005, 0.06),
        (Kind::Hare, Call::Death) => s.call(0.35, &|t| (1700.0 - 700.0 * t) * j, EE, EH, 0.2, 0.1, 0.005, 0.15),
        (Kind::Villager, Call::Idle) => {
            // a thoughtful "hrrm": closed mouth, the pitch up and down again
            let len = s.between(0.35, 0.55);
            let bend = s.between(0.15, 0.3);
            s.call(len, &|t| 135.0 * j * (1.0 + bend * (t * std::f32::consts::PI).sin()), MM, up(MM, 1.1), 0.12, 0.15, 0.04, 0.15)
        }
        (Kind::Villager, Call::Hurt) => s.call(0.3, &|t| (230.0 - 70.0 * t) * j, UH, MM, 0.3, 0.2, 0.01, 0.12),
        (Kind::Villager, Call::Death) => s.call(0.9, &|t| (190.0 - 100.0 * t) * j, UH, MM, 0.45, 0.25, 0.02, 0.5),
    }
}

/// Every kind of creature, for building their voices.
const KINDS: [Kind; 9] = [Kind::Deer, Kind::Boar, Kind::Hare, Kind::Wolf, Kind::Zombie, Kind::Skeleton, Kind::Spider, Kind::Slime, Kind::Villager];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_sound_is_made_and_in_range() {
        let b = build_bank(48000.0);
        let all = b.sounds.iter().flat_map(|o| o.1.iter());
        for (i, s) in all.enumerate() {
            assert!(s.len() > 100, "sound {i} is empty");
            let peak = s.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            assert!(peak.is_finite() && peak > 0.05 && peak <= 1.0, "sound {i} peak {peak}");
        }
        assert_eq!(b.sounds.len(), 8 * 4 + 9 * 3 + 16);
    }
}
