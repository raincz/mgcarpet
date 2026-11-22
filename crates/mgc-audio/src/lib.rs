//! Audio runtime for mgcarpet.
//!
//! Three layers, matching the authenticity-matrix seam:
//! - [`output`]: the dumb device backend (cpal stream, 32 sample
//!   channels + music). Knows nothing about game rules.
//! - [`mixer`]: mixing POLICY. [`mixer::FaithfulMixer`] is the ported
//!   MC1 ruleset (per-id request slots, loudest-wins, tile-driven
//!   ambient loops, per-tick fades). An enhanced distance-weighted
//!   emitter mixer lands beside it later, feeding the same backend.
//! - [`music`]: FLAC track decoding for bundle music members.
//!
//! [`Audio`] bundles the pieces for the app: open device, load an
//! audio bundle, forward sim sound events, tick the mixer at sim
//! rate, start/stop music.

pub mod mixer;
pub mod music;
pub mod output;

use std::path::Path;
use std::sync::Arc;

use mgc_formats::bundle::AudioBundle;
pub use mixer::{FaithfulMixer, Listener, Sounds, Source};

/// Sim ticks per second the per-tick ramps are calibrated against
/// (mirrors `mgc_sim::TICK_RATE_HZ` — this crate deliberately has no
/// sim dependency).
const TICK_RATE: f32 = 24.0;

/// Which baked music arrangement to play. Every track carries the FM
/// render; GM and AWE32 are present when the bake could render them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MusicPreference {
    /// The best available: AWE32, else GM, else FM.
    Best,
    /// The AdLib FM render.
    Fm,
    /// General MIDI, else FM.
    Gm,
    /// AWE32, else GM, else FM.
    Awe32,
}

impl MusicPreference {
    /// The `(file, danger stem)` this preference plays for `track`.
    fn pick(self, t: &mgc_formats::bundle::MusicTrack) -> (&String, &Option<String>) {
        let fm = (&t.file, &t.danger_file);
        let gm = t.gm_file.as_ref().map(|f| (f, &t.gm_danger_file));
        let awe = t.awe_file.as_ref().map(|f| (f, &t.awe_danger_file));
        match self {
            MusicPreference::Fm => fm,
            MusicPreference::Gm => gm.unwrap_or(fm),
            MusicPreference::Best | MusicPreference::Awe32 => awe.or(gm).unwrap_or(fm),
        }
    }
}

