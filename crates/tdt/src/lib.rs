//! Native reader for Tucker-Davis Technologies (TDT) Synapse/OpenEx blocks.
//!
//! A block is a directory holding `<block>.tsq` (40-byte event headers),
//! `<block>.tev` (stream/snip payloads) and optionally `*.sev` files (one per
//! stream channel, used when the store was saved "UCF"/separately).
//! Format semantics follow TDT's reference `TDTbin2mat.m` / `SEV2mat.m`:
//!
//! * all timestamps are made relative to the block start and rounded to the
//!   TDT clock tick (2.56 µs);
//! * epoc onset types are STRON/MARK, offsets are STROFF linked through the
//!   "buddy" name stored in the channel/sortcode bytes; an onset epoc without a
//!   buddy gets `offset[i] = onset[i+1]` and `+inf` for the last one;
//! * stream samples for each channel are concatenated in header order and
//!   start at the timestamp of the first header (`start_time`), *not* at 0.
//!
//! Snips and scalars are not parsed: photometry needs streams and epocs only.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

const HEADER_BYTES: usize = 40;
const TICK: f64 = 2.56e-6;

const EVTYPE_STRON: u32 = 0x0101;
const EVTYPE_STROFF: u32 = 0x0102;
const EVTYPE_MARK: u32 = 0x8801;
const EVTYPE_STREAM: u32 = 0x8101;
const EVTYPE_MASK: u32 = 0xFF0F;
const EVTYPE_UCF: u32 = 0x0010;
const EVMARK_STARTBLOCK: u32 = 0x0001;
const EVMARK_STOPBLOCK: u32 = 0x0002;

