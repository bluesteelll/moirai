//! The items of a scanned text and the functions of [F21 §2] over them: name paths and segment equality (§2.2),
//! recordable name paths (§2.3), the scope a capture records (§2.4), how a scope resolves (§2.5) and item headers of
//! the same kind (§2.6).
//!
//! # Long names
//!
//! A name or qualifier longer than [`SCOPE_MAX_BYTES`] is **long**. No recordable name path holds one, and no
//! selector segment can equal one ([F21 §2.3, §6.1]), so [F21 §2.3]'s marker is all an item keeps of it: the flag,
//! its first [`LONG_PREFIX`] bytes for display, and, for a Rust `impl`, its bare name ([F21 §6.2]) when that is at
//! most [`SCOPE_MAX_BYTES`] bytes — the one way a selector can still match it. A long name equals no segment, makes
//! its name path unrecordable, and is written by [`Items::path_text`] as its prefix followed by the mark `%…`
//! ([`LONG_MARK`](super::scope::LONG_MARK)).
//!
//! # The text buffer
//!
//! Names and qualifiers live in one text buffer; an item holds where its name starts, its length, and where its
//! qualifier starts relative to it. A Markdown or TOML scanner copies each name in. The Rust scanner also uses the
//! buffer as a **stream**: while an `impl` header is being spelled, every token it reads is written to the end of the
//! buffer once ([F21 §3.2]'s spacing depends only on the token before, so the canonical spelling of any run of
//! consecutive tokens is one range of the stream). A header part is then a range, a header nested in another's group
//! is a range inside the enclosing header's range, and a name found inside a header's group is its token's range:
//! nested names overlap instead of being copied. Bytes past the kept mark belong to headers still being read; the
//! scanner drops them ([`Items::trim`]) once no header that is still under the cap needs them. So the buffer holds
//! each written token at most once, besides the names copied in (each one token outside every header) and the bare
//! names and 64-byte prefixes of long `impl` parts.
//!
//! # Tentative items
//!
//! A Rust item is a slot from its first token on; a pattern that fails kills it ([`Items::kill`]) in O(1), and the
//! scan's end compacts the list once ([`Items::shrink`]), giving the children of a killed item its nearest live
//! ancestor ([F21 §3.7] rule 4). Removing a slot at once would shift every later record, which costs the depth times
//! the items when many items sit inside failing groups.

use xxhash_rust::xxh3::xxh3_64_with_seed;

use super::scope::{Scope, Segment, leb_len, scope_text};
use super::{Lang, SCOPE_MAX_BYTES, SCOPE_MAX_SEGMENTS};

/// The `parent` of a top-level item.
pub(crate) const NO_PARENT: usize = usize::MAX;

/// The most bytes kept of a long name or qualifier, cut at a character boundary: enough to show which item a
/// refusal lists ([F21 §6.5]).
pub(crate) const LONG_PREFIX: usize = 64;

/// Flags of a [`Rec`]: bits 0–1 a Markdown heading's dash separator between numbering and name, 0 none, 1 `-`,
/// 2 `–`, 3 `—` ([F21 §4.7]).
const F_DASH: u8 = 0b11;
/// The name or qualifier holds `00`, `0A` or `0D`, which only a byte string that is no anchor text gives a Markdown
/// or TOML item; such a name path is not recordable (no valid scope value holds it, [F08 §10.3.1]).
const F_ODD: u8 = 1 << 2;
/// The name is long; the text buffer holds its prefix.
const F_NAME_LONG: u8 = 1 << 3;
/// The qualifier is long; the text buffer holds its prefix.
const F_QUAL_LONG: u8 = 1 << 4;
/// A long name or qualifier has a bare name, in [`Items::bares`].
const F_BARE: u8 = 1 << 5;
/// The item was killed while tentative; the scan's end compacts it away.
const F_DEAD: u8 = 1 << 6;

/// One item: where its name starts in the text buffer and where its qualifier starts relative to that; its lines,
/// parent, kind and flags. 40 bytes on a 64-bit target: lines stay 64-bit because a scanner reads a text of any
/// length, and a stored name or qualifier is at most [`SCOPE_MAX_BYTES`] bytes, so its length fits 16 bits and a
/// qualifier kept within 32 KiB of its name fits a 16-bit offset ([`Items::set_impl`] copies one that is not).
#[derive(Clone, Copy, Debug)]
struct Rec {
    start: u64,
    end: u64,
    at: usize,
    parent: usize,
    name_len: u16,
    qual_len: u16,
    qual_off: i16,
    skind: u8,
    flags: u8,
}

/// The bare names of an item's long name and qualifier ([F21 §6.2]), in the text buffer.
#[derive(Clone, Copy, Debug)]
struct Bares {
    item: usize,
    name: Option<BareAt>,
    qual: Option<BareAt>,
}

/// A bare name: its word in the text buffer and whether a `!` precedes it.
#[derive(Clone, Copy, Debug)]
struct BareAt {
    at: usize,
    len: u16,
    bang: bool,
}

/// The items of a scanned text in pre-order ([F21 §2.1]), with their names and qualifiers in one text buffer.
#[derive(Clone, Debug)]
pub struct Items {
    lang: Lang,
    recs: Vec<Rec>,
    text: String,
    /// The items with a long name or qualifier that has a bare name; by increasing item index once the scan is
    /// complete ([`Items::shrink`]).
    bares: Vec<Bares>,
    /// The bytes of `text` that items hold; a Rust scan's stream bytes past it belong to open `impl` headers.
    kept: usize,
    /// How many records are killed and not yet compacted away.
    dead: usize,
}

/// One item ([F21 §2.1]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Item<'a> {
    /// Its index in pre-order.
    pub index: usize,
    /// Rust: 1 `mod` … 9 `macro_rules`; Markdown: the heading level 1–6; TOML: 1 `table`, 2 `array_table`, 3 `key`
    /// ([F08 §10.3.1]).
    pub skind: u8,
    /// Non-empty, valid UTF-8; for an anchor text (no `00`, no `0D`) it holds no `00`, `0D` or `0A`. When
    /// [`name_long`](Item::name_long) is set, only its first bytes (at most 64).
    pub name: &'a str,
    /// A Rust trait impl's trait, a Markdown heading's numbering, else empty. When
    /// [`qual_long`](Item::qual_long) is set, only its first bytes (at most 64).
    pub qual: &'a str,
    /// The header line, 1-based.
    pub start: u64,
    /// The last line, at least `start`.
    pub end: u64,
    /// The index of the enclosing item, always less than `index`.
    pub parent: Option<usize>,
    /// The name is longer than [`SCOPE_MAX_BYTES`] bytes ([F21 §2.3]): the item's name path is not recordable and
    /// equals no other.
    pub name_long: bool,
    /// The qualifier is longer than [`SCOPE_MAX_BYTES`] bytes.
    pub qual_long: bool,
}

