//! EBML reader and WebM / Matroska demuxer, streaming (bytes may arrive in any chunking, as Media Source Extensions appends them).
//!
//! Specs:
//! - RFC 8794 "Extensible Binary Meta Language" — §4 Variable-Size Integer (VINT_WIDTH zeros, VINT_MARKER, VINT_DATA), §5 Element ID
//!   (marker kept in the ID value, shortest length), §6 Element Data Size (§6.2 all VINT_DATA bits one = unknown size), §7 data types
//!   (big-endian unsigned / signed integers, 4- or 8-octet floats, strings), §11.2 EBML Header (DocType).
//! - RFC 9559 "Matroska Media Container Format Specification" — §5.1 element IDs used here (Segment 0x18538067, Info 0x1549A966,
//!   TimestampScale 0x2AD7B1 default 1,000,000 ns, Duration 0x4489, Cluster 0x1F43B675, Timestamp 0xE7, SimpleBlock 0xA3,
//!   BlockGroup 0xA0, Block 0xA1, BlockDuration 0x9B, ReferenceBlock 0xFB, DiscardPadding 0x75A2, Tracks 0x1654AE6B, TrackEntry 0xAE,
//!   TrackNumber 0xD7, TrackType 0x83, DefaultDuration 0x23E383, CodecID 0x86, CodecPrivate 0x63A2, CodecDelay 0x56AA,
//!   SeekPreRoll 0x56BB, Video 0xE0 PixelWidth 0xB0 PixelHeight 0xBA, Audio 0xE1 SamplingFrequency 0xB5 (default 8000.0)
//!   Channels 0x9F (default 1), ContentEncodings 0x6D80, Cues 0x1C53BB6B); §10.1 Block and §10.2 SimpleBlock headers (VINT track
//!   number, signed 16-bit timestamp relative to the Cluster, flags KEY / INV / LACING / DIS); §10.3 lacing (Xiph, EBML with signed
//!   VINT deltas, fixed-size); §10.3.5 the Block timestamp applies to the first frame of a lace.
//! - WebM Container Guidelines (webmproject.org): WebM is the Matroska subset with DocType "webm", codecs V_VP8 / V_VP9 / V_AV1 /
//!   A_VORBIS / A_OPUS.
//! - W3C "WebM Byte Stream Format" for MSE: an initialization segment is the EBML header + Segment header + Info + Tracks; a media
//!   segment is one Cluster; the Segment may be of unknown size.
//!
//! Timestamps are reported in nanoseconds: (Cluster Timestamp + Block relative timestamp) × TimestampScale (RFC 9559 §11.1).
//! Encrypted or compressed tracks (ContentEncodings) are reported with `encoded = true`; their frames are passed through untouched.

/// RFC 8794 §4: a VINT at the start of `b` → (value without the marker, octet length, all VINT_DATA bits set). None = need more bytes
/// (or the first octet is 0: a VINT longer than 8 octets, not allowed here).
pub fn vint(b: &[u8]) -> Option<(u64, usize, bool)> {
    let first = *b.first()?;
    if first == 0 { return None; }
    let len = first.leading_zeros() as usize + 1;
    if b.len() < len { return None; }
    let mut v = (first as u64) & (0xFFu64 >> len);
    for &x in &b[1..len] { v = (v << 8) | x as u64; }
    let ones = v == (1u64 << (7 * len)) - 1;
    Some((v, len, ones))
}

/// RFC 8794 §5: an Element ID keeps its VINT_WIDTH + marker bits (0x1A45DFA3 is the EBML header's ID as written). None = need more.
pub fn element_id(b: &[u8]) -> Option<(u32, usize)> {
    let first = *b.first()?;
    if first == 0 { return None; }
    let len = first.leading_zeros() as usize + 1;
    if len > 4 || b.len() < len { return None; }
    let mut v = 0u32;
    for &x in &b[..len] { v = (v << 8) | x as u32; }
    Some((v, len))
}

