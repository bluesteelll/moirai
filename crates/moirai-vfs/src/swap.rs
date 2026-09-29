//! The swap intent of the emulated `swap_dirs` ([OS/fs §4.9.2, §4.9.3]): the names it uses in `a_parent` and the
//! byte-exact codec of the intent file.
//!
//! `swap_dirs` and `swap_recover` are [`crate::StoreFs`] methods that each implementation performs with its own calls,
//! but the intent is an on-disk format: the OS layer and the simulator must write, and accept, exactly the same files.
//! Both therefore use this one codec, as they use the one grant table ([OS/README §2.1]: the simulator cannot depend on
//! `moirai-os`, and a behaviour both sides must share is only shared if it is one piece of code). The module is pure: no
//! I/O, no clock.

use crate::fs::FileIdentity;
use crate::xxh3::xxh3_64;

/// The content of a swap intent file ([OS/fs §4.9.3]): the identities of `A` and `B` before step 4 and the absolute
/// paths of `A`, `B` and `T` in the machine-local form of [80 §2.10] P12 (canonical, `/` separators; Windows `X:/…` with
/// the drive letter upper-cased, `//server/share/…` for UNC).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SwapIntent {
    /// The identity of `A` (`a_parent/a`) before step 4.
    pub a_id: FileIdentity,
    /// The identity of `B` (`b_parent/b`) before step 4.
    pub b_id: FileIdentity,
    /// The absolute path of `A`.
    pub a_path: String,
    /// The absolute path of `B`.
    pub b_path: String,
    /// The absolute path of `T` (`a_parent/<a>.swap-old`).
    pub t_path: String,
}

/// A little-endian `u16` at `at`, widened.
fn u16_at(b: &[u8], at: usize) -> usize {
    usize::from(u16::from_le_bytes([b[at], b[at + 1]]))
}

/// The `FileIdentity` of [OS/fs §2.6] at `b[..24]`.
fn id_at(b: &[u8]) -> FileIdentity {
    let mut w = [0u8; FileIdentity::LEN];
    w.copy_from_slice(&b[..FileIdentity::LEN]);
    FileIdentity::from_bytes(&w)
}

/// The zero padding after `p` bytes of header and paths ([OS/fs §4.9.3] `pad`).
const fn pad_after(p: usize) -> usize {
    (8 - p % 8) % 8
}

impl SwapIntent {
    /// The magic at offset 0: ASCII `MSWP`.
    pub const MAGIC: [u8; 4] = *b"MSWP";
    /// The format version at offset 4.
    pub const VERSION: u16 = 1;
    /// The fixed header's length.
    pub const HEADER_LEN: usize = 64;
    /// The longest path an intent holds, in bytes (`a_len`, `b_len`, `t_len` are 1–4096).
    pub const MAX_PATH_LEN: usize = 4096;
    /// The longest intent file: the header, three longest paths, the widest padding and the checksum. A reader that finds
    /// a longer file treats it as unreadable without reading it.
    pub const MAX_LEN: u64 = (Self::HEADER_LEN + 3 * Self::MAX_PATH_LEN + 7 + 8) as u64;

    /// The two names the emulated form uses in `a_parent` for the entry `a` ([OS/fs §4.9.2]): the intent `I` =
    /// `<a>.swap` and the temporary `T` = `<a>.swap-old`.
    pub fn side_names(a: &str) -> (String, String) {
        (format!("{a}.swap"), format!("{a}.swap-old"))
    }

    /// Whether `path` has a length an intent can hold: 1–4096 bytes ([OS/fs §4.9.3]).
    pub const fn path_fits(path: &str) -> bool {
        !path.is_empty() && path.len() <= Self::MAX_PATH_LEN
    }