/// A Rust `impl` name or qualifier as a range of the stream ([F21 §3.7], §2.3): its kept bytes `[at, at + len)` —
/// the whole spelling, or the first [`LONG_PREFIX`] bytes of a long one — and, when long, its bare name ([F21 §6.2])
/// if that has at most [`SCOPE_MAX_BYTES`] bytes. An absent qualifier has `len` 0.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Span<'a> {
    pub(crate) at: usize,
    pub(crate) len: usize,
    pub(crate) long: bool,
    pub(crate) bare: Option<(bool, &'a str)>,
}

/// The bytes kept of a whole spelling ([F21 §2.3]): all of it, or the first [`LONG_PREFIX`] bytes of a long one.
fn kept_of(s: &str) -> (&str, bool) {
    if s.len() > SCOPE_MAX_BYTES {
        (&s[..prefix_len(s, LONG_PREFIX)], true)
    } else {
        (s, false)
    }
}

/// The offset of `q` from `at` when it fits 16 bits.
fn rel(at: usize, q: usize) -> Option<i16> {
    if q >= at {
        i16::try_from(q - at).ok()
    } else {
        i16::try_from(at - q).ok().map(|d| -d)
    }
}

/// A stored name or qualifier as matching reads it ([F21 §6.2–§6.4]).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Stored<'a> {
    /// The whole text, or the prefix of a long one.
    pub(crate) text: &'a str,
    pub(crate) long: bool,
    /// The bare name of a long Rust name or qualifier, when it has one of at most [`SCOPE_MAX_BYTES`] bytes.
    pub(crate) bare: Option<(bool, &'a str)>,
}

/// The length of the longest prefix of `s` of at most `max` bytes that ends at a character boundary.
pub(crate) fn prefix_len(s: &str, max: usize) -> usize {
    if s.len() <= max {
        return s.len();
    }
    let mut k = max;
    while !s.is_char_boundary(k) {
        k -= 1;
    }
    k
}

/// A stored length: at most [`SCOPE_MAX_BYTES`] bytes, which fits 16 bits.
fn len16(n: usize) -> u16 {
    debug_assert!(n <= SCOPE_MAX_BYTES, "a stored name is capped");
    u16::try_from(n).unwrap_or(u16::MAX)
}

impl Items {
    pub(crate) fn new(lang: Lang) -> Items {
        Items {
            lang,
            recs: Vec::new(),
            text: String::new(),
            bares: Vec::new(),
            kept: 0,
            dead: 0,
        }
    }

    /// The language of the scanned text.
    #[must_use]
    pub fn lang(&self) -> Lang {
        self.lang
    }

    /// The number of items.
    #[must_use]
    pub fn len(&self) -> usize {
        self.recs.len()
    }