pub struct Audio {
    out: output::Output,
    pub mixer: FaithfulMixer,
    sounds: Option<Sounds>,
    bundle: Option<AudioBundle>,
    music_playing: Option<String>,
    /// Danger-music state: the original fades the danger layers
    /// (MIDI channels 3/4/5 of the playing song) in and out with CC7
    /// ramps of step 2 over 0..126 at rate 0x3C in / 0x14 out (remc1
    /// sub_20BD0/sub_20D00 — ~1.05 s up, ~3.15 s down). We run the
    /// same ramp at sim-tick granularity over the baked danger stem.
    danger: bool,
    danger_level: f32, // 0..126, the original's fade counter
    /// Per-game danger ramp steps per sim tick (24 Hz =
    /// `mgc_sim::TICK_RATE_HZ`) on the 0..126 counter — derived from the
    /// original's real-time timer Hz so the audible fade is
    /// tick-rate-independent. MC1: CC7 step 2 at 0x3C/0x14 Hz →
    /// `2·60/24` up / `−2·20/24` down. MC2: cc11 step ±1 at 90 Hz →
    /// `±90/24` (Sound.cpp:5877/6076).
    danger_up: f32,
    danger_down: f32,
    /// Which baked arrangement plays (see [`MusicPreference`]); the
    /// FM render is the always-present fallback.
    arrangement: MusicPreference,
    /// Voiceover duck state: retail drops music+sfx to 1/3 the
    /// instant a line starts (FadeDownSoundVolume_59A50) and ramps
    /// them back when it ends (the 120 Hz FadeUpSoundVolume timer).
    duck_gain: f32,
    /// Movie-sample voices: `(channel, sample id)` for every cue the
    /// FMV player has running. The movies' soundtrack is assembled
    /// from these — see [`Audio::play_movie_sample`].
    movie_voices: Vec<(usize, u32)>,
    /// The sample bank the movie player's `'E'` cue selected.
    movie_bank: u32,
    /// Renderer-state cache for the stream-rebuild resend: the
    /// renderer lives inside the cpal callback and dies with it on a
    /// fatal stream error (Windows standby / display sleep /
    /// default-endpoint change — the "sound never comes back after a
    /// long pause" report), so a rebuilt stream starts blank and
    /// must be re-primed (`resend_state`).
    volumes: (f32, f32, f32),
    suspended: bool,
    /// The playing track's decoded payload (Arc-cheap), for replay
    /// after a rebuild — `play_music`'s same-name guard would
    /// otherwise refuse to restart it.
    music_cmd: Option<(Arc<Vec<i16>>, Option<Arc<Vec<i16>>>, u16, u32, bool)>,
    /// The last STOPPED track's decoded payload, keyed by (name,
    /// arrangement at decode time): a music re-enable (`stop_music`
    /// then `play_music` of the same track) reuses it instead of
    /// decoding ~0.5-0.8 s of FLAC on the game thread again
    /// (player-reported startup lag, 2026-09-08). One entry: it holds
    /// exactly what was resident while the track played, so a muted
    /// game costs no more memory than a playing one.
    music_cache: Option<(
        String,
        MusicPreference,
        (Arc<Vec<i16>>, Option<Arc<Vec<i16>>>, u16, u32),
    )>,
    /// Stream watchdog: last observed heartbeat, ticks it has been
    /// stale, and the reopen retry backoff.
    last_beat: u32,
    stale_ticks: u32,
    reopen_backoff: u32,
}

impl Audio {
    /// Open the output device (silent stub when none) with no bundle
    /// loaded yet.
    pub fn open() -> Audio {
        Audio {
            out: output::Output::open(),
            mixer: FaithfulMixer::new(),
            sounds: None,
            bundle: None,
            music_playing: None,
            danger: false,
            danger_level: 0.0,
            danger_up: 2.0 * 60.0 / TICK_RATE,
            danger_down: -2.0 * 20.0 / TICK_RATE,
            arrangement: MusicPreference::Best,
            duck_gain: 1.0,
            movie_voices: Vec::new(),
            movie_bank: 0,
            volumes: (1.0, 1.0, 1.0),
            suspended: false,
            music_cmd: None,
            music_cache: None,
            last_beat: 0,
            stale_ticks: 0,
            reopen_backoff: 0,
        }
    }

    /// MC2's danger ramp: cc11 expression step ±1 at 90 Hz on the
    /// war channels (Sound.cpp:5877) → `±90/24` per 24 Hz sim tick,
    /// both directions (`24` = `mgc_sim::TICK_RATE_HZ`). Also switches
    /// the mixer to MC2's per-id sound law (`PrepareEventSound_6E450`
    /// — ids up to 69; the MC1 switch dropped everything ≥ 47).
    pub fn set_mc2_danger_ramp(&mut self) {
        self.danger_up = 90.0 / TICK_RATE;
        self.danger_down = -90.0 / TICK_RATE;
        self.mixer.set_mc2(true);
    }

    /// Pick the music arrangement (config `audio.arrangement`).
    /// Applies from the next `play_music` — the playing track is not
    /// restarted.
    pub fn set_arrangement(&mut self, arrangement: MusicPreference) {
        self.arrangement = arrangement;
    }

    /// The danger-mode wish for this tick (the original's wizard
    /// `v_46 > 0` state — armed by taking hits or being targeted).
    pub fn set_danger(&mut self, danger: bool) {
        self.danger = danger;
    }