    /// The file's bytes ([OS/fs §4.9.3]): the 64-byte header, the three paths, zero padding to a multiple of 8, and the
    /// XXH3-64 (seed 0) of all of that, little-endian. `None` if a path does not fit ([`SwapIntent::path_fits`]).
    pub fn encode(&self) -> Option<Vec<u8>> {
        let paths = [&self.a_path, &self.b_path, &self.t_path];
        if !paths.iter().all(|p| Self::path_fits(p)) {
            return None;
        }
        let p = Self::HEADER_LEN + paths.iter().map(|s| s.len()).sum::<usize>();
        let pad = pad_after(p);
        let mut b = Vec::with_capacity(p + pad + 8);
        b.extend_from_slice(&Self::MAGIC);
        b.extend_from_slice(&Self::VERSION.to_le_bytes());
        b.extend_from_slice(&0u16.to_le_bytes());
        b.extend_from_slice(&self.a_id.to_bytes());
        b.extend_from_slice(&self.b_id.to_bytes());
        for s in paths {
            // Fits: `path_fits` bounds every length by 4096.
            b.extend_from_slice(&(s.len() as u16).to_le_bytes());
        }
        b.extend_from_slice(&0u16.to_le_bytes());
        for s in paths {
            b.extend_from_slice(s.as_bytes());
        }
        b.resize(p + pad, 0);
        let sum = xxh3_64(&b);
        b.extend_from_slice(&sum.to_le_bytes());
        Some(b)
    }