#[derive(Debug)]
pub enum Error {
    Io(PathBuf, std::io::Error),
    Format(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(p, e) => write!(f, "{}: {e}", p.display()),
            Error::Format(m) => write!(f, "invalid TDT block: {m}"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

fn bad<T>(msg: impl Into<String>) -> Result<T> {
    Err(Error::Format(msg.into()))
}

/// A continuously sampled store (e.g. `465A`, `405A`). One `Vec` per channel.
#[derive(Debug, Clone)]
pub struct Stream {
    pub name: String,
    pub fs: f64,
    /// Time of the first sample, seconds from block start.
    pub start_time: f64,
    pub channels: Vec<Vec<f32>>,
}

impl Stream {
    /// Timestamp (s, block-relative) of sample `i`.
    pub fn time_of(&self, i: usize) -> f64 {
        self.start_time + i as f64 / self.fs
    }
}

/// A TTL/event store (e.g. `PC0_`, `Note`). Times are block-relative seconds.
#[derive(Debug, Clone, Default)]
pub struct Epoc {
    pub name: String,
    pub onset: Vec<f64>,
    pub offset: Vec<f64>,
    pub data: Vec<f64>,
}

#[derive(Debug, Clone)]
pub struct Block {
    pub path: PathBuf,
    /// Unix time (s) of block start.
    pub start_unix: f64,
    /// Block duration in seconds, if the block was closed cleanly.
    pub duration: Option<f64>,
    pub streams: BTreeMap<String, Stream>,
    pub epocs: BTreeMap<String, Epoc>,
}

/// Which stores to load. `None` = everything.
#[derive(Debug, Clone, Default)]
pub struct ReadOptions {
    pub stores: Option<Vec<String>>,
}

impl ReadOptions {
    fn wants(&self, name: &str) -> bool {
        self.stores.as_ref().is_none_or(|s| s.iter().any(|x| x == name))
    }
}

#[derive(Clone, Copy)]
struct Header {
    size: u32,
    kind: u32,
    code: u32,
    chan: u16,
    sort: u16,
    ts: f64,
    raw: [u8; 8], // file offset (u64) for data stores, f64 value for epocs
    dform: u32,
    fs: f32,
}

impl Header {
    fn parse(b: &[u8]) -> Self {
        let u32_at = |o| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
        Header {
            size: u32_at(0),
            kind: u32_at(4),
            code: u32_at(8),
            chan: u16::from_le_bytes([b[12], b[13]]),
            sort: u16::from_le_bytes([b[14], b[15]]),
            ts: f64::from_le_bytes(b[16..24].try_into().unwrap()),
            raw: b[24..32].try_into().unwrap(),
            dform: u32_at(32),
            fs: f32::from_le_bytes(b[36..40].try_into().unwrap()),
        }
    }
    fn offset(&self) -> u64 {
        u64::from_le_bytes(self.raw)
    }
    fn value(&self) -> f64 {
        f64::from_le_bytes(self.raw)
    }
}

fn store_name(code: u32) -> String {
    code.to_le_bytes()
        .iter()
        .map(|&c| c as char)
        .collect::<String>()
        .trim_end_matches(['\0', ' '])
        .to_string()
}

fn round_tick(t: f64) -> f64 {
    (t / TICK).round() * TICK
}

/// Bytes per sample for a TDT data format code, or `None` if unknown.
fn dform_width(dform: u32) -> Option<usize> {
    match dform & 7 {
        0 | 1 => Some(4), // float, int32
        2 => Some(2),     // int16
        3 => Some(1),     // int8
        4 | 5 => Some(8), // double, int64
        _ => None,
    }
}

/// Decode little-endian samples of `dform` into f32 (photometry stores are f32;
/// wider integer formats lose precision beyond 24 bits, which TDT ADCs never exceed).
fn decode(bytes: &[u8], dform: u32, out: &mut Vec<f32>) {
    match dform & 7 {
        0 => out.extend(bytes.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c))),
        1 => out.extend(bytes.as_chunks::<4>().0.iter().map(|c| i32::from_le_bytes(*c) as f32)),
        2 => out.extend(bytes.as_chunks::<2>().0.iter().map(|c| i16::from_le_bytes(*c) as f32)),
        3 => out.extend(bytes.iter().map(|&c| c as i8 as f32)),
        4 => out.extend(bytes.as_chunks::<8>().0.iter().map(|c| f64::from_le_bytes(*c) as f32)),
        5 => out.extend(bytes.as_chunks::<8>().0.iter().map(|c| i64::from_le_bytes(*c) as f32)),
        _ => {}
    }
}

fn read(path: &Path) -> Result<Vec<u8>> {
    fs::read(path).map_err(|e| Error::Io(path.to_path_buf(), e))
}

fn find_ext(dir: &Path, ext: &str) -> Result<Vec<PathBuf>> {
    let rd = fs::read_dir(dir).map_err(|e| Error::Io(dir.to_path_buf(), e))?;
    let mut v: Vec<PathBuf> = rd
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case(ext)))
        .collect();
    v.sort();
    Ok(v)
}

/// True if `dir` looks like a TDT block (contains a `.tsq`).
pub fn is_block(dir: &Path) -> bool {
    find_ext(dir, "tsq").is_ok_and(|v| !v.is_empty())
}

struct Index {
    tsq_path: PathBuf,
    start_unix: f64,
    duration: Option<f64>,
    by_store: BTreeMap<u32, Vec<Header>>,
}

