//! The import guard (SERVICE.md "Import guard", decision 4): the service's
//! own check of a file, before the engine sees a byte of it, on every path
//! that opens a file (`deck.open`, `deck.cover`, the v1 file methods, and
//! through them the agent's `deck.info` and `deck.run`).
//!
//! deckcraft sizes what it allocates for a zip from the zip's own headers
//! (#448): its pptx reader takes up to 50,000 entries, 512 MiB a part and
//! 2 GiB in all, its native reader 2 GiB an entry, and neither checks that
//! an entry inflates to no more than its directory says. A hostile file
//! could make it allocate gigabytes on the shell's UI thread and abort the
//! process. So, before the engine reads anything:
//!
//! - **A zip** (`.pptx`, `.deckcraft`): its central directory is read with
//!   bounds, by this module and not by the zip library: at most
//!   [`MAX_ENTRIES`] entries, names of at most [`MAX_NAME_BYTES`], no entry
//!   declaring over [`MAX_ENTRY_BYTES`] uncompressed, at most
//!   [`MAX_TOTAL_BYTES`] declared in all, and no entry past [`RATIO_GRACE`]
//!   compressing more than [`MAX_RATIO`] to 1 (Apache POI's zip-bomb rule).
//!   Over any of those is `held:`. A directory that does not parse, an
//!   entry outside the file, an encrypted entry or one compressed in a way
//!   the engine does not read is `damaged:`.
//! - Then its names say what it is: a native deck (a `mimetype` entry
//!   naming deckcraft's type) or an Office package (`[Content_Types].xml`).
//!   Any other zip (`.key`, `.odp`) is `unsupported:`.
//! - Then every entry is inflated, bounded, into a scratch buffer (nothing
//!   is kept but its first bytes): one that inflates past its declared size
//!   is `damaged:`. Any audio or video part is `held:`, found by its
//!   extension, by the content type the package declares for it
//!   (`[Content_Types].xml`, or a native deck's `presentation.json`), or by
//!   its first bytes (a movie or sound container's signature).
//! - So is a zip inside the zip, except an Office package's embedded object
//!   ([`EMBEDDINGS`]): a chart's workbook or an embedded document is a zip
//!   that neither the service nor the engine ever unzips (deckcraft's only
//!   zip readers are its package and native readers; it reads a chart from
//!   the chart's own cached values, and keeps only an embedded object's
//!   XML and preview). Such a part passes, held like every entry to the
//!   bounds above (entries, name, declared and actual size, ratio, totals)
//!   and to the media checks; its own entries are never read, by anyone.
//! - **An OLE compound file** (`D0 CF 11 E0`): one whose directory holds an
//!   `EncryptedPackage` stream is a password-protected Office file
//!   (`protected:`); any other is a legacy `.ppt` or not a presentation at
//!   all (`unsupported:`). Its directory is read with bounds too.
//! - **A PDF** is `unsupported:`, and so is any other file that is not
//!   outline text (`.txt` or `.md`, by name, as the engine reads it).
//!
//! The guard reads only the bytes the service already read (at most the
//! service's 64 MiB a file). Beyond the directory's entries it allocates a
//! 64 KiB inflate buffer, the package's content-type list (at most
//! [`MAX_TYPES_BYTES`]) and, for an OLE file, a sector table of at most one
//! entry a sector. Engine-side bounds stay tracked in #448.

use std::io::Read;

use crate::codes::{fail, Code};

/// The most entries a zip may have.
pub(crate) const MAX_ENTRIES: usize = 10_000;
/// The longest entry name, in bytes.
pub(crate) const MAX_NAME_BYTES: usize = 1024;
/// The most one entry may declare, uncompressed.
pub(crate) const MAX_ENTRY_BYTES: u64 = 64 << 20;
/// The most a zip's entries may declare together.
pub(crate) const MAX_TOTAL_BYTES: u64 = 256 << 20;
/// The most an entry may compress to 1, once past [`RATIO_GRACE`].
pub(crate) const MAX_RATIO: u64 = 100;
/// Entries up to this size are not held to [`MAX_RATIO`]: a small blank
/// picture or printer setting compresses far better than that.
pub(crate) const RATIO_GRACE: u64 = 100 << 10;
/// The most of `[Content_Types].xml` the guard reads for media types.
pub(crate) const MAX_TYPES_BYTES: u64 = 1 << 20;

/// What the engine will be asked to read, once the guard passed the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Format {
    /// deckcraft's own `.deckcraft` zip.
    Native,
    /// An Office Open XML package (`.pptx`).
    Pptx,
    /// Outline text (`.txt`, `.md`).
    Outline,
}

/// The OLE compound file signature: a legacy Office file, or an encrypted
/// Office Open XML one.
const OLE: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

/// deckcraft's native `mimetype`s (`deckcraft_format::MIMETYPE`, and the
/// type of files written before its rename).
fn native_mimetype(content: &[u8]) -> bool {
    content == deckcraft_format::MIMETYPE.as_bytes() || content == deckcraft_format::LEGACY_MIMETYPE.as_bytes()
}

/// Check `bytes`, the file at `name` (storage-relative, only its extension
/// is used), before the engine reads it. `Ok` says how the engine will read
/// it; `Err` is the coded refusal.
pub(crate) fn check(name: &str, bytes: &[u8]) -> Result<Format, String> {
    if bytes.starts_with(&OLE[..4]) {
        return Err(ole(bytes));
    }
    if bytes.starts_with(b"PK") {
        return zip(bytes);
    }
    if bytes.starts_with(b"%PDF") {
        return Err(fail(Code::Unsupported, "this is a PDF, not a presentation the deck engine reads (.pptx or .deckcraft)"));
    }
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".txt") || lower.ends_with(".md") {
        return Ok(Format::Outline);
    }
    Err(fail(Code::Unsupported, "this is not a presentation the deck engine reads (.pptx, .deckcraft, or outline .txt or .md)"))
}

fn held(words: impl std::fmt::Display) -> String {
    fail(Code::Held, words)
}

fn damaged(words: impl std::fmt::Display) -> String {
    fail(Code::Damaged, words)
}