    /// Game pause: freeze the whole output (channels + music hold
    /// their positions, the device streams silence). Retail suspends
    /// ALL sound while paused; mixer requests made meanwhile (the
    /// map-toggle ding) sit queued and flush on the first unpaused
    /// tick — the original's deferred-ding quirk (our per-id request
    /// slot plays it once even if the map toggled twice).
    pub fn set_paused(&mut self, on: bool) {
        self.suspended = on;
        let _ = self.out.tx.send(output::Cmd::Suspend { on });
    }

    /// Load an audio bundle directory (`baked/assets/<game>-audio`)
    /// and select a sample bank (0 = the gameplay bank). Re-selecting
    /// a bank out of the bundle already loaded (MC2's per-level
    /// MapType switch, `LevelInit.cpp:23/29/37` — Day 0 / Night 1 /
    /// Cave 2 — on every level install of a campaign run) costs no
    /// reload. Voices already playing keep their own sample copies.
    pub fn load_bundle(&mut self, dir: &Path, bank: u32) -> Result<(), String> {
        let bundle = match self.bundle.take() {
            Some(b) if b.dir.as_path() == dir => b,
            _ => AudioBundle::load(dir).map_err(|e| e.to_string())?,
        };
        self.sounds = Sounds::from_bundle(&bundle, bank);
        // The bundle stays loaded even without the bank: music and
        // speech do not depend on it.
        self.bundle = Some(bundle);
        if self.sounds.is_none() {
            return Err(format!("{}: no sample bank {bank}", dir.display()));
        }
        Ok(())
    }

    pub fn has_sounds(&self) -> bool {
        self.sounds.is_some()
    }

    /// Forward one sim sound event into the faithful mixer.
    pub fn event(&mut self, id: u8, source: Source, listener: &Listener) {
        if self.sounds.is_some() {
            self.mixer.request(id, source, listener);
        }
    }

    /// Stream watchdog, run once per tick: rebuild the output after
    /// a fatal stream error (the error-callback flag) or a stalled
    /// data callback (~3 s without a heartbeat — the callback beats
    /// even while suspended, so a live stream never trips it). The
    /// backoff retries a failed rebuild every ~5 s, which doubles as
    /// device-hotplug recovery for a session that started silent.
    fn watchdog(&mut self) {
        let beat = self.out.heartbeat();
        if beat != self.last_beat {
            self.last_beat = beat;
            self.stale_ticks = 0;
        } else {
            self.stale_ticks = self.stale_ticks.saturating_add(1);
        }
        self.reopen_backoff = self.reopen_backoff.saturating_sub(1);
        let dead = self.out.needs_reopen() || self.stale_ticks > (3.0 * TICK_RATE) as u32;
        if dead && self.reopen_backoff == 0 {
            self.reopen_backoff = (5.0 * TICK_RATE) as u32;
            if self.out.reopen() {
                eprintln!("audio: output stream rebuilt");
                self.stale_ticks = 0;
                self.resend_state();
            }
        }
    }

    /// Re-prime a rebuilt renderer: gains, suspend, duck, the danger
    /// overlay level and the playing music track (restarted from the
    /// top — the loop point is presentation-only). SFX voices are
    /// transient and simply re-fill from the mixer.
    fn resend_state(&mut self) {
        let (sfx, music, speech) = self.volumes;
        let _ = self
            .out
            .tx
            .send(output::Cmd::MasterVol { sfx, music, speech });
        let _ = self
            .out
            .tx
            .send(output::Cmd::Suspend { on: self.suspended });
        let _ = self.out.tx.send(output::Cmd::Duck {
            gain: self.duck_gain,
        });
        let lvl = self.danger_level / 126.0;
        let _ = self
            .out
            .tx
            .send(output::Cmd::MusicOverlayGain { gain: lvl * lvl });
        if let Some((pcm, overlay, channels, sample_rate, looped)) = &self.music_cmd {
            let _ = self.out.tx.send(output::Cmd::Music {
                pcm: pcm.clone(),
                overlay: overlay.clone(),
                channels: *channels,
                sample_rate: *sample_rate,
                looped: *looped,
            });
        }
    }