fn index(dir: &Path) -> Result<Index> {
    let tsq_path = match find_ext(dir, "tsq")?.as_slice() {
        [p] => p.clone(),
        [] => return bad(format!("no .tsq file in {}", dir.display())),
        _ => return bad(format!("multiple .tsq files in {}", dir.display())),
    };
    let tsq = read(&tsq_path)?;
    if tsq.len() < 2 * HEADER_BYTES {
        return bad("tsq too short");
    }
    // as_chunks drops a partial trailing header left by a crash, as TDT does.
    let headers: Vec<Header> = tsq.as_chunks::<HEADER_BYTES>().0.iter().map(|b| Header::parse(b)).collect();
    if headers[1].code != EVMARK_STARTBLOCK {
        return bad("block start marker not found");
    }
    let start_unix = headers[1].ts;
    let last = headers.last().unwrap();
    let duration = (last.code == EVMARK_STOPBLOCK).then_some(last.ts - start_unix);

    // Group headers by store, preserving file order.
    let mut by_store: BTreeMap<u32, Vec<Header>> = BTreeMap::new();
    for h in &headers[2..] {
        if h.code == 0 || h.code == EVMARK_STARTBLOCK || h.code == EVMARK_STOPBLOCK {
            continue;
        }
        by_store.entry(h.code).or_default().push(*h);
    }
    Ok(Index { tsq_path, start_unix, duration, by_store })
}

/// What a block contains, from the TSQ index alone (no sample data is read).
#[derive(Debug, Clone)]
pub struct BlockInfo {
    pub name: String,
    pub start_unix: f64,
    pub duration: Option<f64>,
    pub streams: Vec<String>,
    pub epocs: Vec<String>,
    pub tsq_path: PathBuf,
}

pub fn block_info(dir: &Path) -> Result<BlockInfo> {
    let ix = index(dir)?;
    let mut streams = vec![];
    let mut epocs = vec![];
    for (code, hs) in &ix.by_store {
        let k = hs[0].kind;
        if k == EVTYPE_STRON || k == EVTYPE_MARK {
            epocs.push(store_name(*code));
        } else if k & EVTYPE_MASK == EVTYPE_STREAM {
            streams.push(store_name(*code));
        }
    }
    let name = ix.tsq_path.file_stem().map_or(String::new(), |s| s.to_string_lossy().into_owned());
    Ok(BlockInfo { name, start_unix: ix.start_unix, duration: ix.duration, streams, epocs, tsq_path: ix.tsq_path })
}

/// Read a TDT block directory.
pub fn read_block(dir: &Path, opts: &ReadOptions) -> Result<Block> {
    let Index { tsq_path, start_unix, duration, by_store, .. } = index(dir)?;

    let tev_path = tsq_path.with_extension("tev");
    // Whole-file read: fine for photometry tanks (≤ a few GB); mmap if raw 24 kHz stores matter.
    let needs_tev = by_store.iter().any(|(c, hs)| hs[0].kind & EVTYPE_MASK == EVTYPE_STREAM && hs[0].kind & EVTYPE_UCF == 0 && opts.wants(&store_name(*c)));
    let tev = if needs_tev && tev_path.exists() { Some(read(&tev_path)?) } else { None };
    let sev_files = find_ext(dir, "sev")?;

    let mut streams = BTreeMap::new();
    let mut onsets: BTreeMap<String, Epoc> = BTreeMap::new();
    let mut offsets: Vec<(String, Vec<f64>)> = Vec::new();

    for (code, hs) in by_store {
        let name = store_name(code);
        let kind = hs[0].kind;
        if kind == EVTYPE_STRON || kind == EVTYPE_MARK {
            if !opts.wants(&name) {
                continue;
            }
            let onset: Vec<f64> = hs.iter().map(|h| round_tick(h.ts - start_unix)).collect();
            let mut offset: Vec<f64> = onset.iter().skip(1).copied().collect();
            offset.push(f64::INFINITY);
            let data = hs.iter().map(Header::value).collect();
            onsets.insert(name.clone(), Epoc { name, onset, offset, data });
        } else if kind == EVTYPE_STROFF {
            let b = [hs[0].chan.to_le_bytes(), hs[0].sort.to_le_bytes()].concat();
            let buddy = store_name(u32::from_le_bytes(b.try_into().unwrap()));
            offsets.push((buddy, hs.iter().map(|h| round_tick(h.ts - start_unix)).collect()));
        } else if kind & EVTYPE_MASK == EVTYPE_STREAM {
            if !opts.wants(&name) {
                continue;
            }
            let start_time = round_tick(hs[0].ts - start_unix);
            let fs = hs[0].fs as f64;
            let stream = if kind & EVTYPE_UCF != 0 {
                read_sev(&sev_files, &name, start_time)?
            } else {
                let Some(tev) = tev.as_deref() else { return bad(format!("{name}: missing .tev")) };
                read_tev_stream(tev, &name, &hs, fs, start_time)?
            };
            streams.insert(name, stream);
        }
    }

    // SEV-only stores (not indexed in the tsq).
    for f in &sev_files {
        if let Some(name) = sev_store_name(f).filter(|n| !streams.contains_key(n) && opts.wants(n)) {
            let s = read_sev(&sev_files, &name, 0.0)?;
            streams.insert(name, s);
        }
    }

    for (buddy, off) in offsets {
        let Some(e) = onsets.get_mut(&buddy) else { continue };
        if off.first().zip(e.onset.first()).is_some_and(|(o, n)| o < n) {
            e.onset.insert(0, 0.0);
            let d0 = e.data.first().copied().unwrap_or(0.0);
            e.data.insert(0, d0);
        }
        e.offset = off;
        if e.offset.len() < e.onset.len() {
            e.offset.push(f64::INFINITY);
        }
        e.offset.truncate(e.onset.len());
    }

    Ok(Block { path: dir.to_path_buf(), start_unix, duration, streams, epocs: onsets })
}