/// RFC 9559 §10.3.3: an EBML-laced size difference — the unsigned VINT value minus (2^(7n−1) − 1), n = octet length.
pub fn signed_vint(b: &[u8]) -> Option<(i64, usize)> {
    let (v, n, _) = vint(b)?;
    Some((v as i64 - ((1i64 << (7 * n - 1)) - 1), n))
}

fn uint(d: &[u8]) -> u64 { d.iter().fold(0u64, |a, &x| (a << 8) | x as u64) }
fn sint(d: &[u8]) -> i64 { if d.is_empty() { 0 } else { let s = 64 - 8 * d.len() as u32; ((uint(d) << s) as i64) >> s } }
fn float(d: &[u8]) -> f64 {
    match d.len() { 4 => f32::from_bits(uint(d) as u32) as f64, 8 => f64::from_bits(uint(d)), _ => 0.0 }
}

pub const ID_EBML: u32 = 0x1A45DFA3;
pub const ID_DOCTYPE: u32 = 0x4282;
pub const ID_SEGMENT: u32 = 0x18538067;
pub const ID_INFO: u32 = 0x1549A966;
pub const ID_TIMESTAMP_SCALE: u32 = 0x2AD7B1;
pub const ID_DURATION: u32 = 0x4489;
pub const ID_CLUSTER: u32 = 0x1F43B675;
pub const ID_TIMESTAMP: u32 = 0xE7;
pub const ID_SIMPLE_BLOCK: u32 = 0xA3;
pub const ID_BLOCK_GROUP: u32 = 0xA0;
pub const ID_BLOCK: u32 = 0xA1;
pub const ID_BLOCK_DURATION: u32 = 0x9B;
pub const ID_REFERENCE_BLOCK: u32 = 0xFB;
pub const ID_DISCARD_PADDING: u32 = 0x75A2;
pub const ID_TRACKS: u32 = 0x1654AE6B;
pub const ID_TRACK_ENTRY: u32 = 0xAE;
pub const ID_TRACK_NUMBER: u32 = 0xD7;
pub const ID_TRACK_TYPE: u32 = 0x83;
pub const ID_DEFAULT_DURATION: u32 = 0x23E383;
pub const ID_CODEC_ID: u32 = 0x86;
pub const ID_CODEC_PRIVATE: u32 = 0x63A2;
pub const ID_CODEC_DELAY: u32 = 0x56AA;
pub const ID_SEEK_PREROLL: u32 = 0x56BB;
pub const ID_VIDEO: u32 = 0xE0;
pub const ID_PIXEL_WIDTH: u32 = 0xB0;
pub const ID_PIXEL_HEIGHT: u32 = 0xBA;
pub const ID_AUDIO: u32 = 0xE1;
pub const ID_SAMPLING_FREQUENCY: u32 = 0xB5;
pub const ID_CHANNELS: u32 = 0x9F;
pub const ID_CONTENT_ENCODINGS: u32 = 0x6D80;
pub const ID_CUES: u32 = 0x1C53BB6B;

