//! Output backend: a cpal stream rendering 32 sample channels plus
//! one music stream. The channels are deliberately dumb — volume and
//! pan arrive as absolute values from the mixer (which runs the
//! original's per-tick fade ramps itself), samples are 8-bit unsigned
//! mono PCM resampled by linear interpolation, and the music stream
//! is decoded PCM handed over whole.
//!
//! Everything crosses the realtime boundary through a lock-free-ish
//! mpsc channel; the callback never allocates or blocks (Arc drops of
//! replaced buffers are the one small exception, accepted for
//! simplicity at this scale).

use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender};

/// Number of driver sample channels in the original (remc1's
/// word_CBFF0 table).
pub const CHANNELS: usize = 32;

/// Linear interpolation between two i16 PCM samples. The music/speech
/// FLAC rate (44100/22050) rarely matches the device rate, and reading
/// the nearest source frame (zero-order hold) stair-steps the waveform
/// into a harsh high-overtone buzz that reads as "grainy, low-bit-depth"
/// audio; interpolating removes it (deliberate; same treatment the SFX
/// lane gets).
#[inline]
fn lerp_i16(a: i16, b: i16, t: f32) -> f32 {
    let a = f32::from(a);
    a + (f32::from(b) - a) * t
}

/// Commands into the audio callback.
pub enum Cmd {
    /// Start `pcm` on channel `ch` (replacing whatever runs there).
    Play {
        ch: usize,
        pcm: Arc<Vec<u8>>,
        sample_rate: u32,
        /// 0..=0x7FFF linear.
        vol: u16,
        /// 0..=0xFFFF, 0x7FFF center.
        pan: u16,
        looped: bool,
    },
    Stop {
        ch: usize,
    },
    SetVol {
        ch: usize,
        vol: u16,
    },
    /// Replace the music stream (interleaved i16, `channels` wide).
    /// `overlay` is the sample-aligned danger stem, mixed on top at
    /// [`Cmd::MusicOverlayGain`]'s level from the same play position.
    Music {
        pcm: Arc<Vec<i16>>,
        overlay: Option<Arc<Vec<i16>>>,
        channels: u16,
        sample_rate: u32,
        looped: bool,
    },
    /// Danger-stem gain, 0..=1 (the mixer runs the original's CC7
    /// ramp and sends the result here).
    MusicOverlayGain {
        gain: f32,
    },
    StopMusic,
    /// One-shot voiceover stream (interleaved i16), on its own lane —
    /// unaffected by the duck gain, never looped.
    Speech {
        pcm: Arc<Vec<i16>>,
        channels: u16,
        sample_rate: u32,
    },
    StopSpeech,
    /// Voiceover duck: multiplies music AND sfx (retail drops both to
    /// 1/3 while a line plays, then fades back up — EF:41069/41103).
    Duck {
        gain: f32,
    },
    /// Master gains, 0..=1 linear.
    MasterVol {
        sfx: f32,
        music: f32,
    },
    /// Freeze the whole output (game pause: retail suspends ALL
    /// sound). Channels and music hold their positions and the
    /// device streams silence until resumed.
    Suspend {
        on: bool,
    },
}

struct Channel {
    pcm: Option<Arc<Vec<u8>>>,
    /// Fixed-point position/step, 32.32.
    pos: u64,
    step: u64,
    vol: f32,
    pan: f32,
    looped: bool,
    /// Declick release envelope: `Some(g)` = the voice is ramping out
    /// after a `Stop` (remaining gain `g`, 1.0 → 0.0), then it clears.
    /// A hard `pcm = None` cut mid-waveform steps the output to zero in
    /// one sample — an audible click; the meteor's fire trail restarts
    /// the same voice ~24×/s, so those clicks stack into a crackle. A
    /// ~2.5 ms fade removes them without touching the (faithful)
    /// retrigger cadence (deliberate). `None` = playing normally.
    release: Option<f32>,
}

