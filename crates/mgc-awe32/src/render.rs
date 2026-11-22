//! The offline render of `main.cpp`: the `--conf` file (`LoadConf` /
//! `ApplyConf`), the master volume and channel mask, the bank load order,
//! and the `--wav` render loop with its latency cut and tail.

use crate::core::Emu8000Core;
use crate::driver::Driver;
use crate::midi::{Sequencer, parse_smf, parse_xmi};
use crate::soundfont::Bank;
use crate::synth::Synth;

/// Output sample rate (main.cpp:565 `kSampleRate`).
pub const SAMPLE_RATE: u32 = 44100;
/// main.cpp:566 `kFramesPerBuffer`
pub const FRAMES_PER_BUFFER: u32 = 1024;
/// main.cpp:567 `kTailSeconds`
pub const TAIL_SECONDS: f64 = 1.5;
/// main.cpp:282 default `--ram` (KB).
pub const DEFAULT_RAM_KB: i32 = 8192;

/// Where a bank's bytes come from.
#[derive(Clone, Copy)]
pub enum BankSource<'a> {
    /// What `--sf <file>` loads: an `AIL3MDI` driver (its compiled-in GM
    /// tables), or a RIFF SoundFont (SF1 `.SBK` / SF2).
    File(&'a [u8]),
    /// The SBAWE32.MDI GM tables found by signature scan in any bytes,
    /// without the `AIL3MDI` magic check - e.g. MC1's CARPET.EXE or
    /// HIDDEN.EXE, which embed the same tables.
    MdiTables(&'a [u8]),
}

/// One `--sf` / `--rombank` argument.
#[derive(Clone, Copy)]
pub struct BankSpec<'a> {
    pub source: BankSource<'a>,
    /// `--rombank`: the bank only describes the wave ROM contents.
    pub samples_in_rom: bool,
    /// `file@N`: move bank-0 presets to MIDI bank N (-1 = keep).
    pub midi_bank: i32,
}

impl<'a> BankSpec<'a> {
    /// A plain `--sf <file>`.
    pub fn sf(bytes: &'a [u8]) -> Self {
        BankSpec {
            source: BankSource::File(bytes),
            samples_in_rom: false,
            midi_bank: -1,
        }
    }
    /// The MDI GM tables scanned out of arbitrary bytes.
    pub fn mdi_tables(bytes: &'a [u8]) -> Self {
        BankSpec {
            source: BankSource::MdiTables(bytes),
            samples_in_rom: false,
            midi_bank: -1,
        }
    }
}

/// The score to play.
#[derive(Clone, Copy)]
pub enum Score<'a> {
    /// An XMI file (first `EVNT` chunk; fixed 120 Hz clock).
    Xmi(&'a [u8]),
    /// A Standard MIDI File, format 0/1.
    Smf(&'a [u8]),
}

/// Everything `AWE32Emu <score> --rom .. --sf .. --driver .. --conf ..
/// --master-volume .. --tracks .. --wav` takes.
#[derive(Clone)]
pub struct RenderOptions<'a> {
    /// `--rom`: the raw 1 MB wave ROM dump (awe32.raw).
    pub rom: &'a [u8],
    /// `--sf` banks in load order (later ones override earlier ones).
    pub banks: Vec<BankSpec<'a>>,
    /// `--driver`
    pub driver: Driver,
    /// `--conf` file text.
    pub conf: Option<&'a str>,
    /// `--master-volume` (0..127). `None` = the conf's `master_volume`,
    /// else 127.
    pub master_volume: Option<i32>,
    /// `--tracks` as a bit mask (bit 0 = MIDI channel 1); see [`parse_tracks`].
    pub channel_mask: u16,
    /// `--ram` in KB.
    pub ram_kb: i32,
    /// `--trace`: also record the port-write trace (the C++ then runs
    /// `PowerOnInit` twice, which does not change the PCM).
    pub trace: bool,
}