/// Segment-level elements: one of these ends an unknown-sized Cluster (RFC 8794 §6.2: a sibling of the unknown-sized element).
const SEGMENT_CHILDREN: [u32; 8] = [ID_CLUSTER, ID_CUES, ID_TRACKS, ID_INFO, 0x114D9B74 /* SeekHead */, 0x1254C367 /* Tags */,
    0x1043A770 /* Chapters */, 0x1941A469 /* Attachments */];

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Track {
    pub number: u64,
    /// RFC 9559 §5.1.4.1.3: 1 video, 2 audio, 17 subtitle, …
    pub kind: u64,
    pub codec: String,
    pub private: Vec<u8>,
    pub codec_delay_ns: u64,
    pub seek_preroll_ns: u64,
    pub default_duration_ns: Option<u64>,
    pub width: u64,
    pub height: u64,
    pub rate: f64,
    pub channels: u64,
    /// ContentEncodings present (encryption / compression): frames are not plain codec data
    pub encoded: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub track: u64,
    pub pts_ns: i64,
    pub key: bool,
    pub invisible: bool,
    pub duration_ns: Option<u64>,
    /// RFC 9559 §5.1.3.5.7: nanoseconds of decoded audio to drop at the end (Opus end trimming)
    pub discard_padding_ns: Option<i64>,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// the EBML header's DocType ("webm" / "matroska")
    Header(String),
    Info { timestamp_scale: u64, duration_ns: Option<f64> },
    Tracks(Vec<Track>),
    /// a Cluster began at this time (MSE: a media segment)
    Cluster(i64),
    Frame(Frame),
    /// the bytes are not a valid stream (MSE: append error); the demuxer stops until `reset`
    Error(String),
}

/// Streaming demuxer: `push` any chunk, get the events completed so far.
pub struct Demuxer {
    buf: Vec<u8>,
    /// absolute stream offset of buf[0]
    base: u64,
    /// open master elements we descended into: (id, absolute end; None = unknown size)
    stack: Vec<(u32, Option<u64>)>,
    /// bytes of an uninteresting element still to drop (may span chunks)
    skip: u64,
    scale: u64,
    cluster_ts: i64,
    failed: bool,
}

impl Default for Demuxer { fn default() -> Self { Self::new() } }

impl Demuxer {
    pub fn new() -> Demuxer { Demuxer { buf: Vec::new(), base: 0, stack: Vec::new(), skip: 0, scale: 1_000_000, cluster_ts: 0, failed: false } }
    /// MSE "reset parser state": forget partial data and open elements (TimestampScale is kept).
    pub fn reset(&mut self) { self.buf.clear(); self.stack.clear(); self.skip = 0; self.failed = false; }
    pub fn timestamp_scale(&self) -> u64 { self.scale }