    /// Per-sim-tick flush (24 Hz = `mgc_sim::TICK_RATE_HZ` — the fade
    /// ramps are per-tick).
    pub fn tick(&mut self) {
        self.watchdog();
        if let Some(sounds) = &self.sounds {
            self.mixer.tick(sounds, &self.out.tx, self.out.live_mask());
        }
        // Danger-stem ramp on the 0..126 counter, per-game rates
        // (see `danger_up`/`danger_down`).
        let target = if self.danger { 126.0 } else { 0.0 };
        if (self.danger_level - target).abs() > f32::EPSILON {
            let step = if self.danger {
                self.danger_up
            } else {
                self.danger_down
            };
            self.danger_level = (self.danger_level + step).clamp(0.0, 126.0);
            // cc11 expression → amplitude follows the GM square law
            // (L = 40·log10(v/127) dB ⇒ amp ≈ (v/127)²). The baked
            // stem is the war channels at FULL expression, so the
            // overlay gain must ride the same curve.
            let lvl = self.danger_level / 126.0;
            let _ = self
                .out
                .tx
                .send(output::Cmd::MusicOverlayGain { gain: lvl * lvl });
        }
        // Voiceover duck recovery: once the line ends, ramp music+sfx
        // back up (retail's 120 Hz FadeUpSoundVolume ≈ 0.7 s full
        // traverse — deliberate approximation, the exact per-callback
        // step is a volume-scale detail). Step = the 1/3→1 span over
        // 0.7 s of 24 Hz sim ticks (`0.7·24` = 16.8 ticks) so the
        // recovery time is tick-rate-independent.
        if self.duck_gain < 1.0 && !self.out.speech_live() {
            self.duck_gain = (self.duck_gain + (2.0 / 3.0) / (0.7 * TICK_RATE)).min(1.0);
            let _ = self.out.tx.send(output::Cmd::Duck {
                gain: self.duck_gain,
            });
        }
    }

    /// Play one voiceover clip (`CdTracks_DB080` address: table row =
    /// 0-based level number, segment slot). Ducks music+sfx to 1/3
    /// for the clip's duration; a new line interrupts the playing one
    /// (retail `PlayCDTrackSegment_86FF0` stops before starting).
    /// Missing clips (empty retail slots) are a quiet no-op.
    pub fn play_speech(&mut self, row: u32, segment: u32) -> Result<(), String> {
        let Some(bundle) = &self.bundle else {
            return Err("no audio bundle loaded".into());
        };
        let Some(index) = &bundle.speech else {
            return Err("bundle has no speech".into());
        };
        let Some(clip) = index
            .clips
            .iter()
            .find(|c| c.row == row && c.segment == segment)
        else {
            return Ok(()); // empty slot — retail no-ops on length 0
        };
        let decoded = music::decode_flac(&bundle.dir.join(&clip.file))?;
        let _ = self.out.tx.send(output::Cmd::Speech {
            pcm: decoded.pcm,
            channels: decoded.channels,
            sample_rate: decoded.sample_rate,
        });
        self.duck_gain = 1.0 / 3.0;
        let _ = self.out.tx.send(output::Cmd::Duck {
            gain: self.duck_gain,
        });
        Ok(())
    }

    /// Whether the bundle carries voiceover clips at all.
    pub fn has_speech(&self) -> bool {
        self.bundle.as_ref().is_some_and(|b| b.speech.is_some())
    }

    /// Cut every live sound effect and ambient loop — the level-
    /// boundary teardown (per-mode audio ownership: a torn-down
    /// session's wind/waves/fire must not survive under the
    /// frontend). Music and speech have their own stops.
    pub fn stop_sounds(&mut self) {
        self.mixer.reset(&self.out.tx);
    }