struct MusicState {
    pcm: Option<Arc<Vec<i16>>>,
    overlay: Option<Arc<Vec<i16>>>,
    overlay_gain: f32,
    channels: u16,
    pos: u64,
    step: u64,
    looped: bool,
    /// Declick release for StopMusic / track replacement: `Some(g)` =
    /// ramping out; at 0 the stream clears (and `pending` installs).
    /// A hard `pcm = None` mid-waveform is an audible click/thump —
    /// the same ~2.5 ms ramp the SFX lane uses (deliberate).
    release: Option<f32>,
    /// The next track, installed once the release ramp completes.
    #[allow(clippy::type_complexity)]
    pending: Option<(Arc<Vec<i16>>, Option<Arc<Vec<i16>>>, u16, u64, bool)>,
}

pub struct Renderer {
    rx: Receiver<Cmd>,
    channels: Vec<Channel>,
    music: MusicState,
    /// Voiceover lane — a second decoded stream, one-shot.
    speech: MusicState,
    sfx_gain: f32,
    music_gain: f32,
    /// Voiceover duck on music+sfx (speech itself is exempt).
    duck_gain: f32,
    out_rate: f64,
    /// Per-sample gain decrement for the ~2.5 ms declick release ramp
    /// (see [`Channel::release`]).
    release_step: f32,
    /// Game pause: stream silence, hold every play position.
    suspended: bool,
    /// The pause edge ease (deliberate: retail mutes instantly): 1 =
    /// running, 0 = fully muted.
    suspend_gain: f32,
}

impl Renderer {
    pub fn new(rx: Receiver<Cmd>, out_rate: u32) -> Self {
        Renderer {
            rx,
            channels: (0..CHANNELS)
                .map(|_| Channel {
                    pcm: None,
                    pos: 0,
                    step: 0,
                    vol: 0.0,
                    pan: 0.5,
                    looped: false,
                    release: None,
                })
                .collect(),
            music: MusicState {
                pcm: None,
                overlay: None,
                overlay_gain: 0.0,
                channels: 2,
                pos: 0,
                step: 0,
                looped: false,
                release: None,
                pending: None,
            },
            speech: MusicState {
                pcm: None,
                overlay: None,
                overlay_gain: 0.0,
                channels: 2,
                pos: 0,
                step: 0,
                looped: false,
                release: None,
                pending: None,
            },
            sfx_gain: 1.0,
            music_gain: 1.0,
            duck_gain: 1.0,
            out_rate: f64::from(out_rate),
            // ~2.5 ms release: long enough to remove the step-cut click,
            // short enough not to smear the retriggered attack.
            release_step: 1.0 / (f64::from(out_rate) * 0.0025).max(1.0) as f32,
            suspended: false,
            suspend_gain: 1.0,
        }
    }

    /// True while `ch` still has samples to play.
    fn channel_live(ch: &Channel) -> bool {
        ch.pcm
            .as_ref()
            .is_some_and(|p| ch.looped || (ch.pos >> 32) < p.len() as u64)
    }

    fn drain_cmds(&mut self) {
        // Stops on Empty or Disconnected alike; a torn-down game
        // thread just leaves the mixer running out its tail.
        while let Ok(cmd) = self.rx.try_recv() {
            self.apply(cmd);
        }
    }

