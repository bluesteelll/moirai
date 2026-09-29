//! Offline o200k_base token counts: the o200k half of the tokenizer ledger ([90 §8.3], [90 §9]) for the card
//! gate ([LQ/card §7.2]), measurement 6's conversion table ([90 §9.1], PLAN WP-54) and the static-text budgets
//! gated on the maximum over the Claude and o200k families ([90 §9.1] "How it is used").
//!
//! The vocabulary is the public `o200k_base.tiktoken` that `tiktoken-rs` embeds; its SHA-256 is
//! [`VOCABULARY_SHA256`], and `Cargo.lock`'s checksum of `tiktoken-rs` pins it. Nothing leaves the machine.

use std::fs;
use std::io;
use std::path::Path;

/// SHA-256 of the o200k_base vocabulary file the counts use: `assets/o200k_base.tiktoken` of `tiktoken-rs` 0.12.1,
/// byte-identical to the file OpenAI publishes for `tiktoken`'s `o200k_base` encoding (3,613,922 bytes).
pub const VOCABULARY_SHA256: &str =
    "446a9538cb6c348e3516120d7c08b09f57c36495e2acfffe59a5bf8b0cfb1a2d";

/// The number of o200k_base tokens in `text`.
///
/// Every character is ordinary text: a special-token spelling such as `<|endoftext|>` inside `text` is counted as the
/// characters it is made of, because a harness places text in a context as data, never as a control token. The first
/// call loads the vocabulary once for the life of the process.
#[must_use]
pub fn count(text: &str) -> usize {
    tiktoken_rs::o200k_base_singleton()
        .encode_ordinary(text)
        .len()
}

/// The token ids of `text` under o200k_base, with the same rules as [`count`].
#[must_use]
pub fn encode(text: &str) -> Vec<u32> {
    tiktoken_rs::o200k_base_singleton().encode_ordinary(text)
}

/// The number of o200k_base tokens in the UTF-8 file at `path`.
///
/// # Errors
///
/// The file cannot be read, or it is not valid UTF-8 (`io::ErrorKind::InvalidData`).
pub fn count_file(path: &Path) -> io::Result<usize> {
    let text = fs::read_to_string(path)?;
    Ok(count(&text))
}

#[cfg(test)]
mod tests {
    use super::{count, count_file, encode};

    #[test]
    fn empty_text_has_no_tokens() {
        assert_eq!(count(""), 0);
    }

    #[test]
    fn counts_use_the_o200k_vocabulary() {
        // o200k_base ids; cl100k_base would give [15339, 1917].
        assert_eq!(encode("hello world"), [24912, 2375]);
        assert_eq!(count("hello world"), 2);
        assert_eq!(count("Hello, world!"), 4);
    }

    #[test]
    fn special_token_spellings_are_ordinary_text() {
        assert!(count("<|endoftext|>") > 1);
    }

    /// [90 §6.2] item 2: under a byte-level BPE, N bytes never make more than N tokens; non-empty text makes at least
    /// one. Checked on fixed scripts and on pseudo-random strings mixing ASCII, Cyrillic, CJK, emoji and whitespace.
    #[test]
    fn tokens_never_exceed_bytes() {
        let fixed = [
            "fn main() {\n    println!(\"hi\");\n}\n",
            "Привет, мир! Это проверка счёта токенов.",
            "#12345\n#12346\n#12347\n",
            "日本語のテキスト",
            "\u{1F600}\u{1F680} \t\r\n",
        ];
        for text in fixed {
            let n = count(text);
            assert!(
                n >= 1 && n <= text.len(),
                "{text:?}: {n} tokens for {} bytes",
                text.len()
            );
        }
        let alphabet: Vec<char> = "aZ09 _-#\n\t{}();:.,'\"ёжЯщ日本語\u{1F600}\u{00E9}\u{0301}"
            .chars()
            .collect();
        let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut text = String::new();
        for _ in 0..200 {
            text.clear();
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let len = 1 + usize::try_from(state % 120).unwrap_or(0);
            for _ in 0..len {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                let pick = usize::try_from(state % alphabet.len() as u64).unwrap_or(0);
                text.push(alphabet[pick]);
            }
            let n = count(&text);
            assert!(
                n >= 1 && n <= text.len(),
                "{text:?}: {n} tokens for {} bytes",
                text.len()
            );
        }
    }

    #[test]
    fn counts_a_file_and_refuses_invalid_utf8() {
        let dir =
            std::env::temp_dir().join(format!("moirai-tokcount-o200k-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let good = dir.join("good.txt");
        let bad = dir.join("bad.txt");
        std::fs::write(&good, "hello world").unwrap();
        std::fs::write(&bad, [0x66, 0xFF, 0x6F]).unwrap();
        let counted = count_file(&good);
        let refused = count_file(&bad);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(counted.unwrap(), 2);
        assert_eq!(refused.unwrap_err().kind(), std::io::ErrorKind::InvalidData);
    }
}