    /// Cut any playing voiceover mid-clip (frontend transitions —
    /// leaving the map screen must silence its narration) and
    /// restore the duck.
    pub fn stop_speech(&mut self) {
        let _ = self.out.tx.send(output::Cmd::StopSpeech);
        self.duck_gain = 1.0;
        let _ = self.out.tx.send(output::Cmd::Duck { gain: 1.0 });
    }

    /// Select the sample bank movie cues draw from — retail's `'E'`
    /// script key, which loads `SNDS<bank>-<quality>` over the
    /// previous set (remc1 `sub_5D070_5D580`). Loading a bank stops
    /// everything playing out of the old one, as retail does.
    pub fn set_movie_bank(&mut self, bank: u32) {
        if self.movie_bank != bank {
            self.stop_movie_samples();
            self.movie_bank = bank;
        }
    }

    /// Play one movie sound cue: the `'S'` (one-shot) and `'R'`
    /// (looping) script keys. `id` is the 1-based index within the
    /// selected bank, exactly as the retail tables store it.
    ///
    /// These deliberately bypass the gameplay mixer: that is a ported
    /// 3-D ruleset with per-id request slots and a listener, and a
    /// movie has neither a world nor a listener — retail plays these
    /// straight onto voices too.
    ///
    /// Safe to share the channel pool with it: no session is alive
    /// during a movie, and even if one were, [`mixer::FaithfulMixer`]
    /// allocates only channels that are both unkeyed AND absent from
    /// the output's live mask, so a sounding movie voice cannot be
    /// stolen. Voices are taken from the top of the range, away from
    /// the mixer's allocation order.
    pub fn play_movie_sample(&mut self, id: u32, looped: bool) -> Result<(), String> {
        self.play_bank_sample(self.movie_bank, id, looped)
    }

    /// Play sample `id` (1-based, as retail indexes it) out of `bank`
    /// on a movie voice, leaving the movie bank selection alone — for
    /// frontend screens that load their own bank (MC1's PPERF chime,
    /// bank 13).
    pub fn play_bank_sample(&mut self, bank: u32, id: u32, looped: bool) -> Result<(), String> {
        let Some(bundle) = &self.bundle else {
            return Err("no audio bundle loaded".into());
        };
        let Some((index, blob)) = &bundle.sounds else {
            return Err("bundle has no samples".into());
        };
        let entry = index
            .banks
            .iter()
            .find(|b| b.bank == bank)
            .and_then(|b| b.entries.iter().find(|e| e.id == id))
            .ok_or_else(|| format!("no sample {id} in bank {bank}"))?;
        let (at, len) = (entry.offset as usize, entry.len as usize);
        let pcm = blob
            .get(at..at + len)
            .ok_or_else(|| format!("sample {bank}:{id} overruns the blob"))?
            .to_vec();
        // Top of the channel range, away from the gameplay mixer's
        // allocation order, so a stray live voice cannot be stolen.
        let used: Vec<usize> = self.movie_voices.iter().map(|(c, _)| *c).collect();
        let Some(ch) = (0..output::CHANNELS).rev().find(|c| !used.contains(c)) else {
            return Ok(()); // all 32 voices busy — retail drops it too
        };
        let rate = index.sample_rate;
        let _ = self.out.tx.send(output::Cmd::Play {
            ch,
            pcm: std::sync::Arc::new(pcm),
            sample_rate: rate,
            vol: 0x7FFF,
            pan: 0x7FFF,
            looped,
        });
        self.movie_voices.push((ch, id));
        Ok(())
    }

    /// Stop movie cues playing sample `id` — the `'T'` script key.
    pub fn stop_movie_sample(&mut self, id: u32) {
        self.movie_voices.retain(|&(ch, playing)| {
            if playing == id {
                let _ = self.out.tx.send(output::Cmd::Stop { ch });
                false
            } else {
                true
            }
        });
    }