fn read_tev_stream(tev: &[u8], name: &str, hs: &[Header], fs: f64, start_time: f64) -> Result<Stream> {
    let Some(w) = dform_width(hs[0].dform) else { return bad(format!("{name}: unknown data format")) };
    let nchan = hs.iter().map(|h| h.chan).max().unwrap_or(1).max(1) as usize;
    let per_chan = hs.len() / nchan * (hs[0].size as usize).saturating_sub(10) * 4 / w;
    let mut channels = vec![Vec::with_capacity(per_chan); nchan];
    for h in hs {
        let payload = (h.size as usize).saturating_sub(10) * 4;
        let o = h.offset() as usize;
        let Some(bytes) = tev.get(o..o + payload) else {
            return bad(format!("{name}: data missing from .tev at t={:.3}s", h.ts));
        };
        let ch = (h.chan.max(1) - 1) as usize;
        decode(bytes, h.dform, &mut channels[ch]);
    }
    // Channels must align sample-for-sample; a crash can leave one short.
    let n = channels.iter().map(Vec::len).min().unwrap_or(0);
    channels.iter_mut().for_each(|c| c.truncate(n));
    Ok(Stream { name: name.to_string(), fs, start_time, channels })
}

/// Store name from a SEV filename like `Block-1_x465A_Ch1.sev` or `..._Ch1-2h.sev`.
fn sev_store_name(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?;
    let parts: Vec<&str> = stem.split('_').filter(|s| !s.is_empty()).collect();
    (parts.len() > 1).then(|| parts[parts.len() - 2].chars().take(4).collect())
}

fn sev_channel_hour(path: &Path) -> (u16, u32) {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let tail = stem.rsplit('_').next().unwrap_or("");
    let digits = |s: &str| s.chars().take_while(char::is_ascii_digit).collect::<String>();
    let chan = tail.strip_prefix(['C', 'c']).and_then(|s| s.strip_prefix('h')).map(digits);
    let hour = tail.split('-').nth(1).map(digits);
    (
        chan.and_then(|s| s.parse().ok()).unwrap_or(1),
        hour.and_then(|s| s.parse().ok()).unwrap_or(0),
    )
}