    fn apply(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Play {
                ch,
                pcm,
                sample_rate,
                vol,
                pan,
                looped,
            } => {
                let c = &mut self.channels[ch];
                c.step = ((f64::from(sample_rate) / self.out_rate) * (1u64 << 32) as f64) as u64;
                c.pcm = Some(pcm);
                c.pos = 0;
                c.vol = f32::from(vol) / 32767.0;
                c.pan = f32::from(pan) / 65535.0;
                c.looped = looped;
                c.release = None;
            }
            // Ramp out over ~2.5 ms instead of a hard cut (declick);
            // `render` clears `pcm` when the ramp completes. A channel
            // already releasing keeps its lower gain rather than jumping
            // back to full.
            Cmd::Stop { ch } => {
                let c = &mut self.channels[ch];
                if c.pcm.is_some() && c.release.is_none() {
                    c.release = Some(1.0);
                }
            }
            Cmd::SetVol { ch, vol } => self.channels[ch].vol = f32::from(vol) / 32767.0,
            Cmd::Music {
                pcm,
                overlay,
                channels,
                sample_rate,
                looped,
            } => {
                let step = ((f64::from(sample_rate) / self.out_rate) * (1u64 << 32) as f64) as u64;
                if self.music.pcm.is_some() {
                    // Replace: ramp the playing track out first, then
                    // install (declick).
                    self.music.pending = Some((pcm, overlay, channels.max(1), step, looped));
                    if self.music.release.is_none() {
                        self.music.release = Some(1.0);
                    }
                } else {
                    self.music.step = step;
                    self.music.pcm = Some(pcm);
                    self.music.overlay = overlay;
                    self.music.channels = channels.max(1);
                    self.music.pos = 0;
                    self.music.looped = looped;
                    self.music.release = None;
                    self.music.pending = None;
                }
            }
            Cmd::MusicOverlayGain { gain } => self.music.overlay_gain = gain,
            Cmd::StopMusic => {
                // Ramp out instead of the hard cut.
                self.music.pending = None;
                if self.music.pcm.is_some() && self.music.release.is_none() {
                    self.music.release = Some(1.0);
                }
            }
            Cmd::Speech {
                pcm,
                channels,
                sample_rate,
            } => {
                self.speech.step =
                    ((f64::from(sample_rate) / self.out_rate) * (1u64 << 32) as f64) as u64;
                self.speech.pcm = Some(pcm);
                self.speech.channels = channels.max(1);
                self.speech.pos = 0;
            }
            Cmd::StopSpeech => self.speech.pcm = None,
            Cmd::Duck { gain } => self.duck_gain = gain,
            Cmd::MasterVol { sfx, music } => {
                self.sfx_gain = sfx;
                self.music_gain = music;
            }
            Cmd::Suspend { on } => self.suspended = on,
        }
    }

    /// Fill an interleaved stereo f32 buffer.
    pub fn render(&mut self, out: &mut [f32]) {
        self.drain_cmds();
        if self.suspended && self.suspend_gain <= 0.0 {
            // Game pause: silence, positions held (retail suspends
            // ALL sound and resumes where it left off). The edge in
            // and out is eased by `suspend_gain` below.
            out.fill(0.0);
            return;
        }
        for frame in out.chunks_exact_mut(2) {
            let (mut l, mut r) = (0.0f32, 0.0f32);
            for ch in &mut self.channels {
                let Some(pcm) = ch.pcm.as_ref() else { continue };
                let len = pcm.len() as u64;
                if len == 0 {
                    continue;
                }
                let mut idx = ch.pos >> 32;
                if idx >= len {
                    if ch.looped {
                        ch.pos %= len << 32;
                        idx = ch.pos >> 32;
                    } else {
                        ch.pcm = None;
                        continue;
                    }
                }
                let frac = (ch.pos & 0xFFFF_FFFF) as f32 / 4294967296.0;
                let s0 = f32::from(pcm[idx as usize]) - 128.0;
                let s1 = f32::from(
                    pcm[if idx + 1 < len {
                        idx as usize + 1
                    } else if ch.looped {
                        0
                    } else {
                        idx as usize
                    }],
                ) - 128.0;
                let rel = ch.release.unwrap_or(1.0);
                let s =
                    (s0 + (s1 - s0) * frac) / 128.0 * ch.vol * self.sfx_gain * self.duck_gain * rel;
                l += s * (1.0 - ch.pan);
                r += s * ch.pan;
                ch.pos += ch.step;
                // Advance the declick release; drop the voice once silent.
                if let Some(g) = ch.release.as_mut() {
                    *g -= self.release_step;
                    if *g <= 0.0 {
                        ch.pcm = None;
                        ch.release = None;
                    }
                }
            }
            let mut music_done = false;
            if let Some(pcm) = self.music.pcm.as_ref() {
                let chans = self.music.channels as u64;
                let frames = pcm.len() as u64 / chans;
                let mut idx = self.music.pos >> 32;
                if idx >= frames {
                    if self.music.looped && frames > 0 {
                        self.music.pos %= frames << 32;
                        idx = self.music.pos >> 32;
                    } else {
                        music_done = true;
                    }
                }
                if !music_done {
                    // Interpolate between the current and next frame
                    // (wrapping when looped) — nearest-neighbor here is
                    // the "grainy" music artifact ([`lerp_i16`]).
                    let frac = (self.music.pos & 0xFFFF_FFFF) as f32 / 4294967296.0;
                    let next = if idx + 1 < frames {
                        idx + 1
                    } else if self.music.looped {
                        0
                    } else {
                        idx
                    };
                    let at = (idx * chans) as usize;
                    let nx = (next * chans) as usize;
                    let (mut ml, mut mr) = if chans >= 2 {
                        (
                            lerp_i16(pcm[at], pcm[nx], frac),
                            lerp_i16(pcm[at + 1], pcm[nx + 1], frac),
                        )
                    } else {
                        let s = lerp_i16(pcm[at], pcm[nx], frac);
                        (s, s)
                    };
                    if let Some(ov) = self.music.overlay.as_ref() {
                        // The danger stem is baked sample-aligned with
                        // the base (stem.len() == pcm.len()); the length
                        // guard keeps both frame reads in range.
                        if self.music.overlay_gain > 0.0 && ov.len() >= pcm.len() {
                            let (ol, or_) = if chans >= 2 {
                                (
                                    lerp_i16(ov[at], ov[nx], frac),
                                    lerp_i16(ov[at + 1], ov[nx + 1], frac),
                                )
                            } else {
                                let s = lerp_i16(ov[at], ov[nx], frac);
                                (s, s)
                            };
                            ml += ol * self.music.overlay_gain;
                            mr += or_ * self.music.overlay_gain;
                        }
                    }
                    let rel = self.music.release.unwrap_or(1.0);
                    l += ml / 32768.0 * self.music_gain * self.duck_gain * rel;
                    r += mr / 32768.0 * self.music_gain * self.duck_gain * rel;
                    self.music.pos += self.music.step;
                    // Advance the release; at silence, clear (and
                    // install the pending replacement — declick).
                    if let Some(g) = self.music.release.as_mut() {
                        *g -= self.release_step;
                        if *g <= 0.0 {
                            music_done = true;
                        }
                    }
                }
            }
            if music_done {
                self.music.pcm = None;
                self.music.overlay = None;
                self.music.release = None;
                if let Some((pcm, overlay, channels, step, looped)) = self.music.pending.take() {
                    self.music.pcm = Some(pcm);
                    self.music.overlay = overlay;
                    self.music.channels = channels;
                    self.music.step = step;
                    self.music.pos = 0;
                    self.music.looped = looped;
                }
            }
            // The voiceover lane: one-shot, duck-exempt.
            let mut speech_done = false;
            if let Some(pcm) = self.speech.pcm.as_ref() {
                let chans = self.speech.channels as u64;
                let frames = pcm.len() as u64 / chans;
                let idx = self.speech.pos >> 32;
                if idx >= frames {
                    speech_done = true;
                } else {
                    // Interpolate (one-shot, no loop) — same anti-grain
                    // treatment as the music lane.
                    let frac = (self.speech.pos & 0xFFFF_FFFF) as f32 / 4294967296.0;
                    let next = if idx + 1 < frames { idx + 1 } else { idx };
                    let at = (idx * chans) as usize;
                    let nx = (next * chans) as usize;
                    let (sl, sr) = if chans >= 2 {
                        (
                            lerp_i16(pcm[at], pcm[nx], frac),
                            lerp_i16(pcm[at + 1], pcm[nx + 1], frac),
                        )
                    } else {
                        let s = lerp_i16(pcm[at], pcm[nx], frac);
                        (s, s)
                    };
                    l += sl / 32768.0;
                    r += sr / 32768.0;
                    self.speech.pos += self.speech.step;
                }
            }
            if speech_done {
                self.speech.pcm = None;
            }
            // Suspend edge ease (~2.5 ms): ramp toward mute on pause
            // and back on resume; playback positions drift a few ms
            // during the ramp, then hold.
            let target = if self.suspended { 0.0 } else { 1.0 };
            if self.suspend_gain != target {
                self.suspend_gain = if self.suspended {
                    (self.suspend_gain - self.release_step).max(0.0)
                } else {
                    (self.suspend_gain + self.release_step).min(1.0)
                };
            }
            frame[0] = (l * self.suspend_gain).clamp(-1.0, 1.0);
            frame[1] = (r * self.suspend_gain).clamp(-1.0, 1.0);
        }
    }

    /// A voiceover clip is still playing.
    pub fn speech_live(&self) -> bool {
        self.speech.pcm.is_some()
    }

    /// Channel-liveness snapshot for the mixer (best-effort; the
    /// mixer keeps its own bookkeeping and only needs "has this
    /// one-shot finished" style answers at tick granularity).
    pub fn live_mask(&self) -> u32 {
        let mut mask = 0u32;
        for (i, ch) in self.channels.iter().enumerate() {
            if Self::channel_live(ch) {
                mask |= 1 << i;
            }
        }
        mask
    }
}