impl<'a> RenderOptions<'a> {
    pub fn new(rom: &'a [u8], driver: Driver) -> Self {
        RenderOptions {
            rom,
            banks: Vec::new(),
            driver,
            conf: None,
            master_volume: None,
            channel_mask: 0xFFFF,
            ram_kb: DEFAULT_RAM_KB,
            trace: false,
        }
    }
}

/// The result of [`render`].
pub struct Rendered {
    /// Interleaved stereo i16 at 44100 Hz, exactly the `--wav` data.
    pub pcm: Vec<i16>,
    /// The `--trace` text when requested.
    pub trace: Option<String>,
    /// The DRAM image (the `--trace` `.dram.raw`).
    pub dram: Vec<i16>,
    /// Banks that failed to load (the C++ warns and goes on).
    pub warnings: Vec<String>,
}

/// One message of the conf file (main.cpp:139 `ConfMessage`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfMessage {
    Control(i32, i32),
    Program(i32),
    Bend(i32),
}

/// A parsed `--conf` file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Conf {
    pub messages: Vec<ConfMessage>,
    /// `master_volume`, -1 when absent.
    pub master: i32,
    pub trigger_mute: bool,
}

/// `istream >> int`: leading whitespace was split off; an optional sign and
/// digits, the rest of the token ignored.
fn parse_int(tok: Option<&str>) -> Option<i32> {
    let t = tok?;
    let b = t.as_bytes();
    let mut i = 0;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    let ds = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i == ds {
        return None;
    }
    t[..i]
        .parse::<i64>()
        .ok()
        .map(|v| v.clamp(i32::MIN as i64, i32::MAX as i64) as i32)
}

impl Conf {
    /// `LoadConf`, main.cpp:149.
    pub fn parse(text: &str) -> Result<Conf, String> {
        let mut c = Conf {
            messages: Vec::new(),
            master: -1,
            trigger_mute: false,
        };
        for (n, line) in text.lines().enumerate() {
            let line_no = n + 1;
            let line = match line.find('#') {
                Some(h) => &line[..h],
                None => line,
            };
            let mut it = line.split_whitespace();
            let Some(word) = it.next() else { continue };
            match word {
                "cc" => {
                    let (Some(a), Some(b)) = (parse_int(it.next()), parse_int(it.next())) else {
                        return Err(format!(
                            "{}: `cc` wants a controller number and a value",
                            line_no
                        ));
                    };
                    c.messages.push(ConfMessage::Control(a, b));
                }
                "program" => {
                    let Some(a) = parse_int(it.next()) else {
                        return Err(format!("{}: `program` wants a program number", line_no));
                    };
                    c.messages.push(ConfMessage::Program(a));
                }
                "bend" => {
                    let Some(a) = parse_int(it.next()) else {
                        return Err(format!("{}: `bend` wants a value 0..16383", line_no));
                    };
                    c.messages.push(ConfMessage::Bend(a));
                }
                "trigger_mute" => {
                    let Some(v) = parse_int(it.next()) else {
                        return Err(format!("{}: `trigger_mute` wants 0 or 1", line_no));
                    };
                    c.trigger_mute = v != 0;
                }
                "master_volume" => match parse_int(it.next()) {
                    Some(m) if (0..=127).contains(&m) => c.master = m,
                    _ => return Err(format!("{}: `master_volume` wants a value 0..127", line_no)),
                },
                _ => return Err(format!("{}: unknown command '{}'", line_no, word)),
            }
        }
        Ok(c)
    }

    /// `ApplyConf`, main.cpp:228: every message on all 16 channels.
    pub fn apply(&self, synth: &mut Synth) {
        for ch in 0..16u8 {
            for m in &self.messages {
                match *m {
                    ConfMessage::Control(a, b) => synth.control_change(ch, a as u8, b as u8),
                    ConfMessage::Program(a) => synth.program_change(ch, a as u8),
                    ConfMessage::Bend(a) => synth.pitch_bend(ch, (a - 8192) as i16),
                }
            }
        }
    }
}