fn read_sev(files: &[PathBuf], name: &str, start_time: f64) -> Result<Stream> {
    let mut parts: Vec<(u16, u32, &PathBuf)> = files
        .iter()
        .filter(|f| sev_store_name(f).as_deref() == Some(name))
        .map(|f| {
            let (c, h) = sev_channel_hour(f);
            (c, h, f)
        })
        .collect();
    if parts.is_empty() {
        return bad(format!("{name}: expected SEV files but none found"));
    }
    parts.sort();
    let nchan = parts.iter().map(|p| p.0).max().unwrap() as usize;
    let mut channels = vec![Vec::new(); nchan];
    let mut fs = 0.0;
    for (chan, _, path) in parts {
        let b = read(path)?;
        if b.len() < HEADER_BYTES || &b[8..11] != b"SEV" {
            return bad(format!("{}: not a SEV file", path.display()));
        }
        let version = b[11];
        if version == 0 || version > 3 {
            return bad(format!("{}: unsupported SEV version {version}", path.display()));
        }
        let dform = (b[24] & 7) as u32;
        let decimate = b[25].max(1) as f64;
        let rate = u16::from_le_bytes([b[26], b[27]]) as i32;
        fs = 2f64.powi(rate - 12) * 25_000_000.0 / decimate;
        decode(&b[HEADER_BYTES..], dform, &mut channels[chan as usize - 1]);
    }
    let n = channels.iter().map(Vec::len).min().unwrap_or(0);
    channels.iter_mut().for_each(|c| c.truncate(n));
    Ok(Stream { name: name.to_string(), fs, start_time, channels })
}

/// Write a minimal synthetic block (tsq + tev). Used by tests and demo data;
/// mirrors the on-disk layout Synapse produces for float streams and epocs.
pub mod synth {
    use super::*;

    pub struct SynthStream<'a> {
        pub name: &'a str,
        pub fs: f32,
        pub channels: &'a [&'a [f32]],
    }

    pub struct SynthEpoc<'a> {
        pub name: &'a str,
        pub onsets: &'a [f64],
        /// Optional offsets, written as a STROFF buddy store named `offset_name`.
        pub offsets: Option<(&'a str, &'a [f64])>,
    }

    #[allow(clippy::too_many_arguments)] // mirrors the 10 fields of a TSQ record
    fn header(out: &mut Vec<u8>, size: u32, kind: u32, code: u32, chan: u16, sort: u16, ts: f64, raw: [u8; 8], dform: u32, fs: f32) {
        out.extend(size.to_le_bytes());
        out.extend(kind.to_le_bytes());
        out.extend(code.to_le_bytes());
        out.extend(chan.to_le_bytes());
        out.extend(sort.to_le_bytes());
        out.extend(ts.to_le_bytes());
        out.extend(raw);
        out.extend(dform.to_le_bytes());
        out.extend(fs.to_le_bytes());
    }

    fn code(name: &str) -> u32 {
        let mut b = [b' '; 4];
        b[..name.len().min(4)].copy_from_slice(&name.as_bytes()[..name.len().min(4)]);
        u32::from_le_bytes(b)
    }

    /// Writes `<dir>/<block>.tsq/.tev`. Streams are chunked in 256-sample headers.
    pub fn write_block(dir: &Path, block: &str, start_unix: f64, streams: &[SynthStream], epocs: &[SynthEpoc]) -> std::io::Result<()> {
        const CHUNK: usize = 256;
        fs::create_dir_all(dir)?;
        let (mut tsq, mut tev) = (Vec::new(), Vec::new());
        header(&mut tsq, 0, 0, 0, 0, 0, 0.0, [0; 8], 0, 0.0);
        header(&mut tsq, 10, 0, EVMARK_STARTBLOCK, 0, 0, start_unix, [0; 8], 0, 0.0);
        let mut duration: f64 = 0.0;
        for s in streams {
            let n = s.channels[0].len();
            for start in (0..n).step_by(CHUNK) {
                let len = CHUNK.min(n - start);
                let ts = start_unix + start as f64 / s.fs as f64;
                for (c, data) in s.channels.iter().enumerate() {
                    let off = tev.len() as u64;
                    data[start..start + len].iter().for_each(|v| tev.extend(v.to_le_bytes()));
                    header(&mut tsq, 10 + len as u32, EVTYPE_STREAM, code(s.name), c as u16 + 1, 0, ts, off.to_le_bytes(), 0, s.fs);
                }
            }
            duration = duration.max(n as f64 / s.fs as f64);
        }
        for e in epocs {
            for (i, &t) in e.onsets.iter().enumerate() {
                header(&mut tsq, 10, EVTYPE_STRON, code(e.name), 0, 0, start_unix + t, (i as f64 + 1.0).to_le_bytes(), 4, 0.0);
            }
            if let Some((oname, offs)) = e.offsets {
                let b = code(e.name).to_le_bytes();
                let (chan, sort) = (u16::from_le_bytes([b[0], b[1]]), u16::from_le_bytes([b[2], b[3]]));
                for &t in offs {
                    header(&mut tsq, 10, EVTYPE_STROFF, code(oname), chan, sort, start_unix + t, 0f64.to_le_bytes(), 4, 0.0);
                }
            }
        }
        header(&mut tsq, 10, 0, EVMARK_STOPBLOCK, 0, 0, start_unix + duration, [0; 8], 0, 0.0);
        fs::write(dir.join(format!("{block}.tsq")), tsq)?;
        fs::write(dir.join(format!("{block}.tev")), tev)
    }
}