    /// Stop every movie cue — `'S'`/`'T'` with index 0, and the end
    /// of a movie.
    pub fn stop_movie_samples(&mut self) {
        for (ch, _) in std::mem::take(&mut self.movie_voices) {
            let _ = self.out.tx.send(output::Cmd::Stop { ch });
        }
    }

    /// Play a bundle music track by name (`cgame1`, `track-02`),
    /// looped. No-op if it is already the one playing.
    pub fn play_music(&mut self, name: &str, looped: bool) -> Result<(), String> {
        if self.music_playing.as_deref() == Some(name) {
            return Ok(());
        }
        let Some(bundle) = &self.bundle else {
            return Err("no audio bundle loaded".into());
        };
        let Some(index) = &bundle.music else {
            return Err("bundle has no music".into());
        };
        let Some(track) = index.tracks.iter().find(|t| t.name == name) else {
            return Err(format!("no music track named {name}"));
        };
        let (pcm, overlay, channels, sample_rate) = match self.music_cache.take() {
            Some((cached, arr, payload)) if cached == name && arr == self.arrangement => payload,
            _ => {
                let (file, danger_file) = self.arrangement.pick(track);
                let decoded = music::decode_flac(&bundle.dir.join(file))?;
                let overlay = match danger_file {
                    Some(f) => Some(music::decode_flac(&bundle.dir.join(f))?.pcm),
                    None => None,
                };
                (decoded.pcm, overlay, decoded.channels, decoded.sample_rate)
            }
        };
        self.music_cmd = Some((pcm.clone(), overlay.clone(), channels, sample_rate, looped));
        let _ = self.out.tx.send(output::Cmd::Music {
            pcm,
            overlay,
            channels,
            sample_rate,
            looped,
        });
        self.music_playing = Some(name.to_string());
        Ok(())
    }

    /// Stop the track. Its decoded payload is kept (one entry) so
    /// re-enabling the same track is instant — see `music_cache`.
    pub fn stop_music(&mut self) {
        let _ = self.out.tx.send(output::Cmd::StopMusic);
        if let (Some(name), Some((pcm, overlay, ch, rate, _))) =
            (self.music_playing.take(), self.music_cmd.take())
        {
            self.music_cache = Some((name, self.arrangement, (pcm, overlay, ch, rate)));
        }
        self.music_playing = None;
        self.music_cmd = None;
    }

    /// Master gains, 0..=1: sound effects, music, and the narration
    /// lane (the app ties the last to the sound toggle + sfx gain —
    /// PLAYER-RULED 2026-09-08, retail has no narration volume).
    pub fn set_volumes(&mut self, sfx: f32, music: f32, speech: f32) {
        self.volumes = (sfx, music, speech);
        let _ = self
            .out
            .tx
            .send(output::Cmd::MasterVol { sfx, music, speech });
    }
}

#[cfg(test)]
mod tests {
    /// The arrangement fallback chain: AWE32 → GM → FM for Best and
    /// Awe32, GM → FM for Gm, FM always available.
    #[test]
    fn arrangement_preferences_fall_back_in_order() {
        use mgc_formats::bundle::MusicTrack;
        let track = |gm: bool, awe: bool| MusicTrack {
            bank: 0,
            name: "t".into(),
            file: "fm.flac".into(),
            danger_file: None,
            gm_file: gm.then(|| "gm.flac".into()),
            gm_danger_file: None,
            awe_file: awe.then(|| "awe.flac".into()),
            awe_danger_file: None,
            source: String::new(),
        };
        let all = track(true, true);
        let pick = |p: MusicPreference, t: &MusicTrack| p.pick(t).0.clone();
        assert_eq!(pick(MusicPreference::Best, &all), "awe.flac");
        assert_eq!(pick(MusicPreference::Awe32, &all), "awe.flac");
        assert_eq!(pick(MusicPreference::Gm, &all), "gm.flac");
        assert_eq!(pick(MusicPreference::Fm, &all), "fm.flac");
        let no_awe = track(true, false);
        assert_eq!(pick(MusicPreference::Best, &no_awe), "gm.flac");
        assert_eq!(pick(MusicPreference::Awe32, &no_awe), "gm.flac");
        let fm_only = track(false, false);
        assert_eq!(pick(MusicPreference::Best, &fm_only), "fm.flac");
        assert_eq!(pick(MusicPreference::Gm, &fm_only), "fm.flac");
    }