    /// Whether the text has no items.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.recs.is_empty()
    }

    /// The item at pre-order index `i`.
    #[must_use]
    pub fn get(&self, i: usize) -> Option<Item<'_>> {
        (i < self.recs.len()).then(|| self.item(i))
    }

    /// The items in pre-order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = Item<'_>> + '_ {
        (0..self.recs.len()).map(|i| self.item(i))
    }

    /// The heap bytes held: 40 bytes per item on a 64-bit target, plus the text buffer — the names and qualifiers
    /// (at most [`SCOPE_MAX_BYTES`] bytes each, 64 for a long one), where a Rust `impl` name or qualifier nested in
    /// another's header shares the enclosing one's bytes — and the bare names of long ones.
    #[must_use]
    pub fn heap_bytes(&self) -> usize {
        self.recs.capacity() * size_of::<Rec>()
            + self.text.capacity()
            + self.bares.capacity() * size_of::<Bares>()
    }

    fn item(&self, i: usize) -> Item<'_> {
        let r = &self.recs[i];
        Item {
            index: i,
            skind: r.skind,
            name: self.name(i),
            qual: self.qual(i),
            start: r.start,
            end: r.end,
            parent: (r.parent != NO_PARENT).then_some(r.parent),
            name_long: r.flags & F_NAME_LONG != 0,
            qual_long: r.flags & F_QUAL_LONG != 0,
        }
    }

    pub(crate) fn name(&self, i: usize) -> &str {
        let r = &self.recs[i];
        &self.text[r.at..r.at + usize::from(r.name_len)]
    }

    pub(crate) fn qual(&self, i: usize) -> &str {
        let r = &self.recs[i];
        let d = usize::from(r.qual_off.unsigned_abs());
        let at = if r.qual_off < 0 { r.at - d } else { r.at + d };
        &self.text[at..at + usize::from(r.qual_len)]
    }

    fn bares_of(&self, i: usize) -> Option<&Bares> {
        if self.recs[i].flags & F_BARE == 0 {
            return None;
        }
        let k = self.bares.binary_search_by_key(&i, |b| b.item).ok()?;
        Some(&self.bares[k])
    }

    fn bare_text(&self, b: Option<BareAt>) -> Option<(bool, &str)> {
        b.map(|b| (b.bang, &self.text[b.at..b.at + usize::from(b.len)]))
    }

    /// Item `i`'s name as matching reads it.
    pub(crate) fn name_stored(&self, i: usize) -> Stored<'_> {
        Stored {
            text: self.name(i),
            long: self.recs[i].flags & F_NAME_LONG != 0,
            bare: self.bare_text(self.bares_of(i).and_then(|b| b.name)),
        }
    }

    /// Item `i`'s qualifier as matching reads it.
    pub(crate) fn qual_stored(&self, i: usize) -> Stored<'_> {
        Stored {
            text: self.qual(i),
            long: self.recs[i].flags & F_QUAL_LONG != 0,
            bare: self.bare_text(self.bares_of(i).and_then(|b| b.qual)),
        }
    }

    pub(crate) fn skind(&self, i: usize) -> u8 {
        self.recs[i].skind
    }

    pub(crate) fn parent(&self, i: usize) -> usize {
        self.recs[i].parent
    }

    fn long_parts(&self, i: usize) -> bool {
        self.recs[i].flags & (F_NAME_LONG | F_QUAL_LONG) != 0
    }

    /// A Markdown heading's text ([F21 §4.7] step 3): its numbering, a SP, its dash separator and a SP when it had
    /// one, and its name; without numbering, its name. `None` for an item that is not a Markdown heading, or whose
    /// name or numbering is long (only a prefix of it is kept).
    #[must_use]
    pub fn heading_text(&self, i: usize) -> Option<String> {
        if self.lang != Lang::Markdown || i >= self.recs.len() || self.long_parts(i) {
            return None;
        }
        let (qual, name) = (self.qual(i), self.name(i));
        if qual.is_empty() {
            return Some(name.to_owned());
        }
        let dash = dash_str(self.recs[i].flags & F_DASH);
        Some(format!("{qual} {dash}{name}"))
    }

    /// Whether `h` equals a Markdown heading's text, without building it; never for a long name or numbering.
    pub(crate) fn heading_text_is(&self, i: usize, h: &str) -> bool {
        if self.long_parts(i) {
            return false;
        }
        let (qual, name) = (self.qual(i), self.name(i));
        if qual.is_empty() {
            return h == name;
        }
        let dash = dash_str(self.recs[i].flags & F_DASH);
        h.len() == qual.len() + 1 + dash.len() + name.len()
            && h.starts_with(qual)
            && h[qual.len()..].starts_with(' ')
            && h[qual.len() + 1..].starts_with(dash)
            && h.ends_with(name)
    }

    // --- building ----------------------------------------------------------------------------------------------

    /// Appends an item of kind `skind` whose header line is `start`, with an empty name, `end` = `start`.
    pub(crate) fn push(&mut self, skind: u8, start: u64, parent: usize) -> usize {
        self.recs.push(Rec {
            start,
            end: start,
            at: 0,
            parent,
            name_len: 0,
            qual_len: 0,
            qual_off: 0,
            skind,
            flags: 0,
        });
        self.recs.len() - 1
    }

    pub(crate) fn set_skind(&mut self, i: usize, skind: u8) {
        self.recs[i].skind = skind;
    }

    pub(crate) fn set_end(&mut self, i: usize, end: u64) {
        self.recs[i].end = end;
    }

    pub(crate) fn end(&self, i: usize) -> u64 {
        self.recs[i].end
    }

    /// Sets the name and qualifier of item `i` from whole spellings, copied into the text buffer, keeping a prefix
    /// of a long one.
    pub(crate) fn set_names(&mut self, i: usize, name: &str, qual: &str) {
        let at = self.text.len();
        let (n, name_long) = kept_of(name);
        let (q, qual_long) = kept_of(qual);
        self.text.push_str(n);
        self.text.push_str(q);
        self.kept = self.text.len();
        let mut flags = self.recs[i].flags & F_DASH;
        if name_long {
            flags |= F_NAME_LONG;
        }
        if qual_long {
            flags |= F_QUAL_LONG;
        }
        if !name_long && !qual_long && odd(&self.text[at..]) {
            flags |= F_ODD;
        }
        let r = &mut self.recs[i];
        r.at = at;
        r.name_len = len16(n.len());
        r.qual_len = len16(q.len());
        // A kept name has at most `SCOPE_MAX_BYTES` bytes, so the qualifier right after it is within reach.
        r.qual_off = rel(at, at + n.len()).unwrap_or(0);
        r.flags = flags;
    }

    /// Sets the name of item `i` to what `write` appends to the text buffer, keeping a prefix of a long one; the
    /// qualifier is empty.
    pub(crate) fn set_name_with(&mut self, i: usize, write: impl FnOnce(&mut String)) {
        let at = self.text.len();
        write(&mut self.text);
        let (len, long) = {
            let (k, long) = kept_of(&self.text[at..]);
            (k.len(), long)
        };
        self.text.truncate(at + len);
        self.kept = self.text.len();
        let flags = if long {
            F_NAME_LONG
        } else if odd(&self.text[at..]) {
            F_ODD
        } else {
            0
        };
        let r = &mut self.recs[i];
        r.at = at;
        r.name_len = len16(len);
        r.qual_len = 0;
        r.qual_off = 0;
        r.flags = (r.flags & F_DASH) | flags;
    }

    /// Sets the name of Rust item `i` to the stream range `[at, end)`, one token's canonical spelling written there
    /// ([F21 §3.2]), keeping a prefix of a long one; the qualifier is empty. A Rust spelling never holds `00`, `0A`
    /// or `0D` (`canon` escapes them), so the range is not searched for them.
    pub(crate) fn set_name_ref(&mut self, i: usize, at: usize, end: usize) {
        let (len, long) = {
            let (k, long) = kept_of(&self.text[at..end]);
            (k.len(), long)
        };
        self.keep(at + len);
        let r = &mut self.recs[i];
        r.at = at;
        r.name_len = len16(len);
        r.qual_len = 0;
        r.qual_off = 0;
        r.flags = (r.flags & F_DASH) | if long { F_NAME_LONG } else { 0 };
    }

    /// Sets the name and qualifier of Rust `impl` item `i` to ranges of the stream ([F21 §3.7] rule 3), which it
    /// then keeps. A qualifier more than 32 KiB from the name — only a long one, whose prefix stayed where it went
    /// long while the header read on — is copied to the end, and the name after it when that is still out of reach:
    /// at most 64 bytes each, once per item. Bare names are copied to the end.
    pub(crate) fn set_impl(&mut self, i: usize, name: &Span<'_>, qual: &Span<'_>) {
        let (mut at, mut q_at) = (name.at, qual.at);
        let mut off = if qual.len == 0 {
            Some(0)
        } else {
            rel(at, q_at)
        };
        if off.is_none() {
            q_at = self.copy_to_end(q_at, qual.len);
            off = rel(at, q_at);
            if off.is_none() {
                at = self.copy_to_end(at, name.len);
                off = rel(at, q_at);
            }
        }
        self.keep(at + name.len);
        self.keep(q_at + qual.len);
        let mut flags = self.recs[i].flags & F_DASH;
        if name.long {
            flags |= F_NAME_LONG;
        }
        if qual.long {
            flags |= F_QUAL_LONG;
        }
        let nb = name.bare.filter(|_| name.long);
        let qb = qual.bare.filter(|_| qual.long);
        if nb.is_some() || qb.is_some() {
            flags |= F_BARE;
            let name = nb.map(|b| self.push_bare(b));
            let qual = qb.map(|b| self.push_bare(b));
            self.bares.push(Bares {
                item: i,
                name,
                qual,
            });
        }
        let r = &mut self.recs[i];
        r.at = at;
        r.name_len = len16(name.len);
        r.qual_len = len16(qual.len);
        r.qual_off = off.unwrap_or(0);
        r.flags = flags;
    }

    /// Copies `len` bytes at `from` to the end of the text buffer; their new offset.
    fn copy_to_end(&mut self, from: usize, len: usize) -> usize {
        let at = self.text.len();
        self.text.extend_from_within(from..from + len);
        self.kept = self.text.len();
        at
    }

    fn push_bare(&mut self, (bang, word): (bool, &str)) -> BareAt {
        let at = self.text.len();
        self.text.push_str(word);
        self.kept = self.text.len();
        BareAt {
            at,
            len: len16(word.len()),
            bang,
        }
    }

    pub(crate) fn set_dash(&mut self, i: usize, dash: u8) {
        let r = &mut self.recs[i];
        r.flags = (r.flags & !F_DASH) | (dash & F_DASH);
    }

    /// Kills tentative item `i`, whose name was never set, in O(1): [`Items::shrink`] drops it and gives its
    /// children its nearest live ancestor.
    pub(crate) fn kill(&mut self, i: usize) {
        debug_assert!(
            self.recs[i].flags & (F_BARE | F_DEAD) == 0,
            "a killed item has no name"
        );
        self.recs[i].flags |= F_DEAD;
        self.dead += 1;
    }

    /// Forgets every item.
    pub(crate) fn clear(&mut self) {
        self.recs.clear();
        self.text.clear();
        self.bares.clear();
        self.kept = 0;
        self.dead = 0;
    }

    /// The scan is complete: killed items go, the bare-name index is sorted, and the buffers give back their spare
    /// capacity.
    pub(crate) fn shrink(&mut self) {
        if !self.bares.is_sorted_by_key(|b| b.item) {
            self.bares.sort_unstable_by_key(|b| b.item);
        }
        self.compact();
        self.recs.shrink_to_fit();
        self.text.shrink_to_fit();
        self.bares.shrink_to_fit();
    }

    /// Drops the killed records in one pass, renumbering the live ones, their parents and the bare-name index. A
    /// parent precedes its children and lies on the chain of open ancestors of the record before them (pre-order),
    /// so a stack of that chain — old index and the new index it stands for (its own, or for a killed record its
    /// nearest live ancestor's) — resolves every parent; the stack is as deep as the items nest.
    fn compact(&mut self) {
        if self.dead == 0 {
            return;
        }
        let mut chain: Vec<(usize, usize)> = Vec::new();
        let (mut w, mut b) = (0, 0);
        for i in 0..self.recs.len() {
            let mut r = self.recs[i];
            let parent = if r.parent == NO_PARENT {
                chain.clear();
                NO_PARENT
            } else {
                while chain.last().is_some_and(|&(old, _)| old != r.parent) {
                    chain.pop();
                }
                debug_assert!(!chain.is_empty(), "a parent is an open ancestor");
                chain.last().map_or(NO_PARENT, |&(_, new)| new)
            };
            if r.flags & F_DEAD != 0 {
                chain.push((i, parent));
                continue;
            }
            r.parent = parent;
            if r.flags & F_BARE != 0 {
                while self.bares[b].item < i {
                    b += 1;
                }
                self.bares[b].item = w;
            }
            self.recs[w] = r;
            chain.push((i, w));
            w += 1;
        }
        self.recs.truncate(w);
        self.dead = 0;
    }

    // --- the Rust stream -----------------------------------------------------------------------------------------

    /// The text buffer, which a Rust scan also writes `impl` header tokens to.
    pub(crate) fn stream(&self) -> &str {
        &self.text
    }

    /// The text buffer for writing one token at its end.
    pub(crate) fn stream_mut(&mut self) -> &mut String {
        &mut self.text
    }

    /// Keeps the bytes before `end`: a name, a qualifier or a header part that stopped growing holds them.
    pub(crate) fn keep(&mut self, end: usize) {
        self.kept = self.kept.max(end);
    }

    /// Drops the stream bytes past the kept ones: no open header still needs them.
    pub(crate) fn trim(&mut self) {
        if self.text.len() > self.kept {
            self.text.truncate(self.kept);
        }
    }

    // --- §2.2: name paths ----------------------------------------------------------------------------------------

    /// The segment of item `i`, as far as it is kept.
    fn segment(&self, i: usize) -> Segment<'_> {
        Segment {
            skind: self.recs[i].skind,
            name: self.name(i),
            qual: self.qual(i),
        }
    }

    /// Segment equality ([F21 §2.2]): kind and name byte for byte, and for Rust and TOML the qualifier; a Markdown
    /// heading's numbering is not compared. A long name or compared qualifier equals nothing.
    fn seg_eq(&self, i: usize, s: &Segment<'_>) -> bool {
        let r = &self.recs[i];
        r.skind == s.skind
            && r.flags & F_NAME_LONG == 0
            && self.name(i) == s.name
            && (self.lang == Lang::Markdown
                || (r.flags & F_QUAL_LONG == 0 && self.qual(i) == s.qual))
    }

    /// Whether items `i` and `j`, which differ, have equal segments.
    fn same_segment(&self, i: usize, j: usize) -> bool {
        self.recs[j].flags & F_NAME_LONG == 0
            && (self.lang == Lang::Markdown || self.recs[j].flags & F_QUAL_LONG == 0)
            && self.seg_eq(i, &self.segment(j))
    }

    /// The indices of item `i` and its ancestors, outermost first: its name path.
    #[must_use]
    pub fn path(&self, i: usize) -> Vec<usize> {
        let mut v = Vec::new();
        let mut x = i;
        while x < self.recs.len() {
            v.push(x);
            x = self.recs[x].parent;
        }
        v.reverse();
        v
    }

    /// Whether item `i`'s name path equals the segments `segs` ([F21 §2.2]).
    fn path_is(&self, i: usize, segs: &[Segment<'_>]) -> bool {
        let mut x = i;
        for s in segs.iter().rev() {
            if x == NO_PARENT || !self.seg_eq(x, s) {
                return false;
            }
            x = self.recs[x].parent;
        }
        x == NO_PARENT
    }

    /// Whether items `i` and `j` have equal name paths ([F21 §2.2]).
    fn paths_equal(&self, mut i: usize, mut j: usize) -> bool {
        loop {
            match (i == NO_PARENT, j == NO_PARENT) {
                (true, true) => return true,
                (false, false) => {}
                _ => return false,
            }
            if i != j && !self.same_segment(i, j) {
                return false;
            }
            i = self.recs[i].parent;
            j = self.recs[j].parent;
        }
    }

    /// The items whose name path equals `scope` ([F21 §2.2]), at most `limit` of them; none when the language
    /// differs. A scope need not be recordable: an imported one may be longer than [`SCOPE_MAX_BYTES`] bytes
    /// ([F08 §10.3.1] bounds only its segments), and it is compared like any other. Only a segment whose name, or
    /// whose compared qualifier, is itself longer than [`SCOPE_MAX_BYTES`] bytes equals no item's: an item keeps
    /// [F21 §2.3]'s marker of such a name, not the name (spec finding of WP-63's review round 2, on [F21 §2.2]).
    #[must_use]
    pub fn matching(&self, scope: &Scope, limit: usize) -> Vec<usize> {
        let mut out = Vec::new();
        if scope.lang() != self.lang || limit == 0 {
            return out;
        }
        let segs: Vec<Segment<'_>> = scope.segments().collect();
        let Some(last) = segs.last() else { return out };
        for i in 0..self.recs.len() {
            if self.seg_eq(i, last) && self.path_is(i, &segs) {
                out.push(i);
                if out.len() == limit {
                    break;
                }
            }
        }
        out
    }

    /// The item that `scope` names, when it names exactly one ([F21 §2.2] "names one item").
    #[must_use]
    pub fn names_one(&self, scope: &Scope) -> Option<usize> {
        match self.matching(scope, 2)[..] {
            [i] => Some(i),
            _ => None,
        }
    }

    // --- §2.3: recordable name paths -----------------------------------------------------------------------------

    /// Whether item `i`'s name path is recordable: at most [`SCOPE_MAX_SEGMENTS`] segments and a scope value of at
    /// most [`SCOPE_MAX_BYTES`] bytes ([F21 §2.3]), so no name or qualifier on it is long. A name path whose names
    /// hold `00`, `0A` or `0D` — possible only for a Markdown or TOML scan of bytes that are no anchor text — is
    /// not recordable either, so no invalid scope value is ever built. The walk stops at the first bound it passes,
    /// after at most [`SCOPE_MAX_SEGMENTS`] + 1 items.
    #[must_use]
    pub fn recordable(&self, i: usize) -> bool {
        let (mut n, mut bytes) = (0, 2);
        let mut x = i;
        while x != NO_PARENT {
            let r = &self.recs[x];
            n += 1;
            if n > SCOPE_MAX_SEGMENTS || r.flags & (F_NAME_LONG | F_QUAL_LONG | F_ODD) != 0 {
                return false;
            }
            let (nl, ql) = (usize::from(r.name_len), usize::from(r.qual_len));
            bytes += 1 + leb_len(nl) + nl + leb_len(ql) + ql;
            if bytes > SCOPE_MAX_BYTES {
                return false;
            }
            x = r.parent;
        }
        true
    }

    /// Item `i`'s name path as a scope value, when it is recordable ([F21 §2.3], [F08 §10.3.1]).
    #[must_use]
    pub fn scope_of(&self, i: usize) -> Option<Scope> {
        (i < self.recs.len() && self.recordable(i)).then(|| self.path_scope(i))
    }

    fn path_scope(&self, i: usize) -> Scope {
        let path = self.path(i);
        Scope::encode(self.lang, path.iter().map(|&x| self.segment(x)))
    }

    /// Item `i`'s name path in the scope-text form of [F14 §5.6], recordable or not: the form in which a refusal
    /// lists items ([F21 §6.5]). A long name or qualifier is written as its first bytes followed by the mark `%…`,
    /// which no scope text of a real name holds, since it escapes every `%` as `%25`.
    #[must_use]
    pub fn path_text(&self, i: usize) -> String {
        let path = self.path(i);
        scope_text(
            self.lang,
            path.iter().map(|&x| {
                let f = self.recs[x].flags;
                (self.segment(x), f & F_NAME_LONG != 0, f & F_QUAL_LONG != 0)
            }),
        )
    }

    // --- §2.4: the scope of a capture ----------------------------------------------------------------------------

    /// Step 1 of [F21 §2.4] for a span of lines [`qs`, `qe`]: the deepest item that is an ancestor of, or equal to,
    /// every innermost item whose range contains the span. `None` when no range contains it or the innermost items
    /// have no common ancestor.
    #[must_use]
    pub fn scope_item(&self, qs: u64, qe: u64) -> Option<usize> {
        let n = self.recs.len();
        let mut in_c = vec![false; n];
        let mut outer = vec![false; n];
        for (i, r) in self.recs.iter().enumerate() {
            if r.start <= qs && qe <= r.end {
                in_c[i] = true;
                if r.parent != NO_PARENT {
                    outer[r.parent] = true;
                }
            }
        }
        let depth = self.depths();
        let mut acc: Option<usize> = None;
        for i in 0..n {
            if in_c[i] && !outer[i] {
                acc = Some(match acc {
                    None => i,
                    Some(a) => self.common(a, i, &depth)?,
                });
            }
        }
        acc
    }

    /// Step 2 of [F21 §2.4]: the name path of the first item, from `from` through its ancestors, whose name path is
    /// recordable and names one item of the text; `None` when there is none.
    #[must_use]
    pub fn recorded_scope(&self, from: usize) -> Option<Scope> {
        if from >= self.recs.len() {
            return None;
        }
        let index = self.path_index();
        let mut x = from;
        while x != NO_PARENT {
            if index.depth[x] <= SCOPE_MAX_SEGMENTS && self.recordable(x) && self.unique(x, &index)
            {
                return Some(self.path_scope(x));
            }
            x = self.recs[x].parent;
        }
        None
    }

    /// [F21 §2.4] for a span of lines [`qs`, `qe`] of a capture in any form but `symbol` and `heading`: the scope
    /// recorded, or `None` (`has_scope` clear).
    #[must_use]
    pub fn capture_scope(&self, qs: u64, qe: u64) -> Option<Scope> {
        self.recorded_scope(self.scope_item(qs, qe)?)
    }

    /// Whether item `x`'s name path names one item of the text.
    fn unique(&self, x: usize, index: &PathIndex) -> bool {
        let mut seen = 0;
        for j in 0..self.recs.len() {
            if index.hash[j] == index.hash[x]
                && index.depth[j] == index.depth[x]
                && self.paths_equal(j, x)
            {
                seen += 1;
                if seen == 2 {
                    return false;
                }
            }
        }
        seen == 1
    }

    /// The items whose name path equals item `i`'s, `i` included.
    pub(crate) fn path_twins(&self, i: usize) -> Vec<usize> {
        let index = self.path_index();
        (0..self.recs.len())
            .filter(|&j| {
                index.hash[j] == index.hash[i]
                    && index.depth[j] == index.depth[i]
                    && self.paths_equal(j, i)
            })
            .collect()
    }

    fn depths(&self) -> Vec<usize> {
        let mut d = vec![0usize; self.recs.len()];
        for i in 0..self.recs.len() {
            let p = self.recs[i].parent;
            d[i] = if p == NO_PARENT { 1 } else { d[p] + 1 };
        }
        d
    }

    /// Depths and name-path hashes of every item, in one pass (a parent precedes its children). A long name hashes
    /// its prefix; equality is decided by [`Items::paths_equal`].
    fn path_index(&self) -> PathIndex {
        let n = self.recs.len();
        let mut depth = vec![0usize; n];
        let mut hash = vec![0u64; n];
        for i in 0..n {
            let r = &self.recs[i];
            let (pd, ph) = if r.parent == NO_PARENT {
                (0, 0)
            } else {
                (depth[r.parent], hash[r.parent])
            };
            depth[i] = pd + 1;
            let mut h = xxh3_64_with_seed(self.name(i).as_bytes(), ph ^ (u64::from(r.skind) << 56));
            if self.lang != Lang::Markdown {
                h = xxh3_64_with_seed(
                    self.qual(i).as_bytes(),
                    h.rotate_left(17) ^ 0x9E37_79B9_7F4A_7C15,
                );
            }
            hash[i] = h;
        }
        PathIndex { depth, hash }
    }

    /// The deepest common ancestor-or-self of `a` and `b`.
    fn common(&self, mut a: usize, mut b: usize, depth: &[usize]) -> Option<usize> {
        while depth[a] > depth[b] {
            a = self.recs[a].parent;
        }
        while depth[b] > depth[a] {
            b = self.recs[b].parent;
        }
        while a != b {
            a = self.recs[a].parent;
            b = self.recs[b].parent;
            if a == NO_PARENT || b == NO_PARENT {
                return None;
            }
        }
        Some(a)
    }

    // --- §2.5 and §2.6 ------------------------------------------------------------------------------------------

    /// [F21 §2.5]: the item y that `scope` names in this text when it resolves uniquely — the language is the
    /// scope's and exactly one item has its name path. Its lines [`start`, `end`] give the byte range
    /// `[start(y.start) .. end(y.end))` of `N(t)` ([F20 §2.5]). `None` narrows nothing: the whole text is searched.
    /// A scope with a name longer than [`SCOPE_MAX_BYTES`] bytes names no item ([`Items::matching`]).
    #[must_use]
    pub fn resolve(&self, scope: &Scope) -> Option<Item<'_>> {
        self.names_one(scope).map(|i| self.item(i))
    }

    /// [F21 §2.6]: the items whose headers count for a `symbol` or `heading` anchor with scope `scope` — those of
    /// the scope's language and of the kind of its last segment. Each header is `header(start, k)` of [F20 §2.8].
    pub fn same_kind<'a>(&'a self, scope: &Scope) -> impl Iterator<Item = Item<'a>> + 'a {
        let skind = scope
            .last()
            .map(|s| s.skind)
            .filter(|_| scope.lang() == self.lang);
        (0..self.recs.len())
            .filter(move |&i| Some(self.recs[i].skind) == skind)
            .map(|i| self.item(i))
    }
}

