//! 16-bit stereo PCM WAV image (`WavWriter.h`).

/// Builds a WAV file exactly as `WavWriter` writes it (44-byte header).
pub fn wav_bytes(interleaved_stereo: &[i16], sample_rate: u32) -> Vec<u8> {
    let data_bytes = (interleaved_stereo.len() * 2) as u32;
    let mut o = Vec::with_capacity(44 + data_bytes as usize);
    o.extend_from_slice(b"RIFF");
    o.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    o.extend_from_slice(b"WAVE");
    o.extend_from_slice(b"fmt ");
    o.extend_from_slice(&16u32.to_le_bytes());
    o.extend_from_slice(&1u16.to_le_bytes());
    o.extend_from_slice(&2u16.to_le_bytes());
    o.extend_from_slice(&sample_rate.to_le_bytes());
    o.extend_from_slice(&(sample_rate * 4).to_le_bytes());
    o.extend_from_slice(&4u16.to_le_bytes());
    o.extend_from_slice(&16u16.to_le_bytes());
    o.extend_from_slice(b"data");
    o.extend_from_slice(&data_bytes.to_le_bytes());
    for s in interleaved_stereo {
        o.extend_from_slice(&s.to_le_bytes());
    }
    o
}

/// The PCM of a WAV written by [`wav_bytes`] / AWE32Emu (data after the
/// 44-byte header).
pub fn wav_pcm(wav: &[u8]) -> Vec<i16> {
    wav.get(44..)
        .unwrap_or(&[])
        .chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]))
        .collect()
}