/// An entry's name as an error may show it: printable, and short.
fn shown(name: &str) -> String {
    let clean: String = name.chars().filter(|c| !c.is_control()).collect();
    if clean.chars().count() > 80 {
        format!("{}…", clean.chars().take(79).collect::<String>())
    } else {
        clean
    }
}

/// Little-endian numbers at an offset the file itself may have named: out
/// of range (or overflowing) is `None`, never a panic.
fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at.checked_add(2)?)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at.checked_add(4)?)?.try_into().ok()?))
}

fn u64_at(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(at..at.checked_add(8)?)?.try_into().ok()?))
}

/// One entry of a zip's central directory.
#[derive(Debug)]
struct Entry {
    name: String,
    flags: u16,
    method: u16,
    compressed: u64,
    declared: u64,
    offset: u64,
}

impl Entry {
    fn is_dir(&self) -> bool {
        self.name.ends_with('/')
    }

    fn extension(&self) -> String {
        let file = self.name.rsplit('/').next().unwrap_or("");
        file.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default()
    }
}

const EOCD: u32 = 0x0605_4b50;
const ZIP64_LOCATOR: u32 = 0x0706_4b50;
const ZIP64_EOCD: u32 = 0x0606_4b50;
const CENTRAL: u32 = 0x0201_4b50;
const LOCAL: u32 = 0x0403_4b50;

/// The central directory, read with the bounds of the module doc.
fn directory(b: &[u8]) -> Result<Vec<Entry>, String> {
    let unreadable = || damaged("the presentation's zip directory can't be read");
    // The end record: the last one in the final 64 KiB (its comment's most).
    let lowest = b.len().saturating_sub(22 + 0xFFFF);
    let mut at = None;
    let mut i = b.len().checked_sub(22).ok_or_else(unreadable)?;
    loop {
        if u32_at(b, i) == Some(EOCD) && u16_at(b, i + 20).is_some_and(|c| i + 22 + c as usize <= b.len()) {
            at = Some(i);
            break;
        }
        if i == lowest {
            break;
        }
        i -= 1;
    }
    let eocd = at.ok_or_else(unreadable)?;
    let field16 = |o: usize| u16_at(b, eocd + o).ok_or_else(unreadable);
    let field32 = |o: usize| u32_at(b, eocd + o).ok_or_else(unreadable);
    if field16(4)? != 0 || field16(6)? != 0 {
        return Err(damaged("the presentation's zip spans several disks"));
    }
    let mut count = field16(10)? as u64;
    let mut size = field32(12)? as u64;
    let mut start = field32(16)? as u64;
    if count == 0xFFFF || size == 0xFFFF_FFFF || start == 0xFFFF_FFFF {
        // ZIP64: the locator just before the end record names the record.
        let locator = eocd.checked_sub(20).filter(|l| u32_at(b, *l) == Some(ZIP64_LOCATOR)).ok_or_else(unreadable)?;
        let record = usize::try_from(u64_at(b, locator + 8).ok_or_else(unreadable)?).map_err(|_| unreadable())?;
        if u32_at(b, record) != Some(ZIP64_EOCD) {
            return Err(unreadable());
        }
        count = u64_at(b, record + 32).ok_or_else(unreadable)?;
        size = u64_at(b, record + 40).ok_or_else(unreadable)?;
        start = u64_at(b, record + 48).ok_or_else(unreadable)?;
    }
    if count > MAX_ENTRIES as u64 {
        return Err(held(format!("the presentation's zip has {count} entries, more than the {MAX_ENTRIES} the deck service opens (#448)")));
    }
    let start = usize::try_from(start).map_err(|_| unreadable())?;
    let end = start.checked_add(usize::try_from(size).map_err(|_| unreadable())?).filter(|e| *e <= b.len()).ok_or_else(unreadable)?;
    let mut entries = Vec::with_capacity(count as usize);
    let mut total: u64 = 0;
    let mut p = start;
    for _ in 0..count {
        if p + 46 > end || u32_at(b, p) != Some(CENTRAL) {
            return Err(unreadable());
        }
        let flags = u16_at(b, p + 8).ok_or_else(unreadable)?;
        let method = u16_at(b, p + 10).ok_or_else(unreadable)?;
        let mut compressed = u32_at(b, p + 20).ok_or_else(unreadable)? as u64;
        let mut declared = u32_at(b, p + 24).ok_or_else(unreadable)? as u64;
        let name_len = u16_at(b, p + 28).ok_or_else(unreadable)? as usize;
        let extra_len = u16_at(b, p + 30).ok_or_else(unreadable)? as usize;
        let comment_len = u16_at(b, p + 32).ok_or_else(unreadable)? as usize;
        let mut offset = u32_at(b, p + 42).ok_or_else(unreadable)? as u64;
        if name_len > MAX_NAME_BYTES {
            return Err(held(format!("an entry of the presentation's zip has a name of {name_len} bytes, more than the {MAX_NAME_BYTES} the deck service opens (#448)")));
        }
        let name_at = p + 46;
        let extra_at = name_at + name_len;
        let next = extra_at + extra_len + comment_len;
        if next > end {
            return Err(unreadable());
        }
        let name = String::from_utf8_lossy(&b[name_at..extra_at]).into_owned();
        // ZIP64 sizes and offset, for the fields their 32-bit slot marks.
        if compressed == 0xFFFF_FFFF || declared == 0xFFFF_FFFF || offset == 0xFFFF_FFFF {
            let mut e = extra_at;
            let extra_end = extra_at + extra_len;
            let mut found = false;
            while e + 4 <= extra_end {
                let id = u16_at(b, e).ok_or_else(unreadable)?;
                let len = u16_at(b, e + 2).ok_or_else(unreadable)? as usize;
                let body = e + 4;
                if body + len > extra_end {
                    return Err(unreadable());
                }
                if id == 0x0001 {
                    let mut q = body;
                    let mut take = |slot: &mut u64| -> Result<(), String> {
                        if *slot == 0xFFFF_FFFF {
                            if q + 8 > body + len {
                                return Err(unreadable());
                            }
                            *slot = u64_at(b, q).ok_or_else(unreadable)?;
                            q += 8;
                        }
                        Ok(())
                    };
                    take(&mut declared)?;
                    take(&mut compressed)?;
                    take(&mut offset)?;
                    found = true;
                    break;
                }
                e = body + len;
            }
            if !found {
                return Err(unreadable());
            }
        }
        if declared > MAX_ENTRY_BYTES {
            return Err(held(format!(
                "`{}` in the presentation's zip declares {declared} bytes, more than the {MAX_ENTRY_BYTES} the deck service opens in one entry (#448)",
                shown(&name)
            )));
        }
        total = total.saturating_add(declared);
        if total > MAX_TOTAL_BYTES {
            return Err(held(format!("the presentation's zip declares more than the {MAX_TOTAL_BYTES} bytes the deck service opens in all (#448)")));
        }
        if declared > RATIO_GRACE && declared > compressed.saturating_mul(MAX_RATIO) {
            return Err(held(format!(
                "`{}` in the presentation's zip compresses {declared} bytes into {compressed}, more than the {MAX_RATIO} to 1 the deck service opens (#448)",
                shown(&name)
            )));
        }
        entries.push(Entry { name, flags, method, compressed, declared, offset });
        p = next;
    }
    Ok(entries)
}