#[cfg(test)]
mod tests {
    use super::synth::*;
    use super::*;

    #[test]
    fn roundtrip_streams_and_epocs() {
        let dir = std::env::temp_dir().join(format!("simplefp-tdt-{}", std::process::id()));
        let fs = 1_017.252_6_f32;
        let a: Vec<f32> = (0..5000).map(|i| (i as f32 * 0.01).sin()).collect();
        let b: Vec<f32> = (0..5000).map(|i| i as f32).collect();
        write_block(
            &dir,
            "Test-250101-120000",
            1.7e9,
            &[SynthStream { name: "465A", fs, channels: &[&a] }, SynthStream { name: "405A", fs, channels: &[&b] }],
            &[
                SynthEpoc { name: "PC0_", onsets: &[1.0, 2.5], offsets: Some(("PC0\\", &[1.5, 3.0])) },
                SynthEpoc { name: "Tick", onsets: &[0.0, 1.0, 2.0], offsets: None },
            ],
        )
        .unwrap();

        let blk = read_block(&dir, &ReadOptions::default()).unwrap();
        let s = &blk.streams["465A"];
        assert_eq!(s.channels[0], a);
        assert_eq!(blk.streams["405A"].channels[0], b);
        assert!((s.fs - fs as f64).abs() < 1e-9);
        assert_eq!(s.start_time, 0.0);

        let pc = &blk.epocs["PC0_"];
        assert_eq!(pc.onset.len(), 2);
        assert!((pc.onset[1] - 2.5).abs() < TICK);
        assert!((pc.offset[0] - 1.5).abs() < TICK);
        let tick = &blk.epocs["Tick"];
        assert_eq!(tick.offset.last(), Some(&f64::INFINITY));
        assert!((blk.duration.unwrap() - 5000.0 / fs as f64).abs() < 1e-6);

        let only = read_block(&dir, &ReadOptions { stores: Some(vec!["405A".into()]) }).unwrap();
        assert_eq!(only.streams.keys().collect::<Vec<_>>(), ["405A"]);
        let info = block_info(&dir).unwrap();
        assert_eq!((info.streams.len(), info.epocs.len()), (2, 2));
        assert!(only.epocs.is_empty());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn sev_name_parsing() {
        let p = Path::new("Block-1_x465A_Ch2-3h.sev");
        assert_eq!(sev_store_name(p).as_deref(), Some("x465"));
        assert_eq!(sev_channel_hour(p), (2, 3));
        assert_eq!(sev_channel_hour(Path::new("B_Wav1_Ch1.sev")), (1, 0));
    }
}
