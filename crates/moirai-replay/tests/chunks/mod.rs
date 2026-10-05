//! A scanner fed as a reader feeds it: the anchor text in chunks that may end anywhere, inside a token, a UTF-8
//! sequence or a line ([F21 §1.3]; `moirai_files::scan::Scanner::feed`).

use moirai_files::scan::{Lang, Scanner};
use moirai_replay::scandiff::{Row, scanner_rows};

/// The rows of `t` scanned in chunks that end at the positions `cuts` gives (each taken modulo `len(t) + 1`, in any
/// order, repeats allowed); `None` when the scan fails ([F21 §2.7]).
pub fn scan_chunked(lang: Lang, t: &[u8], cuts: &[usize]) -> Option<Vec<Row>> {
    let mut ends: Vec<usize> = cuts.iter().map(|c| c % (t.len() + 1)).collect();
    ends.push(t.len());
    ends.sort_unstable();
    let mut s = Scanner::new(lang);
    let mut from = 0;
    for end in ends {
        s.feed(&t[from..end]);
        from = end;
    }
    s.finish().ok().map(|items| scanner_rows(&items))
}
