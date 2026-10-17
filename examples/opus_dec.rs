//! opus_dec IN.bit OUT.(raw|wav) — decode an RFC 6716 conformance-vector stream (4-byte BE length, 4-byte BE final range,
//! packet) to 48 kHz stereo 16-bit PCM, like the reference's `opus_demo -d 48000 2`. Prints final-range agreement.
use omni_spec::opus::{read_vector, Decoder};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let bit = std::fs::read(&a[1]).expect("read");
    let mut d = Decoder::new();
    let (mut pcm, mut ok, mut bad, mut none) = (Vec::<i16>::new(), 0, 0, 0);
    for p in read_vector(&bit).unwrap() {
        if p.data.is_empty() { pcm.extend(std::iter::repeat(0).take(1920)); continue; }
        let r = d.decode(p.data).unwrap();
        match r.final_range { Some(f) if f == p.final_range => ok += 1, Some(_) => bad += 1, None => none += 1 }
        pcm.extend(r.pcm.iter().map(|&x| (x * 32768.0).round().clamp(-32768.0, 32767.0) as i16));
    }
    let mut out = Vec::with_capacity(pcm.len() * 2 + 44);
    if a[2].ends_with(".wav") {
        let n = (pcm.len() * 2) as u32;
        out.extend_from_slice(b"RIFF"); out.extend_from_slice(&(36 + n).to_le_bytes()); out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes()); out.extend_from_slice(&1u16.to_le_bytes()); out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&48000u32.to_le_bytes()); out.extend_from_slice(&(48000u32 * 4).to_le_bytes()); out.extend_from_slice(&4u16.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes()); out.extend_from_slice(b"data"); out.extend_from_slice(&n.to_le_bytes());
    }
    for s in pcm { out.extend_from_slice(&s.to_le_bytes()); }
    std::fs::write(&a[2], out).expect("write");
    eprintln!("final range: {ok} match, {bad} differ, {none} not decoded (CELT/Hybrid)");
}