/// `--tracks` (main.cpp:337): `1,2,3` = only these MIDI channels (1..16),
/// `-8,-9` = all but these; both = white list first.
pub fn parse_tracks(list: &str) -> u16 {
    let (mut allow, mut deny) = (0u16, 0u16);
    for part in list.split(',') {
        // std::atoi
        let n = parse_int(Some(part.trim_start())).unwrap_or(0);
        if (1..=16).contains(&n) {
            allow |= 1 << (n - 1);
        }
        if (-16..=-1).contains(&n) {
            deny |= 1 << (-n - 1);
        }
    }
    (if allow != 0 { allow } else { 0xFFFF }) & !deny
}

/// Renders a score exactly like the AWE32Emu `--wav` render with the 86box
/// chip (main.cpp:556-890).
pub fn render(opts: &RenderOptions, score: Score) -> Result<Rendered, String> {
    let sequence = match score {
        Score::Xmi(b) => parse_xmi(b)?,
        Score::Smf(b) => parse_smf(b)?,
    };
    let conf = match opts.conf {
        Some(t) => Some(Conf::parse(t)?),
        None => None,
    };
    let mut master_volume = 127;
    if let Some(c) = &conf {
        if c.master >= 0 {
            master_volume = c.master;
        }
    }
    if let Some(m) = opts.master_volume {
        master_volume = m.clamp(0, 127);
    }

    let core = Emu8000Core::new(opts.rom, opts.ram_kb, opts.driver)?;
    let mut synth = Synth::new(core);

    let mut warnings = Vec::new();
    for spec in &opts.banks {
        let bank = match spec.source {
            BankSource::File(b) => Bank::load(b),
            BankSource::MdiTables(b) => Bank::load_mdi_tables(b),
        };
        match bank {
            Ok(bank) => synth.load_bank(bank, spec.samples_in_rom, spec.midi_bank),
            Err(e) => warnings.push(format!("could not load bank: {}", e)),
        }
    }

    synth.core().copy_dram_to_chip();
    synth.core().power_on_init();
    synth.set_channel_mask(opts.channel_mask);
    synth.set_master_volume(master_volume);

    if opts.trace {
        synth.core().open_trace();
        synth.core().power_on_init();
    }
    if let Some(c) = &conf {
        c.apply(&mut synth);
    }

    let mut sequencer = Sequencer::new(sequence);
    sequencer.set_trigger_mute(conf.as_ref().is_some_and(|c| c.trigger_mute));

    let latency = synth.core().chip_latency_frames();
    let tail_blocks = ((TAIL_SECONDS * SAMPLE_RATE as f64) / FRAMES_PER_BUFFER as f64) as u32 + 1;
    let mut block = vec![0i16; FRAMES_PER_BUFFER as usize * 2];
    let mut pcm = Vec::new();
    let mut skip = latency;
    let mut write_trimmed = |buf: &[i16], pcm: &mut Vec<i16>| {
        let frames = (buf.len() / 2) as u32;
        if skip >= frames {
            skip -= frames;
            return;
        }
        pcm.extend_from_slice(&buf[skip as usize * 2..]);
        skip = 0;
    };
    while sequencer.has_more_events() {
        sequencer.render_block(&mut synth, &mut block, SAMPLE_RATE);
        write_trimmed(&block, &mut pcm);
    }
    let extra = tail_blocks + (latency + FRAMES_PER_BUFFER - 1) / FRAMES_PER_BUFFER;
    for _ in 0..extra {
        sequencer.render_block(&mut synth, &mut block, SAMPLE_RATE);
        write_trimmed(&block, &mut pcm);
    }
    let trace = synth.core().take_trace();
    let dram = std::mem::take(&mut synth.core().dram);
    Ok(Rendered {
        pcm,
        trace,
        dram,
        warnings,
    })
}