    pub fn push(&mut self, data: &[u8]) -> Vec<Event> {
        let mut out = Vec::new();
        if self.failed { return out; }
        self.buf.extend_from_slice(data);
        let mut p = 0usize;
        loop {
            if self.skip > 0 {
                let n = self.skip.min((self.buf.len() - p) as u64) as usize;
                p += n; self.skip -= n as u64;
                if self.skip > 0 { break; }
            }
            let at = self.base + p as u64;
            // leave masters whose known end we reached
            while let Some(&(_, Some(end))) = self.stack.last() { if at >= end { self.stack.pop(); } else { break; } }
            let rest = &self.buf[p..];
            let Some((id, il)) = element_id(rest) else {
                // a first octet of 0, or one announcing more than 4 octets (RFC 8794 §5 default EBMLMaxIDLength), is not an ID;
                // anything else is an ID that has not fully arrived yet
                if rest.first().is_some_and(|&f| f == 0 || f.leading_zeros() >= 4) {
                    self.failed = true; out.push(Event::Error(format!("invalid element ID at offset {at}")));
                }
                break;
            };
            let Some((size, sl, unknown)) = vint(&rest[il..]) else {
                if rest.len() > il && rest[il] == 0 { self.failed = true; out.push(Event::Error(format!("invalid size at offset {at}"))); }
                break;
            };
            let hdr = il + sl;
            // an unknown-sized Cluster ends at the next Segment-level element (RFC 8794 §6.2)
            if SEGMENT_CHILDREN.contains(&id) {
                while matches!(self.stack.last(), Some(&(ID_CLUSTER, None))) { self.stack.pop(); }
            }
            match id {
                ID_SEGMENT => { self.stack.push((id, (!unknown).then(|| at + (hdr as u64) + size))); p += hdr; }
                ID_CLUSTER => {
                    self.stack.push((id, (!unknown).then(|| at + (hdr as u64) + size)));
                    p += hdr;
                    self.cluster_ts = 0;
                }
                ID_BLOCK_GROUP | ID_EBML | ID_INFO | ID_TRACKS | ID_SIMPLE_BLOCK | ID_TIMESTAMP => {
                    if unknown { self.failed = true; out.push(Event::Error(format!("element {id:#x} of unknown size at {at}"))); break; }
                    let total = hdr + size as usize;
                    if rest.len() < total { break; }
                    let body = &rest[hdr..total];
                    match id {
                        ID_EBML => out.push(Event::Header(children(body).iter().find(|c| c.0 == ID_DOCTYPE)
                            .map(|c| String::from_utf8_lossy(c.1).trim_end_matches('\0').to_string()).unwrap_or_default())),
                        ID_INFO => {
                            let kids = children(body);
                            if let Some(s) = kids.iter().find(|c| c.0 == ID_TIMESTAMP_SCALE) { let v = uint(s.1); if v > 0 { self.scale = v; } }
                            let dur = kids.iter().find(|c| c.0 == ID_DURATION).map(|d| float(d.1) * self.scale as f64);
                            out.push(Event::Info { timestamp_scale: self.scale, duration_ns: dur });
                        }
                        ID_TRACKS => out.push(Event::Tracks(children(body).iter().filter(|c| c.0 == ID_TRACK_ENTRY).map(|c| track(c.1)).collect())),
                        ID_TIMESTAMP => { self.cluster_ts = uint(body) as i64; out.push(Event::Cluster(self.cluster_ts * self.scale as i64)); }
                        ID_SIMPLE_BLOCK => match block(body, true, self.cluster_ts, self.scale) {
                            Ok(fs) => out.extend(fs.into_iter().map(Event::Frame)),
                            Err(e) => { self.failed = true; out.push(Event::Error(e)); break; }
                        },
                        _ => {
                            // BlockGroup: the Block plus its duration / discard padding (RFC 9559 §5.1.3.5)
                            let kids = children(body);
                            let Some(b) = kids.iter().find(|c| c.0 == ID_BLOCK) else { p += total; continue };
                            match block(b.1, false, self.cluster_ts, self.scale) {
                                Ok(mut fs) => {
                                    let key = !kids.iter().any(|c| c.0 == ID_REFERENCE_BLOCK);
                                    let dur = kids.iter().find(|c| c.0 == ID_BLOCK_DURATION).map(|c| uint(c.1) * self.scale);
                                    let pad = kids.iter().find(|c| c.0 == ID_DISCARD_PADDING).map(|c| sint(c.1));
                                    for f in fs.iter_mut() { f.key = key; f.duration_ns = dur; }
                                    if let Some(last) = fs.last_mut() { last.discard_padding_ns = pad; }
                                    out.extend(fs.into_iter().map(Event::Frame));
                                }
                                Err(e) => { self.failed = true; out.push(Event::Error(e)); break; }
                            }
                        }
                    }
                    p += total;
                }
                _ => {
                    // anything else (SeekHead, Cues, Tags, Void, CRC-32, …): dropped, possibly across chunks
                    if unknown { self.failed = true; out.push(Event::Error(format!("element {id:#x} of unknown size at {at}"))); break; }
                    p += hdr; self.skip = size;
                }
            }
        }
        self.buf.drain(..p);
        self.base += p as u64;
        out
    }
}

/// The direct children of a master element's body (known sizes only): (id, data).
pub fn children(mut b: &[u8]) -> Vec<(u32, &[u8])> {
    let mut v = Vec::new();
    while let Some((id, il)) = element_id(b) {
        let Some((size, sl, _)) = vint(&b[il..]) else { break };
        let s = il + sl;
        let e = (s as u64 + size).min(b.len() as u64) as usize;
        v.push((id, &b[s..e]));
        b = &b[e..];
    }
    v
}