/// Handle used by the game thread.
pub struct Output {
    pub tx: Sender<Cmd>,
    /// Kept alive for the stream's lifetime.
    _stream: Option<cpal::Stream>,
    live: Arc<std::sync::atomic::AtomicU32>,
    speech_live: Arc<std::sync::atomic::AtomicBool>,
    /// Set by the stream's error callback on a fatal error kind. On
    /// Windows/WASAPI those break the worker loop and exit the stream
    /// thread (display sleep taking an HDMI endpoint, standby
    /// invalidating every client, the default endpoint changing) —
    /// the renderer and the command receiver die with it, and every
    /// later send is silently discarded. The flag is the rebuild
    /// request consumed by [`Output::reopen`].
    dead: Arc<std::sync::atomic::AtomicBool>,
    /// Bumped by every data callback — the liveness heartbeat: a
    /// counter that stops advancing means the worker thread is gone
    /// even when no error callback fired (the callback runs and
    /// renders silence even while suspended, so a live stream always
    /// beats).
    beat: Arc<std::sync::atomic::AtomicU32>,
}

impl Output {
    /// Open the default output device. Returns a silent stub when no
    /// device is available (headless runs must not fail).
    pub fn open() -> Output {
        let live = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let speech_live = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let dead = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let beat = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let (tx, stream) = Self::build(&live, &speech_live, &dead, &beat);
        if stream.is_none() {
            eprintln!("note: no audio output device — sound disabled");
        }
        Output {
            tx,
            _stream: stream,
            live,
            speech_live,
            dead,
            beat,
        }
    }