    use super::*;

    /// The baked MC2 bundle, when present (a bake is not a test
    /// prerequisite — the test skips without one).
    fn mc2_dir() -> Option<std::path::PathBuf> {
        let d = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../baked/assets/mc2-audio");
        d.is_dir().then_some(d)
    }

    /// MC2 selects its sample bank per level by MapType
    /// (`LevelInit.cpp:23/29/37`: Day 0 / Night 1 / Cave 2), so a
    /// campaign run re-selects the bank on every level install out
    /// of the bundle it already holds. Bank 2 alone carries the cave
    /// drips (65-69); bank 0's firefly slots (43/44) are a silence
    /// stub. Going 0 → 2 → 0 must track the bank exactly.
    #[test]
    fn load_bundle_reselects_the_bank_of_the_loaded_bundle() {
        let Some(dir) = mc2_dir() else {
            eprintln!("skip: no baked mc2-audio bundle");
            return;
        };
        let has = |a: &Audio, id: usize| {
            a.sounds
                .as_ref()
                .and_then(|s| s.entries.get(id))
                .is_some_and(|e| e.is_some())
        };
        let mut a = Audio::open();
        a.load_bundle(&dir, 0).unwrap();
        assert!(a.has_sounds());
        assert!(!has(&a, 65), "bank 0 has no drip samples");
        a.load_bundle(&dir, 2).unwrap();
        assert!(has(&a, 65), "the cave bank carries Drip1");
        assert!(has(&a, 69), "… through Drip5");
        a.load_bundle(&dir, 0).unwrap();
        assert!(!has(&a, 65), "back on the day bank");
        assert!(a.bundle.is_some(), "the bundle stays loaded throughout");
        // A bank the bundle lacks reports it and keeps the bundle for
        // music/speech.
        assert!(a.load_bundle(&dir, 99).is_err());
        assert!(!a.has_sounds());
        assert!(a.bundle.is_some());
    }

    /// A music re-enable (stop, then play the same track) reuses the
    /// stopped track's decoded payload instead of decoding the FLAC
    /// again on the game thread (the ~0.5-0.8 s startup lag the
    /// player heard). A different track, or a changed arrangement,
    /// decodes afresh.
    #[test]
    fn music_reenable_reuses_the_stopped_tracks_decode() {
        let Some(dir) = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../baked/assets/mc1-audio")
            .is_dir()
            .then(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../baked/assets/mc1-audio"))
        else {
            eprintln!("skip: no baked mc1-audio bundle");
            return;
        };
        let mut a = Audio::open();
        a.load_bundle(&dir, 0).unwrap();
        a.play_music("csetup", true).unwrap();
        let first = a.music_cmd.as_ref().unwrap().0.clone();
        a.stop_music();
        assert!(a.music_playing.is_none() && a.music_cmd.is_none());
        let t = std::time::Instant::now();
        a.play_music("csetup", true).unwrap();
        assert!(
            Arc::ptr_eq(&first, &a.music_cmd.as_ref().unwrap().0),
            "the same decoded buffer plays again"
        );
        assert!(t.elapsed().as_millis() < 50, "no decode: {:?}", t.elapsed());
        assert!(
            a.music_cache.is_none(),
            "the cache entry moved back into play"
        );
        // The arrangement changed under the cache: decode afresh.
        a.stop_music();
        a.set_arrangement(MusicPreference::Fm);
        a.play_music("csetup", true).unwrap();
        assert!(!Arc::ptr_eq(&first, &a.music_cmd.as_ref().unwrap().0));
    }
}