    /// The intent in `b`, if the length, the magic, `version = 1`, the reserved fields, the padding, the UTF-8 paths and
    /// the checksum all check ([OS/fs §4.9.3]); `None` otherwise: the intent is unreadable, and `swap_recover` acts by
    /// [OS/fs §4.9.4]'s row "`I` unreadable".
    pub fn decode(b: &[u8]) -> Option<SwapIntent> {
        let h = Self::HEADER_LEN;
        if b.len() < h + 8
            || b[..4] != Self::MAGIC
            || u16_at(b, 4) != usize::from(Self::VERSION)
            || u16_at(b, 6) != 0
        {
            return None;
        }
        let lens = [u16_at(b, 56), u16_at(b, 58), u16_at(b, 60)];
        if u16_at(b, 62) != 0 || lens.iter().any(|&l| l == 0 || l > Self::MAX_PATH_LEN) {
            return None;
        }
        let p = h + lens.iter().sum::<usize>();
        let pad = pad_after(p);
        if b.len() != p + pad + 8 || b[p..p + pad].iter().any(|&x| x != 0) {
            return None;
        }
        let mut sum = [0u8; 8];
        sum.copy_from_slice(&b[p + pad..]);
        if xxh3_64(&b[..p + pad]) != u64::from_le_bytes(sum) {
            return None;
        }
        let mut at = h;
        let mut path = |l: usize| {
            let s = core::str::from_utf8(&b[at..at + l]).ok().map(str::to_owned);
            at += l;
            s
        };
        Some(SwapIntent {
            a_id: id_at(&b[8..32]),
            b_id: id_at(&b[32..56]),
            a_path: path(lens[0])?,
            b_path: path(lens[1])?,
            t_path: path(lens[2])?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::proptest_config;
    use proptest::prelude::*;

    fn id(n: u8) -> FileIdentity {
        FileIdentity {
            volume: 0x1234_5678_9ABC_DEF0,
            file: [n; 16],
        }
    }

    fn sample() -> SwapIntent {
        SwapIntent {
            a_id: id(1),
            b_id: id(2),
            a_path: "D:/repo/.git/moirai".into(),
            b_path: "D:/repo/.git/restore.1".into(),
            t_path: "D:/repo/.git/moirai.swap-old".into(),
        }
    }

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    /// The golden intent: every byte of [OS/fs §4.9.3] for one fixed content, pinned, so that a change to the layout or
    /// to the checksum is a failing test rather than a silent format change. The OS layer and the simulator write
    /// through this codec, so the vector holds for both.
    #[test]
    fn the_golden_intent_is_byte_exact() {
        let b = sample().encode().unwrap();
        // P = 64 + 19 + 22 + 28 = 133, pad = 3, total = 144.
        assert_eq!(b.len(), 144);
        let want_header = concat!(
            "4d535750",                         // magic "MSWP"
            "0100",                             // version 1
            "0000",                             // flags
            "f0debc9a78563412",                 // a_id.volume, little-endian
            "01010101010101010101010101010101", // a_id.file
            "f0debc9a78563412",                 // b_id.volume
            "02020202020202020202020202020202", // b_id.file
            "1300",                             // a_len 19
            "1600",                             // b_len 22
            "1c00",                             // t_len 28
            "0000",                             // reserved
        );
        assert_eq!(hex(&b[..64]), want_header);
        assert_eq!(
            &b[64..133],
            b"D:/repo/.git/moiraiD:/repo/.git/restore.1D:/repo/.git/moirai.swap-old"
        );
        assert_eq!(&b[133..136], &[0, 0, 0]);
        assert_eq!(hex(&b[136..]), GOLDEN_SUM);
        assert_eq!(
            u64::from_le_bytes(b[136..].try_into().unwrap()),
            xxh3_64(&b[..136])
        );
    }

    /// The XXH3-64 of the golden intent's first 136 bytes, little-endian.
    const GOLDEN_SUM: &str = "5327c06623f4dd76";

    #[test]
    fn the_intent_round_trips_and_rejects_every_damage() {
        let i = sample();
        let b = i.encode().unwrap();
        assert_eq!(&b[..4], b"MSWP");
        assert_eq!(b.len() % 8, 0);
        assert_eq!(u16_at(&b, 56), i.a_path.len());
        assert_eq!(&b[8..16], &0x1234_5678_9ABC_DEF0u64.to_le_bytes());
        assert_eq!(SwapIntent::decode(&b), Some(i));
        for at in [0, 4, 6, 8, 40, 56, 62, 64, 133, b.len() - 9, b.len() - 1] {
            let mut d = b.clone();
            d[at] ^= 1;
            assert_eq!(SwapIntent::decode(&d), None, "byte {at}");
        }
        assert_eq!(SwapIntent::decode(&b[..b.len() - 1]), None);
        let mut longer = b.clone();
        longer.extend_from_slice(&[0; 8]);
        assert_eq!(SwapIntent::decode(&longer), None);
        assert_eq!(SwapIntent::decode(&[]), None);
        assert!(SwapIntent::MAX_LEN >= b.len() as u64);
    }

    #[test]
    fn a_path_must_fit() {
        assert!(!SwapIntent::path_fits(""));
        assert!(SwapIntent::path_fits(&"x".repeat(4096)));
        assert!(!SwapIntent::path_fits(&"x".repeat(4097)));
        let mut i = sample();
        i.b_path = String::new();
        assert_eq!(i.encode(), None);
        i.b_path = "x".repeat(4097);
        assert_eq!(i.encode(), None);
        // The longest intent is exactly `MAX_LEN` minus the padding it does not need.
        i.a_path = "a".repeat(4096);
        i.b_path = "b".repeat(4096);
        i.t_path = "t".repeat(4096);
        let b = i.encode().unwrap();
        assert_eq!(b.len() as u64, SwapIntent::MAX_LEN - 7);
        assert_eq!(SwapIntent::decode(&b), Some(i));
    }

    #[test]
    fn the_side_names_follow_the_entry() {
        assert_eq!(
            SwapIntent::side_names("moirai"),
            ("moirai.swap".to_owned(), "moirai.swap-old".to_owned())
        );
    }

    proptest! {
        #![proptest_config(proptest_config())]

        #[test]
        fn intents_round_trip(a in "[A-Z]:/[a-z0-9./ ]{0,60}", b in "[A-Z]:/[a-z0-9]{0,60}", t in "//[a-z]{1,8}/[a-z]{1,8}",
                              va in any::<u64>(), fa in any::<[u8; 16]>(), vb in any::<u64>(), fb in any::<[u8; 16]>()) {
            let i = SwapIntent {
                a_id: FileIdentity { volume: va, file: fa },
                b_id: FileIdentity { volume: vb, file: fb },
                a_path: a, b_path: b, t_path: t,
            };
            let bytes = i.encode().unwrap();
            prop_assert_eq!(bytes.len() % 8, 0);
            prop_assert_eq!(SwapIntent::decode(&bytes), Some(i));
        }

        #[test]
        fn a_flipped_bit_is_always_unreadable(a in "[A-Z]:/[a-z]{1,40}", at in any::<prop::sample::Index>(),
                                              bit in 0u8..8) {
            let i = SwapIntent {
                a_id: id(3),
                b_id: id(4),
                t_path: format!("{a}.swap-old"),
                b_path: format!("{a}-b"),
                a_path: a,
            };
            let mut bytes = i.encode().unwrap();
            let k = at.index(bytes.len());
            bytes[k] ^= 1 << bit;
            prop_assert_eq!(SwapIntent::decode(&bytes), None);
        }
    }
}