/// Where `e`'s data lies in `b`, from its local header.
fn data_of<'a>(b: &'a [u8], e: &Entry) -> Result<&'a [u8], String> {
    let outside = || damaged(format!("`{}` lies outside the presentation's zip", shown(&e.name)));
    let at = usize::try_from(e.offset).map_err(|_| outside())?;
    if u32_at(b, at) != Some(LOCAL) {
        return Err(outside());
    }
    let name_len = u16_at(b, at + 26).ok_or_else(outside)? as usize;
    let extra_len = u16_at(b, at + 28).ok_or_else(outside)? as usize;
    let start = at + 30 + name_len + extra_len;
    let len = usize::try_from(e.compressed).map_err(|_| outside())?;
    b.get(start..start.checked_add(len).ok_or_else(outside)?).ok_or_else(outside)
}

/// What an entry turned out to hold: its first bytes, and the whole of it
/// when the caller asked to keep it.
struct Inflated {
    head: Vec<u8>,
    kept: Option<Vec<u8>>,
}

/// How many first bytes of each entry the signatures look at.
const HEAD: usize = 16;

/// Inflate (or read) `e` with its declared size as the bound, keeping its
/// first bytes, the whole of it when `keep`, and feeding every chunk to
/// `watch`.
fn inflate(b: &[u8], e: &Entry, keep: bool, watch: &mut dyn FnMut(&[u8])) -> Result<Inflated, String> {
    if e.flags & 1 != 0 {
        return Err(damaged(format!("`{}` in the presentation's zip is encrypted, which the deck engine doesn't read", shown(&e.name))));
    }
    let data = data_of(b, e)?;
    let mut out = Inflated { head: Vec::with_capacity(HEAD), kept: keep.then(Vec::new) };
    let mut take = |chunk: &[u8], out: &mut Inflated| {
        let room = HEAD.saturating_sub(out.head.len());
        out.head.extend_from_slice(&chunk[..chunk.len().min(room)]);
        if let Some(kept) = out.kept.as_mut() {
            kept.extend_from_slice(chunk);
        }
        watch(chunk);
    };
    match e.method {
        0 => {
            if e.compressed != e.declared {
                return Err(damaged(format!("`{}` in the presentation's zip is not the size its directory says", shown(&e.name))));
            }
            take(data, &mut out);
        }
        8 => {
            let mut decoder = flate2::read::DeflateDecoder::new(data);
            let mut buf = vec![0u8; 64 << 10];
            let mut total: u64 = 0;
            loop {
                let n = decoder
                    .read(&mut buf)
                    .map_err(|err| damaged(format!("`{}` in the presentation's zip can't be inflated: {err}", shown(&e.name))))?;
                if n == 0 {
                    break;
                }
                total += n as u64;
                if total > e.declared {
                    return Err(damaged(format!(
                        "`{}` in the presentation's zip inflates past the {} bytes its directory declares",
                        shown(&e.name),
                        e.declared
                    )));
                }
                take(&buf[..n], &mut out);
            }
        }
        other => {
            return Err(damaged(format!(
                "`{}` in the presentation's zip is compressed with method {other}, which the deck engine doesn't read",
                shown(&e.name)
            )))
        }
    }
    Ok(out)
}

/// File extensions of movies and sounds.
const MEDIA_EXTENSIONS: &[&str] = &[
    "mp4", "m4v", "mov", "qt", "wmv", "asf", "avi", "webm", "mkv", "mka", "mpg", "mpeg", "mpe", "m1v", "m2v", "3gp", "3g2", "flv", "ogv", "ogg",
    "oga", "opus", "mp3", "wav", "wave", "m4a", "m4b", "aac", "wma", "flac", "aif", "aiff", "aifc", "mid", "midi", "au", "snd", "caf", "amr",
    "swf", "mts", "m2ts",
];