/// Whether a name holds a byte that no text value of a scope may hold ([F08 §10.3.1]).
fn odd(s: &str) -> bool {
    s.bytes().any(|b| matches!(b, 0x00 | 0x0A | 0x0D))
}

/// Per-item depth and name-path hash.
struct PathIndex {
    depth: Vec<usize>,
    hash: Vec<u64>,
}

fn dash_str(dash: u8) -> &'static str {
    match dash {
        1 => "- ",
        2 => "\u{2013} ",
        3 => "\u{2014} ",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds items from (skind, name, qual, start, end, parent).
    fn build(lang: Lang, v: &[(u8, &str, &str, u64, u64, usize)]) -> Items {
        let mut it = Items::new(lang);
        for &(k, n, q, s, e, p) in v {
            let i = it.push(k, s, p);
            it.set_names(i, n, q);
            it.set_end(i, e);
        }
        it.shrink();
        it
    }

    #[test]
    fn records_are_compact() {
        #[cfg(target_pointer_width = "64")]
        assert_eq!(size_of::<Rec>(), 40);
    }

    #[test]
    fn killed_items_give_their_children_the_nearest_live_ancestor() {
        let mut it = Items::new(Lang::Rust);
        let a = it.push(1, 1, NO_PARENT);
        it.set_names(a, "a", "");
        let t1 = it.push(0, 2, a);
        let t2 = it.push(0, 2, t1);
        let f = it.push(3, 3, t2);
        it.set_names(f, "f", "");
        let t3 = it.push(0, 4, NO_PARENT);
        let g = it.push(3, 5, t3);
        it.set_names(g, "g", "");
        for i in [t2, t3, t1] {
            it.kill(i);
        }
        it.shrink();
        let got: Vec<(&str, Option<usize>)> = it.iter().map(|x| (x.name, x.parent)).collect();
        assert_eq!(got, [("a", None), ("f", Some(0)), ("g", None)]);
    }

    /// The old removal, one record at a time with every later record shifted: the reference for compaction.
    fn remove_naive(recs: &mut Vec<(usize, bool)>, i: usize) {
        let (gone_parent, _) = recs.remove(i);
        for r in &mut recs[i..] {
            if r.0 == i {
                r.0 = gone_parent;
            } else if r.0 != NO_PARENT && r.0 > i {
                r.0 -= 1;
            }
        }
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig {
            failure_persistence: None,
            ..proptest::prelude::ProptestConfig::default()
        })]

        #[test]
        fn compaction_equals_removal_one_at_a_time(
            shape in proptest::collection::vec((0usize..6, proptest::bool::ANY), 1..80),
        ) {
            // A pre-order forest: each record's parent is on the chain of open ancestors of the record before it,
            // `up` levels up from it (the whole chain: a root).
            let mut it = Items::new(Lang::Rust);
            let mut chain: Vec<usize> = Vec::new();
            let mut reference: Vec<(usize, bool)> = Vec::new();
            for (k, &(up, dead)) in shape.iter().enumerate() {
                chain.truncate(chain.len().saturating_sub(up));
                let parent = chain.last().copied().unwrap_or(NO_PARENT);
                let i = it.push(3, k as u64 + 1, parent);
                if !dead {
                    it.set_names(i, &format!("n{k}"), "");
                }
                chain.push(i);
                reference.push((parent, dead));
            }
            for (k, &(_, dead)) in shape.iter().enumerate().rev() {
                if dead {
                    it.kill(k);
                    remove_naive(&mut reference, k);
                }
            }
            it.shrink();
            let got: Vec<usize> = it.iter().map(|x| x.parent.unwrap_or(NO_PARENT)).collect();
            let want: Vec<usize> = reference.iter().map(|r| r.0).collect();
            proptest::prop_assert_eq!(got, want);
        }
    }

    /// A stream holding `Tr for ` and then `name`; the items name ranges of it.
    fn stream_items(name: &str) -> Items {
        let mut it = Items::new(Lang::Rust);
        it.stream_mut().push_str("Tr for ");
        it.stream_mut().push_str(name);
        it
    }

    #[test]
    fn killing_keeps_bare_names_with_their_items() {
        let long = format!("Foo<{}>", "x".repeat(5000));
        let mut it = stream_items(&long);
        // The name went long: its first 64 bytes stay, and the stream past them goes once nothing grows.
        it.keep(7 + LONG_PREFIX);
        it.trim();
        let a = it.push(2, 1, NO_PARENT);
        let gone = it.push(3, 1, NO_PARENT);
        let b = it.push(2, 2, NO_PARENT);
        let name = Span {
            at: 7,
            len: LONG_PREFIX,
            long: true,
            bare: Some((false, "Foo")),
        };
        let qual = Span {
            at: 0,
            len: 2,
            long: false,
            bare: None,
        };
        // Headers end innermost first, so the bare-name index is filled out of order.
        for i in [b, a] {
            it.set_impl(i, &name, &qual);
        }
        it.kill(gone);
        it.shrink();
        assert_eq!(it.len(), 2);
        for i in [0, 1] {
            let n = it.name_stored(i);
            assert!(n.long && n.text.len() <= LONG_PREFIX && n.text.starts_with("Foo<x"));
            assert_eq!(n.bare, Some((false, "Foo")));
            assert_eq!(it.qual(i), "Tr");
            assert!(!it.qual_stored(i).long);
        }
        // The stream past the kept prefix is gone: 7 + 64 bytes and the bare names.
        assert_eq!(it.stream().len(), 7 + LONG_PREFIX + 2 * 3);
    }

    #[test]
    fn nested_impl_names_share_the_stream() {
        // `impl A<{ impl B<{}> {} }> {}`: the inner name is a range inside the outer one.
        let mut it = Items::new(Lang::Rust);
        it.stream_mut().push_str("A<{impl B<{}>{}}>");
        let outer = it.push(2, 1, NO_PARENT);
        let inner = it.push(2, 1, outer);
        let none = Span {
            at: 0,
            len: 0,
            long: false,
            bare: None,
        };
        let span = |at, len| Span {
            at,
            len,
            long: false,
            bare: None,
        };
        it.set_impl(inner, &span(8, 5), &none);
        it.set_impl(outer, &span(0, 17), &none);
        it.shrink();
        assert_eq!(it.name(0), "A<{impl B<{}>{}}>");
        assert_eq!(it.name(1), "B<{}>");
        assert_eq!(it.stream().len(), 17);
    }

    #[test]
    fn a_qualifier_out_of_reach_is_copied() {
        // A long qualifier's prefix far before the name: copied behind it; a long name far behind the end: both.
        let gap = "g".repeat(40_000);
        let mut it = Items::new(Lang::Rust);
        it.stream_mut().push_str(&format!("Q{gap}N"));
        let i = it.push(2, 1, NO_PARENT);
        let q = Span {
            at: 0,
            len: 1,
            long: true,
            bare: Some((false, "Q")),
        };
        it.set_impl(
            i,
            &Span {
                at: 40_001,
                len: 1,
                long: false,
                bare: None,
            },
            &q,
        );
        assert_eq!((it.name(i), it.qual(i)), ("N", "Q"));
        let mut it = Items::new(Lang::Rust);
        it.stream_mut().push_str(&format!("Q{gap}N{gap}"));
        let i = it.push(2, 1, NO_PARENT);
        it.set_impl(
            i,
            &Span {
                at: 40_001,
                len: 1,
                long: true,
                bare: None,
            },
            &q,
        );
        assert_eq!((it.name(i), it.qual(i)), ("N", "Q"));
        assert!(it.name_stored(i).long && it.qual_stored(i).bare == Some((false, "Q")));
    }

    #[test]
    fn scope_items_and_twins() {
        // mod a (1–6) { fn g (3–3); fn g (5–5) }; fn h (7–7); fn k (7–7).
        let it = build(
            Lang::Rust,
            &[
                (1, "a", "", 1, 6, NO_PARENT),
                (3, "g", "", 3, 3, 0),
                (3, "g", "", 5, 5, 0),
                (3, "h", "", 7, 7, NO_PARENT),
                (3, "k", "", 7, 7, NO_PARENT),
            ],
        );
        assert_eq!(it.scope_item(5, 5), Some(2));
        assert_eq!(
            it.capture_scope(5, 5).map(|s| s.to_string()),
            Some("rust:mod a".to_owned())
        );
        assert_eq!(it.scope_item(7, 7), None);
        assert_eq!(it.scope_item(3, 5), Some(0));
        assert_eq!(it.scope_item(8, 8), None);
        assert_eq!(it.path_twins(1), [1, 2]);
    }

    #[test]
    fn markdown_numbering_is_not_compared() {
        let it = build(
            Lang::Markdown,
            &[
                (1, "Design", "1", 1, 9, NO_PARENT),
                (2, "Storage", "1.4", 3, 9, 0),
            ],
        );
        let s = Scope::encode(
            Lang::Markdown,
            [
                Segment {
                    skind: 1,
                    name: "Design",
                    qual: "1",
                },
                Segment {
                    skind: 2,
                    name: "Storage",
                    qual: "1.1",
                },
            ]
            .into_iter(),
        );
        assert_eq!(it.names_one(&s), Some(1));
        assert_eq!(it.resolve(&s).map(|y| (y.start, y.end)), Some((3, 9)));
        let toml = Scope::encode(
            Lang::Toml,
            [Segment {
                skind: 1,
                name: "Design",
                qual: "",
            }]
            .into_iter(),
        );
        assert_eq!(it.names_one(&toml), None);
    }

    #[test]
    fn recordable_limits() {
        // 2 + 1 + 2 + 4,090 + 1 = 4,096 bytes: recordable; one more byte is not.
        let edge = "x".repeat(4090);
        let long = "x".repeat(4091);
        let it = build(
            Lang::Rust,
            &[
                (3, &edge, "", 1, 1, NO_PARENT),
                (3, &long, "", 2, 2, NO_PARENT),
            ],
        );
        assert!(it.recordable(0));
        assert!(!it.recordable(1));
        assert_eq!(it.scope_of(0).map(|s| s.as_bytes().len()), Some(4096));
        assert_eq!(it.capture_scope(2, 2), None);
        let mut v = Vec::new();
        for d in 0..70usize {
            v.push((
                1u8,
                "m",
                "",
                1u64,
                1u64,
                if d == 0 { NO_PARENT } else { d - 1 },
            ));
        }
        let deep = build(Lang::Rust, &v);
        assert!(deep.recordable(63));
        assert!(!deep.recordable(64));
        assert_eq!(deep.recorded_scope(69).map(|s| s.len()), Some(64));
    }

    #[test]
    fn long_names_keep_a_prefix_and_equal_nothing() {
        // 4,096 bytes are kept whole; 4,097 make a long name, of which 64 bytes are kept.
        let whole = "é".repeat(2048);
        let long = format!("{whole}x");
        let it = build(
            Lang::Rust,
            &[
                (3, &whole, "", 1, 1, NO_PARENT),
                (3, &long, "", 2, 2, NO_PARENT),
                (3, &long, "", 3, 3, NO_PARENT),
            ],
        );
        let (a, b) = (it.get(0).unwrap(), it.get(1).unwrap());
        assert!(!a.name_long && a.name.len() == 4096);
        assert!(b.name_long && b.name == "é".repeat(32));
        assert!(!it.recordable(1));
        // Two long names spelled alike are not twins: a long name equals no segment.
        assert_eq!(it.path_twins(1), [1]);
        assert_eq!(
            it.path_text(1),
            format!("rust:fn {}%\u{2026}", "é".repeat(32))
        );
        assert_eq!(it.capture_scope(2, 2), None);
        assert!(it.heap_bytes() < 4096 + 200 + 3 * 40);
        // A Markdown heading with a long numbering keeps a name that still matches by its segment.
        let md = build(
            Lang::Markdown,
            &[(1, "Design", &"1.".repeat(3000), 1, 1, NO_PARENT)],
        );
        let s = Scope::encode(
            Lang::Markdown,
            [Segment {
                skind: 1,
                name: "Design",
                qual: "1",
            }]
            .into_iter(),
        );
        assert_eq!(md.names_one(&s), Some(0));
        assert!(!md.recordable(0));
        assert_eq!(md.heading_text(0), None);
        assert!(!md.heading_text_is(0, "Design"));
    }

    #[test]
    fn a_scope_over_the_byte_bound_resolves_unless_a_name_is_long() {
        // A name over the cap: the item keeps a marker, which equals no segment.
        let long = "x".repeat(5000);
        let it = build(Lang::Rust, &[(3, &long, "", 1, 1, NO_PARENT)]);
        let s = Scope::from_segments(
            Lang::Rust,
            &[Segment {
                skind: 3,
                name: &long,
                qual: "",
            }],
        )
        .unwrap();
        assert_eq!(it.resolve(&s), None);
        // A qualifier over the cap likewise, for Rust; a Markdown numbering is never compared.
        let tr = build(Lang::Rust, &[(2, "S", &long, 1, 1, NO_PARENT)]);
        let s = Scope::from_segments(
            Lang::Rust,
            &[Segment {
                skind: 2,
                name: "S",
                qual: &long,
            }],
        )
        .unwrap();
        assert_eq!(tr.resolve(&s), None);
        let md = build(Lang::Markdown, &[(1, "Design", &long, 1, 3, NO_PARENT)]);
        let s = Scope::from_segments(
            Lang::Markdown,
            &[Segment {
                skind: 1,
                name: "Design",
                qual: &long,
            }],
        )
        .unwrap();
        assert!(s.as_bytes().len() > 4096);
        assert_eq!(md.resolve(&s).map(|y| y.index), Some(0));
        // 64 segments of 70 bytes: every name is short, the value is not recordable, and it names the deepest item.
        let seg = "y".repeat(70);
        let mut v = Vec::new();
        for d in 0..64usize {
            v.push((1u8, seg.as_str(), "", 1u64, 1u64, d.wrapping_sub(1)));
        }
        let deep = build(Lang::Rust, &v);
        let segs: Vec<Segment<'_>> = (0..64)
            .map(|_| Segment {
                skind: 1,
                name: &seg,
                qual: "",
            })
            .collect();
        let s = Scope::from_segments(Lang::Rust, &segs).unwrap();
        assert!(s.as_bytes().len() > 4096);
        assert!(!deep.recordable(63));
        assert_eq!(deep.resolve(&s).map(|y| y.index), Some(63));
        assert_eq!(deep.scope_of(63), None);
    }

    #[test]
    fn heading_texts() {
        let mut it = build(
            Lang::Markdown,
            &[
                (1, "R4 constants", "20", 1, 1, NO_PARENT),
                (1, "Plain", "", 2, 2, NO_PARENT),
            ],
        );
        it.set_dash(0, 3);
        assert_eq!(
            it.heading_text(0).as_deref(),
            Some("20 \u{2014} R4 constants")
        );
        assert!(it.heading_text_is(0, "20 \u{2014} R4 constants"));
        assert!(!it.heading_text_is(0, "20 R4 constants"));
        assert!(it.heading_text_is(1, "Plain"));
    }
}