    /// Build one stream + its command channel around a fresh
    /// [`Renderer`]. Shared by [`Output::open`] and
    /// [`Output::reopen`].
    fn build(
        live: &Arc<std::sync::atomic::AtomicU32>,
        speech_live: &Arc<std::sync::atomic::AtomicBool>,
        dead: &Arc<std::sync::atomic::AtomicBool>,
        beat: &Arc<std::sync::atomic::AtomicU32>,
    ) -> (Sender<Cmd>, Option<cpal::Stream>) {
        use cpal::traits::{DeviceTrait, HostTrait};
        let (tx, rx) = std::sync::mpsc::channel();
        let stream = (|| {
            let host = cpal::default_host();
            let device = host.default_output_device()?;
            let config = device.default_output_config().ok()?;
            let rate = config.sample_rate();
            // The mixer's native layout is interleaved stereo. Open
            // stereo when the endpoint ranges allow it; otherwise
            // take the endpoint's own channel count (5.1/7.1 or mono
            // defaults reject a 2-channel open — previously audio
            // never started on those) and adapt in the callback.
            let stereo_ok = config.channels() == 2
                || device.supported_output_configs().is_ok_and(|mut it| {
                    it.any(|c| {
                        c.channels() == 2
                            && c.min_sample_rate() <= rate
                            && rate <= c.max_sample_rate()
                    })
                });
            let out_ch = if stereo_ok {
                2
            } else {
                config.channels().max(1)
            };
            let mut renderer = Renderer::new(rx, rate);
            let live_w = live.clone();
            let speech_live_w = speech_live.clone();
            let dead_w = dead.clone();
            let beat_w = beat.clone();
            let mut scratch: Vec<f32> = Vec::new();
            let stream = device
                .build_output_stream(
                    cpal::StreamConfig {
                        channels: out_ch,
                        sample_rate: rate,
                        buffer_size: cpal::BufferSize::Default,
                    },
                    move |data: &mut [f32], _| {
                        if out_ch == 2 {
                            renderer.render(data);
                        } else {
                            // Render stereo into the scratch, then
                            // spread it: mono = the L/R average,
                            // surround = front L/R with the rest
                            // silent.
                            let ch = out_ch as usize;
                            let frames = data.len() / ch.max(1);
                            scratch.resize(frames * 2, 0.0);
                            renderer.render(&mut scratch);
                            if out_ch == 1 {
                                for f in 0..frames {
                                    data[f] = 0.5 * (scratch[2 * f] + scratch[2 * f + 1]);
                                }
                            } else {
                                data.fill(0.0);
                                for f in 0..frames {
                                    data[f * ch] = scratch[2 * f];
                                    data[f * ch + 1] = scratch[2 * f + 1];
                                }
                            }
                        }
                        beat_w.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        live_w.store(renderer.live_mask(), std::sync::atomic::Ordering::Relaxed);
                        speech_live_w
                            .store(renderer.speech_live(), std::sync::atomic::Ordering::Relaxed);
                    },
                    {
                        // ALSA warm-up chatter: cpal polls
                        // `snd_pcm_avail_delay` before the PCM is
                        // actually running, so startup emits a burst
                        // of identical EIO errors (1..~30 depending
                        // on init timing) that cpal recovers from by
                        // itself. Print each DISTINCT error once;
                        // swallow identical repeats, surfacing the
                        // count only if a different error follows.
                        // FATAL kinds additionally raise the rebuild
                        // flag — dedup must never swallow that.
                        let mut last = String::new();
                        let mut repeats = 0u32;
                        move |e: cpal::Error| {
                            use cpal::ErrorKind as K;
                            if matches!(
                                e.kind(),
                                K::DeviceNotAvailable
                                    | K::StreamInvalidated
                                    | K::HostUnavailable
                                    | K::DeviceChanged
                            ) {
                                // DeviceChanged nominally keeps the
                                // stream alive, but it plays on into
                                // the OLD endpoint — rebuilding is
                                // what follows the new default.
                                dead_w.store(true, std::sync::atomic::Ordering::Relaxed);
                            }
                            let msg = e.to_string();
                            if msg == last {
                                repeats += 1;
                                return;
                            }
                            if repeats > 0 {
                                eprintln!(
                                    "audio stream error: (previous error repeated \
                                     {repeats} more time(s))"
                                );
                            }
                            eprintln!("audio stream error: {msg}");
                            last = msg;
                            repeats = 0;
                        }
                    },
                    None,
                )
                .ok()?;
            use cpal::traits::StreamTrait;
            stream.play().ok()?;
            Some(stream)
        })();
        (tx, stream)
    }

    /// Whether the stream reported a fatal error and needs a rebuild.
    pub fn needs_reopen(&self) -> bool {
        self.dead.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// The data-callback liveness counter (see the `beat` field).
    pub fn heartbeat(&self) -> u32 {
        self.beat.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Tear down the dead stream and build a fresh one (new channel,
    /// new renderer). ALL renderer state is lost with the old
    /// callback — the caller must resend gains/suspend/duck/music
    /// afterwards. Returns false when no device came up; the rebuild
    /// flag stays set so the caller retries later (which also grants
    /// device-hotplug recovery to a session that started silent).
    pub fn reopen(&mut self) -> bool {
        self._stream = None; // release the old endpoint first
        self.dead.store(false, std::sync::atomic::Ordering::Relaxed);
        let (tx, stream) = Self::build(&self.live, &self.speech_live, &self.dead, &self.beat);
        self.tx = tx;
        let ok = stream.is_some();
        self._stream = stream;
        if !ok {
            self.dead.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        ok
    }

    pub fn live_mask(&self) -> u32 {
        self.live.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn speech_live(&self) -> bool {
        self.speech_live.load(std::sync::atomic::Ordering::Relaxed)
    }
}