/// The kind of movie or sound container `head` starts with, by its
/// signature (strong signatures only: a two-byte frame sync would also
/// match the first bytes of obfuscated fonts and other binary parts).
fn media_signature(head: &[u8]) -> Option<&'static str> {
    let at = |i: usize, sig: &[u8]| head.get(i..i + sig.len()) == Some(sig);
    if at(4, b"ftyp") {
        Some("an MP4 or QuickTime movie or sound")
    } else if at(0, b"RIFF") && at(8, b"WAVE") {
        Some("a WAV sound")
    } else if at(0, b"RIFF") && at(8, b"AVI ") {
        Some("an AVI movie")
    } else if at(0, b"OggS") {
        Some("an Ogg movie or sound")
    } else if at(0, b"fLaC") {
        Some("a FLAC sound")
    } else if at(0, &[0x1A, 0x45, 0xDF, 0xA3]) {
        Some("a Matroska or WebM movie or sound")
    } else if at(0, &[0x30, 0x26, 0xB2, 0x75, 0x8E, 0x66, 0xCF, 0x11]) {
        Some("a Windows Media movie or sound")
    } else if at(0, b"ID3") {
        Some("an MP3 sound")
    } else if at(0, b"FORM") && (at(8, b"AIFF") || at(8, b"AIFC")) {
        Some("an AIFF sound")
    } else if at(0, b"caff") {
        Some("a Core Audio sound")
    } else if at(0, b"MThd") {
        Some("a MIDI sound")
    } else if at(0, b"FLV\x01") {
        Some("a Flash video")
    } else if at(0, b".snd") {
        Some("an AU sound")
    } else if at(0, &[0, 0, 1, 0xBA]) || at(0, &[0, 0, 1, 0xB3]) {
        Some("an MPEG movie")
    } else if at(0, b"#!AMR") {
        Some("an AMR sound")
    } else {
        None
    }
}

/// Whether `head` starts a zip archive: a zip inside the zip.
fn zip_signature(head: &[u8]) -> bool {
    head.starts_with(b"PK\x03\x04") || head.starts_with(b"PK\x05\x06") || head.starts_with(b"PK\x07\x08")
}

/// Where an Office package keeps its embedded objects (a chart's workbook,
/// an embedded document or OLE object), whose parts may be zips themselves.
pub(crate) const EMBEDDINGS: &str = "ppt/embeddings/";

/// Whether `name` is an embedded object's part: under [`EMBEDDINGS`], with
/// no empty, `.` or `..` segment and no backslash, so no other part can
/// borrow the exemption by its spelling.
fn embedded_object(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.strip_prefix(EMBEDDINGS).is_some_and(|rest| !rest.contains('\\') && rest.split('/').all(|seg| !seg.is_empty() && seg != "." && seg != ".."))
}

/// Pairs of names, lowercased.
type Pairs = Vec<(String, String)>;

/// `[Content_Types].xml`'s declarations: (extension, type) defaults and
/// (part name, type) overrides, lowercased, part names without their
/// leading `/`.
fn content_types(xml: &str) -> (Pairs, Pairs) {
    let mut defaults = Vec::new();
    let mut overrides = Vec::new();
    let mut rest = xml;
    while let Some(lt) = rest.find('<') {
        rest = &rest[lt + 1..];
        let end = rest.find('>').unwrap_or(rest.len());
        let tag = &rest[..end];
        let name_end = tag.find(|c: char| c.is_whitespace() || c == '/').unwrap_or(tag.len());
        let (name, attrs) = tag.split_at(name_end);
        let attr = |key: &str| -> Option<String> {
            let mut a = attrs;
            while let Some(i) = a.find(key) {
                let before_ok = a[..i].chars().next_back().is_none_or(char::is_whitespace);
                let after = a[i + key.len()..].trim_start();
                if before_ok && after.starts_with('=') {
                    let value = after[1..].trim_start();
                    let quote = value.chars().next()?;
                    if quote == '"' || quote == '\'' {
                        let body = &value[1..];
                        return body.find(quote).map(|j| body[..j].to_string());
                    }
                }
                a = &a[i + key.len()..];
            }
            None
        };
        match name {
            "Default" => {
                if let (Some(ext), Some(ct)) = (attr("Extension"), attr("ContentType")) {
                    defaults.push((ext.to_ascii_lowercase(), ct.to_ascii_lowercase()));
                }
            }
            "Override" => {
                if let (Some(part), Some(ct)) = (attr("PartName"), attr("ContentType")) {
                    overrides.push((part.trim_start_matches('/').to_ascii_lowercase(), ct.to_ascii_lowercase()));
                }
            }
            _ => {}
        }
        rest = &rest[end.min(rest.len())..];
    }
    (defaults, overrides)
}

/// A streaming search for `needles` across the chunks of one entry.
struct Scan {
    needles: &'static [&'static [u8]],
    tail: Vec<u8>,
    found: bool,
}

impl Scan {
    fn new(needles: &'static [&'static [u8]]) -> Scan {
        Scan { needles, tail: Vec::new(), found: false }
    }

    fn feed(&mut self, chunk: &[u8]) {
        if self.found {
            return;
        }
        let longest = self.needles.iter().map(|n| n.len()).max().unwrap_or(1);
        let mut window = std::mem::take(&mut self.tail);
        window.extend_from_slice(chunk);
        let first: Vec<u8> = self.needles.iter().filter_map(|n| n.first().copied()).collect();
        let mut i = 0;
        while i < window.len() {
            match window[i..].iter().position(|c| first.contains(c)) {
                Some(k) => {
                    let at = i + k;
                    if self.needles.iter().any(|n| window[at..].starts_with(n)) {
                        self.found = true;
                        return;
                    }
                    i = at + 1;
                }
                None => break,
            }
        }
        let keep = window.len().saturating_sub(longest - 1);
        self.tail = window.split_off(keep);
    }
}

/// How a native deck's `presentation.json` names the content type of a
/// movie or a sound (`deckcraft_format` writes it compact).
const NATIVE_MEDIA_TYPES: &[&[u8]] = &[b"\"contentType\":\"audio/", b"\"contentType\":\"video/"];