fn track(b: &[u8]) -> Track {
    let mut t = Track { rate: 8000.0, channels: 1, ..Default::default() };
    for (id, d) in children(b) {
        match id {
            ID_TRACK_NUMBER => t.number = uint(d),
            ID_TRACK_TYPE => t.kind = uint(d),
            ID_CODEC_ID => t.codec = String::from_utf8_lossy(d).trim_end_matches('\0').to_string(),
            ID_CODEC_PRIVATE => t.private = d.to_vec(),
            ID_CODEC_DELAY => t.codec_delay_ns = uint(d),
            ID_SEEK_PREROLL => t.seek_preroll_ns = uint(d),
            ID_DEFAULT_DURATION => t.default_duration_ns = Some(uint(d)),
            ID_CONTENT_ENCODINGS => t.encoded = true,
            ID_VIDEO => for (vid, vd) in children(d) { match vid { ID_PIXEL_WIDTH => t.width = uint(vd), ID_PIXEL_HEIGHT => t.height = uint(vd), _ => {} } },
            ID_AUDIO => for (aid, ad) in children(d) { match aid { ID_SAMPLING_FREQUENCY => t.rate = float(ad), ID_CHANNELS => t.channels = uint(ad), _ => {} } },
            _ => {}
        }
    }
    t
}

/// RFC 9559 §10.1 / §10.2 / §10.3: one Block or SimpleBlock → its frames.
pub fn block(b: &[u8], simple: bool, cluster_ts: i64, scale: u64) -> Result<Vec<Frame>, String> {
    let (track, n, _) = vint(b).ok_or("block: track number")?;
    if b.len() < n + 3 { return Err("block: short header".into()); }
    let rel = i16::from_be_bytes([b[n], b[n + 1]]) as i64;
    let flags = b[n + 2];
    let key = simple && flags & 0x80 != 0;
    let invisible = flags & 0x08 != 0;
    let lacing = (flags >> 1) & 3;
    let data = &b[n + 3..];
    let sizes: Vec<usize>;
    let mut p = 0usize;
    match lacing {
        0 => sizes = vec![data.len()],
        _ => {
            let count = *data.first().ok_or("lace: no count")? as usize + 1;
            p = 1;
            let mut s = Vec::with_capacity(count);
            match lacing {
                1 => for _ in 0..count - 1 {
                    // Xiph (§10.3.2): 255-valued octets add up, the first octet < 255 ends the size
                    let mut v = 0usize;
                    loop { let x = *data.get(p).ok_or("xiph lace: truncated")?; p += 1; v += x as usize; if x != 255 { break; } }
                    s.push(v);
                },
                3 => {
                    // EBML (§10.3.3): first size a VINT, then signed VINT differences from the previous size
                    let (first, l, _) = vint(&data[p..]).ok_or("ebml lace: first size")?;
                    p += l; s.push(first as usize);
                    for _ in 1..count - 1 {
                        let (d, l) = signed_vint(&data[p..]).ok_or("ebml lace: size delta")?;
                        p += l;
                        let v = *s.last().unwrap() as i64 + d;
                        if v < 0 { return Err("ebml lace: negative size".into()); }
                        s.push(v as usize);
                    }
                }
                _ => {
                    // fixed-size (§10.3.4): every frame the same size
                    let total = data.len() - p;
                    if total % count != 0 { return Err("fixed lace: size not divisible".into()); }
                    s = vec![total / count; count - 1];
                }
            }
            let used: usize = s.iter().sum();
            if p + used > data.len() { return Err("lace: sizes exceed the block".into()); }
            s.push(data.len() - p - used);
            sizes = s;
        }
    }
    let pts = (cluster_ts + rel) * scale as i64;
    let mut out = Vec::with_capacity(sizes.len());
    for sz in sizes {
        out.push(Frame { track, pts_ns: pts, key, invisible, duration_ns: None, discard_padding_ns: None, data: data[p..p + sz].to_vec() });
        p += sz;
    }
    Ok(out)
}