/// The guard for a zip (module doc).
fn zip(b: &[u8]) -> Result<Format, String> {
    let entries = directory(b)?;
    // What it is, by its names (and a native deck's `mimetype`).
    let native = match entries.iter().find(|e| e.name == "mimetype") {
        Some(e) if e.declared <= 256 => native_mimetype(&inflate(b, e, true, &mut |_| {})?.kept.unwrap_or_default()),
        _ => false,
    };
    let package = entries.iter().find(|e| e.name.eq_ignore_ascii_case("[Content_Types].xml"));
    let format = if native {
        Format::Native
    } else if package.is_some() {
        Format::Pptx
    } else {
        return Err(fail(Code::Unsupported, "this zip is not a presentation the deck engine reads (.pptx or .deckcraft)"));
    };
    // The package's declared content types.
    let (defaults, overrides) = match (format, package) {
        (Format::Pptx, Some(types)) => {
            if types.declared > MAX_TYPES_BYTES {
                return Err(held(format!("the presentation's content-type list is larger than the {MAX_TYPES_BYTES} bytes the deck service reads (#448)")));
            }
            let xml = inflate(b, types, true, &mut |_| {})?.kept.unwrap_or_default();
            content_types(&String::from_utf8_lossy(&xml))
        }
        _ => (Vec::new(), Vec::new()),
    };
    for e in entries.iter().filter(|e| !e.is_dir()) {
        let mut scan = Scan::new(NATIVE_MEDIA_TYPES);
        let watch_json = format == Format::Native && e.name == "presentation.json";
        let inflated = inflate(b, e, false, &mut |chunk| {
            if watch_json {
                scan.feed(chunk)
            }
        })?;
        // An Office package's embedded object may be a zip: nothing unzips it.
        if zip_signature(&inflated.head) && !(format == Format::Pptx && embedded_object(&e.name)) {
            return Err(held(format!("the presentation holds a zip inside its zip (`{}`), which the deck service does not open (#448)", shown(&e.name))));
        }
        let lower = e.name.to_ascii_lowercase();
        let declared_type = overrides
            .iter()
            .find(|(part, _)| *part == lower)
            .or_else(|| defaults.iter().find(|(ext, _)| *ext == e.extension()))
            .map(|(_, ct)| ct.as_str());
        let media = if MEDIA_EXTENSIONS.contains(&e.extension().as_str()) {
            Some("a movie or sound file")
        } else if declared_type.is_some_and(|ct| ct.starts_with("audio/") || ct.starts_with("video/")) {
            Some("a part its package says is a movie or sound")
        } else {
            media_signature(&inflated.head)
        };
        if let Some(what) = media {
            return Err(held(format!(
                "the presentation holds video or audio (`{}`, {what}), which the deck service does not open yet (#448)",
                shown(&e.name)
            )));
        }
        if scan.found {
            return Err(held("the presentation holds video or audio, which the deck service does not open yet (#448)"));
        }
    }
    Ok(format)
}

/// The guard for an OLE compound file: `protected:` when its directory holds
/// an `EncryptedPackage` stream, `unsupported:` otherwise (module doc).
fn ole(b: &[u8]) -> String {
    if encrypted_package(b) == Some(true) {
        fail(Code::Protected, "this presentation is protected with a password, and the deck service doesn't open those")
    } else {
        fail(Code::Unsupported, "this is a legacy .ppt or another OLE file, not a presentation the deck engine reads (.pptx or .deckcraft)")
    }
}

/// Whether the compound file's directory holds a stream named
/// `EncryptedPackage` (`None`: the file is no compound file it can read).
/// Every walk is bounded by the file's own sector count, so a looping chain
/// ends.
fn encrypted_package(b: &[u8]) -> Option<bool> {
    const FREE: u32 = 0xFFFF_FFFA;
    if b.len() < 512 || b[..8] != OLE {
        return None;
    }
    let sector = match (u16_at(b, 0x1A)?, u16_at(b, 0x1E)?) {
        (3, 9) => 512usize,
        (4, 12) => 4096usize,
        _ => return None,
    };
    let sectors = b.len() / sector;
    let sector_at = |s: u32| -> Option<&[u8]> {
        let at = (s as usize).checked_add(1)?.checked_mul(sector)?;
        b.get(at..at + sector)
    };
    // The FAT's sectors: the header's 109, then the DIFAT chain; never more
    // than the file has sectors, whatever the header says.
    let fat_count = (u32_at(b, 0x2C)? as usize).min(sectors);
    let mut fat: Vec<u32> = (0..109).filter_map(|i| u32_at(b, 0x4C + 4 * i)).filter(|s| *s < FREE).collect();
    let mut difat = u32_at(b, 0x44)?;
    let mut steps = 0;
    while difat < FREE && fat.len() < fat_count && steps <= sectors {
        let sec = sector_at(difat)?;
        let per = sector / 4 - 1;
        fat.extend((0..per).filter_map(|i| u32_at(sec, 4 * i)).filter(|s| *s < FREE));
        difat = u32_at(sec, 4 * per)?;
        steps += 1;
    }
    fat.truncate(fat_count);
    let next = |s: u32| -> Option<u32> {
        let per = sector / 4;
        let fs = *fat.get(s as usize / per)?;
        u32_at(sector_at(fs)?, (s as usize % per) * 4)
    };
    // The directory chain: 128-byte entries, each a UTF-16 name.
    let mut s = u32_at(b, 0x30)?;
    let mut steps = 0;
    while s < FREE {
        if steps > sectors {
            return None;
        }
        steps += 1;
        let sec = sector_at(s)?;
        for entry in sec.as_chunks::<128>().0 {
            let len = u16_at(entry, 0x40)? as usize;
            if entry[0x42] == 2 && (4..=64).contains(&len) {
                let units: Vec<u16> = entry[..len - 2].as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
                if String::from_utf16_lossy(&units).eq_ignore_ascii_case("EncryptedPackage") {
                    return Some(true);
                }
            }
        }
        s = next(s)?;
    }
    Some(false)
}

/// Hostile and honest files for the guard's tests, built here byte by byte
/// so a test can make a zip lie.
#[cfg(test)]
pub(crate) mod fixtures {
    use std::io::Write;

    /// One entry of a test zip.
    pub(crate) struct Part {
        pub name: String,
        pub data: Vec<u8>,
        pub deflate: bool,
        /// The uncompressed size the directory declares, when it lies.
        pub declared: Option<u64>,
    }

    impl Part {
        pub(crate) fn stored(name: &str, data: &[u8]) -> Part {
            Part { name: name.into(), data: data.to_vec(), deflate: false, declared: None }
        }
        pub(crate) fn deflated(name: &str, data: &[u8]) -> Part {
            Part { name: name.into(), data: data.to_vec(), deflate: true, declared: None }
        }
    }

    fn crc32(data: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFFu32;
        for &byte in data {
            crc ^= byte as u32;
            for _ in 0..8 {
                crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
            }
        }
        !crc
    }

    /// A zip of `parts`, honest unless a part says otherwise.
    pub(crate) fn zip(parts: &[Part]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut central = Vec::new();
        for p in parts {
            let body = if p.deflate {
                let mut e = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::fast());
                e.write_all(&p.data).unwrap();
                e.finish().unwrap()
            } else {
                p.data.clone()
            };
            let crc = crc32(&p.data);
            let declared = p.declared.unwrap_or(p.data.len() as u64) as u32;
            let method: u16 = if p.deflate { 8 } else { 0 };
            let offset = out.len() as u32;
            let name = p.name.as_bytes();
            out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
            out.extend_from_slice(&20u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&method.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0x21u16.to_le_bytes());
            out.extend_from_slice(&crc.to_le_bytes());
            out.extend_from_slice(&(body.len() as u32).to_le_bytes());
            out.extend_from_slice(&declared.to_le_bytes());
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(name);
            out.extend_from_slice(&body);
            central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
            central.extend_from_slice(&20u16.to_le_bytes());
            central.extend_from_slice(&20u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&method.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0x21u16.to_le_bytes());
            central.extend_from_slice(&crc.to_le_bytes());
            central.extend_from_slice(&(body.len() as u32).to_le_bytes());
            central.extend_from_slice(&declared.to_le_bytes());
            central.extend_from_slice(&(name.len() as u16).to_le_bytes());
            central.extend_from_slice(&[0u8; 12]);
            central.extend_from_slice(&offset.to_le_bytes());
            central.extend_from_slice(name);
        }
        let start = out.len() as u32;
        let count = parts.len() as u16;
        out.extend_from_slice(&central);
        out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        out.extend_from_slice(&[0u8; 4]);
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&(central.len() as u32).to_le_bytes());
        out.extend_from_slice(&start.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    /// An Office package's content-type list and its first part.
    pub(crate) fn package(extra: Vec<Part>, types: &str) -> Vec<u8> {
        let mut parts = vec![Part::deflated(
            "[Content_Types].xml",
            format!("<?xml version=\"1.0\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"xml\" ContentType=\"application/xml\"/>{types}</Types>").as_bytes(),
        )];
        parts.push(Part::deflated("ppt/presentation.xml", b"<p:presentation/>"));
        parts.extend(extra);
        zip(&parts)
    }

    /// The zip `bytes` again, every entry read back with the guard's own
    /// reader and deflated, with `extra` after them: an engine-written
    /// package with parts added.
    pub(crate) fn with_parts(bytes: &[u8], extra: Vec<Part>) -> Vec<u8> {
        let mut parts: Vec<Part> = super::directory(bytes)
            .unwrap()
            .iter()
            .filter(|e| !e.is_dir())
            .map(|e| Part::deflated(&e.name, &super::inflate(bytes, e, true, &mut |_| {}).unwrap().kept.unwrap()))
            .collect();
        parts.extend(extra);
        zip(&parts)
    }

    /// A native deck's skeleton: its `mimetype` and `presentation.json`.
    pub(crate) fn native(extra: Vec<Part>, json: &str) -> Vec<u8> {
        let mut parts = vec![Part::stored("mimetype", deckcraft_format::MIMETYPE.as_bytes()), Part::deflated("presentation.json", json.as_bytes())];
        parts.extend(extra);
        zip(&parts)
    }

    /// An OLE compound file (version 3, 512-byte sectors) whose directory
    /// holds `streams` after its root entry.
    pub(crate) fn ole(streams: &[&str]) -> Vec<u8> {
        let mut header = vec![0u8; 512];
        header[..8].copy_from_slice(&super::OLE);
        header[0x18..0x1A].copy_from_slice(&0x3Eu16.to_le_bytes());
        header[0x1A..0x1C].copy_from_slice(&3u16.to_le_bytes());
        header[0x1C..0x1E].copy_from_slice(&0xFFFEu16.to_le_bytes());
        header[0x1E..0x20].copy_from_slice(&9u16.to_le_bytes());
        header[0x20..0x22].copy_from_slice(&6u16.to_le_bytes());
        header[0x2C..0x30].copy_from_slice(&1u32.to_le_bytes());
        header[0x30..0x34].copy_from_slice(&1u32.to_le_bytes());
        header[0x38..0x3C].copy_from_slice(&4096u32.to_le_bytes());
        header[0x3C..0x40].copy_from_slice(&0xFFFF_FFFEu32.to_le_bytes());
        header[0x44..0x48].copy_from_slice(&0xFFFF_FFFEu32.to_le_bytes());
        for i in 0..109 {
            let v: u32 = if i == 0 { 0 } else { 0xFFFF_FFFF };
            header[0x4C + 4 * i..0x50 + 4 * i].copy_from_slice(&v.to_le_bytes());
        }
        // Sector 0: the FAT (itself, then the directory's two sectors).
        let mut fat = vec![0xFFu8; 512];
        fat[..4].copy_from_slice(&0xFFFF_FFFDu32.to_le_bytes());
        fat[4..8].copy_from_slice(&2u32.to_le_bytes());
        fat[8..12].copy_from_slice(&0xFFFF_FFFEu32.to_le_bytes());
        // Sectors 1 and 2: the directory, four entries a sector.
        let mut dir = vec![0u8; 1024];
        let names: Vec<(&str, u8)> = std::iter::once(("Root Entry", 5u8)).chain(streams.iter().map(|s| (*s, 2u8))).collect();
        for (i, (name, kind)) in names.iter().enumerate().take(8) {
            let e = &mut dir[i * 128..(i + 1) * 128];
            let units: Vec<u16> = name.encode_utf16().collect();
            for (k, u) in units.iter().enumerate() {
                e[k * 2..k * 2 + 2].copy_from_slice(&u.to_le_bytes());
            }
            e[0x40..0x42].copy_from_slice(&(((units.len() + 1) * 2) as u16).to_le_bytes());
            e[0x42] = *kind;
            e[0x44..0x50].copy_from_slice(&[0xFF; 12]);
        }
        let mut out = header;
        out.extend_from_slice(&fat);
        out.extend_from_slice(&dir);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::{native, ole, package, zip, Part};
    use super::*;

    #[track_caller]
    fn coded(r: Result<Format, String>, code: &str) -> String {
        let e = r.expect_err("refused");
        assert!(e.starts_with(&format!("{code}: ")), "expected {code}: {e}");
        e
    }

    #[test]
    fn honest_files_pass_and_say_what_they_are() {
        assert_eq!(check("deck.pptx", &package(vec![Part::stored("ppt/media/image1.png", b"\x89PNG\r\n\x1a\nrest")], "")), Ok(Format::Pptx));
        assert_eq!(check("deck.deckcraft", &native(vec![Part::stored("media/7", b"\xFF\xD8\xFF\xE0jpeg")], "{\"format\":\"deckcraft\"}")), Ok(Format::Native));
        assert_eq!(check("outline.txt", b"Title\n\tPoint\n"), Ok(Format::Outline));
        assert_eq!(check("Notes.MD", b"# x"), Ok(Format::Outline));
        // The engine's own writers' output passes.
        let deck = deckcraft_model::defaults::new_presentation(None);
        assert_eq!(check("x.deckcraft", &deckcraft_format::save(&deck).unwrap()), Ok(Format::Native));
        assert_eq!(check("x.pptx", &deckcraft_engine::cmd::file::save_bytes(&deck, "pptx").unwrap()), Ok(Format::Pptx));
    }

    #[test]
    fn what_the_engine_does_not_read_is_unsupported() {
        coded(check("legacy.ppt", &ole(&["PowerPoint Document", "Current User"])), "unsupported");
        coded(check("broken.ppt", &OLE), "unsupported");
        coded(check("doc.pdf", b"%PDF-1.7\n..."), "unsupported");
        coded(check("deck.key", &zip(&[Part::stored("Index/Document.iwa", b"\0\0\0"), Part::stored("Metadata/Properties.plist", b"<plist/>")])), "unsupported");
        coded(check("deck.odp", &zip(&[Part::stored("mimetype", b"application/vnd.oasis.opendocument.presentation")])), "unsupported");
        coded(check("deck.pptx", b"not a zip at all"), "unsupported");
        coded(check("image.png", b"\x89PNG\r\n\x1a\n"), "unsupported");
    }

    #[test]
    fn an_encrypted_package_is_protected() {
        let e = coded(check("locked.pptx", &ole(&["EncryptionInfo", "EncryptedPackage"])), "protected");
        assert!(e.contains("password"), "{e}");
        // Found past the first directory sector too.
        coded(check("locked.pptx", &ole(&["a", "b", "c", "d", "EncryptedPackage"])), "protected");
    }

    #[test]
    fn a_zip_beyond_the_guard_is_held() {
        // Ten thousand entries and one more.
        let many: Vec<Part> = (0..=MAX_ENTRIES).map(|i| Part::stored(&format!("f{i}"), b"")).collect();
        let e = coded(check("many.pptx", &zip(&many)), "held");
        assert!(e.contains("10001 entries"), "{e}");
        // At the cap the directory passes (and the names decide).
        let at_cap: Vec<Part> = (0..MAX_ENTRIES).map(|i| Part::stored(&format!("f{i}"), b"")).collect();
        coded(check("cap.zip", &zip(&at_cap)), "unsupported");
        // A zip bomb: 16 MiB of zeros in a few kilobytes.
        let bomb = package(vec![Part::deflated("ppt/slides/slide1.xml", &vec![0u8; 16 << 20])], "");
        assert!(bomb.len() < 1 << 20, "{} bytes", bomb.len());
        let e = coded(check("bomb.pptx", &bomb), "held");
        assert!(e.contains("to 1"), "{e}");
        // One entry declaring more than 64 MiB, and entries declaring more
        // than 256 MiB in all.
        let big = Part { declared: Some(MAX_ENTRY_BYTES + 1), ..Part::stored("ppt/big.bin", &vec![7u8; 1 << 20]) };
        coded(check("big.pptx", &package(vec![big], "")), "held");
        let parts: Vec<Part> = (0..5).map(|i| Part { declared: Some(60 << 20), ..Part::stored(&format!("ppt/p{i}.bin"), &vec![7u8; 1 << 20]) }).collect();
        let e = coded(check("total.pptx", &package(parts, "")), "held");
        assert!(e.contains("in all"), "{e}");
        // A name past 1,024 bytes.
        coded(check("name.pptx", &package(vec![Part::stored(&"n".repeat(MAX_NAME_BYTES + 1), b"x")], "")), "held");
    }

    #[test]
    fn a_lying_or_broken_zip_is_damaged() {
        // Declares 100 bytes, inflates to a megabyte.
        let liar = Part { declared: Some(100), ..Part::deflated("ppt/slides/slide1.xml", &vec![b' '; 1 << 20]) };
        let e = coded(check("liar.pptx", &package(vec![liar], "")), "damaged");
        assert!(e.contains("inflates past the 100 bytes"), "{e}");
        let stored_liar = Part { declared: Some(3), ..Part::stored("ppt/x.bin", b"0123456789") };
        coded(check("liar.pptx", &package(vec![stored_liar], "")), "damaged");
        // A directory that doesn't parse: cut in half.
        let good = package(vec![], "");
        coded(check("cut.pptx", &good[..good.len() / 2]), "damaged");
        coded(check("cut.pptx", b"PK\x03\x04garbage"), "damaged");
        // An entry whose data lies outside the file.
        let mut bad = package(vec![], "");
        let n = bad.len();
        // The last central entry's local offset (16 bytes before the end
        // record's start + the name) — rewrite every offset to past the end.
        for i in 0..n.saturating_sub(4) {
            if bad[i..i + 4] == 0x0201_4b50u32.to_le_bytes() {
                bad[i + 42..i + 46].copy_from_slice(&(n as u32 + 100).to_le_bytes());
            }
        }
        coded(check("outside.pptx", &bad), "damaged");
    }

    #[test]
    fn embedded_objects_pass_and_other_nested_zips_and_media_are_held() {
        let inner = zip(&[Part::deflated("[Content_Types].xml", b"<Types/>"), Part::deflated("xl/workbook.xml", b"<workbook/>")]);
        // A chart's workbook, an embedded document or object: zips that
        // nothing unzips, under the package's embeddings.
        let xlsx = "<Default Extension=\"xlsx\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet\"/>";
        assert_eq!(check("chart.pptx", &package(vec![Part::stored("ppt/embeddings/Microsoft_Excel_Worksheet1.xlsx", &inner)], xlsx)), Ok(Format::Pptx));
        let objects = vec![
            Part::deflated("ppt/embeddings/Microsoft_Word_Document.docx", &inner),
            Part::deflated("ppt/embeddings/oleObject1.bin", &inner),
            Part::stored("PPT/Embeddings/Sheet2.xlsx", &inner),
        ];
        assert_eq!(check("objects.pptx", &package(objects, "")), Ok(Format::Pptx));
        // A zip anywhere else stays held, and so does one only spelled to
        // look embedded, or one in a native deck, which embeds no objects.
        for name in ["ppt/media/x.zip", "ppt/slides/x.zip", "x.zip", "ppt/embeddings/../media/x.zip", "ppt/embeddings//x.xlsx", "ppt/embeddings/a/./x.xlsx", "ppt\\embeddings\\x.xlsx"] {
            let e = coded(check("nested.pptx", &package(vec![Part::stored(name, &inner)], "")), "held");
            assert!(e.contains("zip inside its zip"), "{name}: {e}");
        }
        coded(check("nested.deckcraft", &native(vec![Part::stored("ppt/embeddings/x.xlsx", &inner)], "{}")), "held");
        // An embedded object is held to every other bound: its declared
        // size, its name, its actual size, and the ratio.
        let big = Part { declared: Some(MAX_ENTRY_BYTES + 1), ..Part::stored("ppt/embeddings/big.xlsx", &vec![7u8; 1 << 20]) };
        coded(check("big.pptx", &package(vec![big], "")), "held");
        let long = format!("ppt/embeddings/{}.xlsx", "n".repeat(MAX_NAME_BYTES));
        coded(check("name.pptx", &package(vec![Part::stored(&long, &inner)], "")), "held");
        let liar = Part { declared: Some(100), ..Part::deflated("ppt/embeddings/liar.xlsx", &[inner.clone(), vec![0u8; 1 << 20]].concat()) };
        coded(check("liar.pptx", &package(vec![liar], "")), "damaged");
        // A workbook that is a zip bomb (16 MiB in a few kilobytes) is
        // refused by its directory entry's ratio, before anything is
        // inflated or unzipped: with its stream broken it is refused the
        // same way, where inflating it would have found it damaged.
        let name = "ppt/embeddings/bomb.xlsx";
        let mut bomb = package(vec![Part::deflated(name, &[inner.clone(), vec![0u8; 16 << 20]].concat())], "");
        assert!(bomb.len() < 1 << 20, "{} bytes", bomb.len());
        let e = coded(check("bomb.pptx", &bomb), "held");
        assert!(e.contains(name) && e.contains("100 to 1"), "{e}");
        let body = bomb.windows(name.len()).position(|w| w == name.as_bytes()).unwrap() + name.len();
        bomb[body + 64..body + 128].fill(0xFF);
        let e = coded(check("bomb.pptx", &bomb), "held");
        assert!(e.contains("100 to 1"), "{e}");
        // Media under the embeddings is still media.
        coded(check("movie.pptx", &package(vec![Part::stored("ppt/embeddings/clip.mp4", b"\0\0\0\x18ftypisom")], "")), "held");
        // Media by extension, by declared type, by signature; in a native
        // deck by signature and by its manifest's content type.
        let e = coded(check("movie.pptx", &package(vec![Part::stored("ppt/media/media1.mp4", b"\0\0\0\0data")], "")), "held");
        assert!(e.contains("video or audio") && e.contains("ppt/media/media1.mp4"), "{e}");
        coded(check("movie.pptx", &package(vec![Part::stored("ppt/media/clip.bin", b"0123")], "<Override PartName=\"/ppt/media/clip.bin\" ContentType=\"video/mp4\"/>")), "held");
        coded(check("sound.pptx", &package(vec![Part::stored("ppt/media/a.dat", b"0123")], "<Default Extension=\"dat\" ContentType='audio/x-dat'/>")), "held");
        coded(check("sound.pptx", &package(vec![Part::deflated("ppt/media/a.bin", b"RIFF\x10\0\0\0WAVEfmt ")], "")), "held");
        coded(check("movie.pptx", &package(vec![Part::stored("ppt/media/b.bin", b"\0\0\0\x18ftypisom")], "")), "held");
        coded(check("sound.deckcraft", &native(vec![Part::stored("media/9", b"ID3\x04\0\0\0")], "{}")), "held");
        let json = "{\"presentation\":{\"media\":[{\"id\":9,\"name\":\"a.wav\",\"contentType\":\"audio/wav\"}]}}";
        let e = coded(check("sound.deckcraft", &native(vec![], json)), "held");
        assert!(e.contains("video or audio"), "by the manifest's content type: {e}");
        // A type declared for an extension no part has is no media.
        assert_eq!(check("deck.pptx", &package(vec![], "<Default Extension=\"wav\" ContentType=\"audio/wav\"/>")), Ok(Format::Pptx));
    }

    #[test]
    fn the_streaming_scan_finds_a_needle_across_chunks() {
        const NEEDLES: &[&[u8]] = &[b"\"contentType\":\"audio/"];
        let mut scan = Scan::new(NEEDLES);
        scan.feed(b"{\"media\":[{\"content");
        assert!(!scan.found);
        scan.feed(b"Type\":\"audio/wav\"}]}");
        assert!(scan.found);
        let mut scan = Scan::new(NEEDLES);
        scan.feed(b"\"contentType\":\"image/png\"");
        assert!(!scan.found);
    }
}
