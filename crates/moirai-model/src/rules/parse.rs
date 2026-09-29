//! The hand-written Markdown-table parser of [RULES/README] §2–§5: byte rules, table discovery by marker lines, the
//! table grammar, cell splitting and the typed column values. It knows no table: the registry (§7) supplies every
//! table's columns, and [`super`] checks the registry against the model's compiled-in column lists.
//!
//! Every violation is a [`ParseError`] naming the file, the 1-based line, the table id and the column; the loader turns
//! it into a panic, so a rule file never parses partially ([RULES/README] §8).

use std::fmt;

/// A parse failure: where it is and what is wrong.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    /// The rule file's name (`merge-table.md`).
    pub file: String,
    /// The 1-based line, or 0 for a whole-file fault.
    pub line: usize,
    /// The table id, when the fault is inside a table.
    pub table: Option<String>,
    /// The column name, when the fault is inside a cell.
    pub column: Option<String>,
    /// What is wrong.
    pub what: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.file, self.line)?;
        if let Some(t) = &self.table {
            write!(f, " table {t}")?;
        }
        if let Some(c) = &self.column {
            write!(f, " column {c}")?;
        }
        write!(f, ": {}", self.what)
    }
}

/// The type of a registered column ([RULES/README] §5 `column-types`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ColType {
    /// CT-001: a row id with the table's prefix.
    Id,
    /// CT-002: one printable-ASCII word.
    Token,
    /// CT-003: tokens separated by `, `.
    Tokens,
    /// CT-004: a token from a fixed set.
    Enum(Vec<String>),
    /// CT-005: decimal digits without sign or leading zeros.
    Int,
    /// CT-006: citations separated by `; `.
    Cite,
    /// CT-007: free prose, the final column only.
    Text,
}

impl ColType {
    /// The column-type word of `column-types` (`enum` for every enumeration).
    pub fn word(&self) -> &'static str {
        match self {
            ColType::Id => "id",
            ColType::Token => "token",
            ColType::Tokens => "tokens",
            ColType::Enum(_) => "enum",
            ColType::Int => "int",
            ColType::Cite => "cite",
            ColType::Text => "text",
        }
    }
}

/// The shared `basis` enumeration of [RULES/README] §5.
pub const BASIS: [&str; 5] = ["design", "derived", "proposed", "gap", "withdrawn"];

/// The shared `disposition` enumeration of [RULES/README] §5.
pub const DISPOSITION: [&str; 6] = ["clean", "value", "structural", "hint", "gap", "none"];

/// One column of a table: its name and type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Column {
    /// The column name, as the header row writes it.
    pub name: String,
    /// Its type.
    pub ty: ColType,
}

/// Parses one `name:type` token of a registry `columns` cell ([RULES/README] §7): `basis:enum` and `disposition:enum`
/// without a set take the shared enumerations; `enum(a/b/c)` lists its set.
pub fn parse_column(spec: &str) -> Result<Column, String> {
    let (name, ty) = spec
        .split_once(':')
        .ok_or_else(|| format!("column spec {spec:?} has no ':'"))?;
    let nb = name.as_bytes();
    if nb.is_empty()
        || !nb[0].is_ascii_lowercase()
        || !nb
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'_')
    {
        return Err(format!("column name {name:?} is not a lower-case word"));
    }
    let ty = match ty {
        "id" => ColType::Id,
        "token" => ColType::Token,
        "tokens" => ColType::Tokens,
        "int" => ColType::Int,
        "cite" => ColType::Cite,
        "text" => ColType::Text,
        "enum" => match name {
            "basis" => ColType::Enum(BASIS.iter().map(|s| s.to_string()).collect()),
            "disposition" => ColType::Enum(DISPOSITION.iter().map(|s| s.to_string()).collect()),
            _ => {
                return Err(format!(
                    "column {name}: enum without a set names no shared enumeration"
                ));
            }
        },
        t => {
            let set = t
                .strip_prefix("enum(")
                .and_then(|r| r.strip_suffix(')'))
                .ok_or_else(|| format!("unknown column type {t:?}"))?;
            let values: Vec<String> = set.split('/').map(str::to_string).collect();
            if values.iter().any(|v| v.is_empty() || !is_token(v)) {
                return Err(format!("column {name}: bad enumeration set {set:?}"));
            }
            ColType::Enum(values)
        }
    };
    Ok(Column {
        name: name.to_string(),
        ty,
    })
}

/// One typed cell value ([RULES/README] §4 "Cell values"). A `-` is kept as the token `-`; [`Cell::is_dash`] tests it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cell {
    /// A row id.
    Id(String),
    /// A token, or `-`.
    Token(String),
    /// A token list; `-` is the one-element list `["-"]`.
    Tokens(Vec<String>),
    /// An enumeration value.
    Enum(String),
    /// A non-negative integer.
    Int(u64),
    /// The citations, split at `; `.
    Cite(Vec<String>),
    /// Free prose with `\|` unescaped.
    Text(String),
}

impl Cell {
    /// The single-token value of an id, token, enum or int cell (an int as its digits).
    pub fn tok(&self) -> &str {
        match self {
            Cell::Id(s) | Cell::Token(s) | Cell::Enum(s) | Cell::Text(s) => s,
            Cell::Tokens(v) if v.len() == 1 => &v[0],
            Cell::Int(_) | Cell::Tokens(_) | Cell::Cite(_) => {
                panic!("rule cell {self:?} is not a single token")
            }
        }
    }

    /// The token list of a `tokens` cell (a single-token cell reads as a one-element list).
    pub fn toks(&self) -> Vec<&str> {
        match self {
            Cell::Tokens(v) => v.iter().map(String::as_str).collect(),
            Cell::Cite(v) => v.iter().map(String::as_str).collect(),
            other => vec![other.tok()],
        }
    }

    /// The value of an `int` cell.
    pub fn int(&self) -> u64 {
        match self {
            Cell::Int(n) => *n,
            other => panic!("rule cell {other:?} is not an int"),
        }
    }

    /// Whether the cell is `-` (empty, not applicable).
    pub fn is_dash(&self) -> bool {
        match self {
            Cell::Token(s) | Cell::Enum(s) | Cell::Text(s) => s == "-",
            Cell::Tokens(v) => v.len() == 1 && v[0] == "-",
            Cell::Cite(v) => v.len() == 1 && v[0] == "-",
            Cell::Id(_) | Cell::Int(_) => false,
        }
    }
}

/// One raw table as found in a file: its marker, header and data rows, cells split and unescaped but not yet typed.
#[derive(Clone, Debug)]
pub struct RawTable {
    /// The table id of the marker line.
    pub id: String,
    /// The 1-based line of the marker.
    pub marker_line: usize,
    /// The header cells.
    pub header: Vec<String>,
    /// The data rows: (1-based line, cells).
    pub rows: Vec<(usize, Vec<String>)>,
}

/// What one rule file holds for the parser: its raw tables and the numbers of its open points.
#[derive(Clone, Debug)]
pub struct RawFile {
    /// The raw tables, in file order.
    pub tables: Vec<RawTable>,
    /// The numbers `n` of the items of the section "Open points for the review" (cited as `[OP-n]`).
    pub open_points: Vec<u32>,
}

/// Whether `s` is a CT-002 token: printable ASCII without space or backtick, `|` only escaped (already unescaped here).
pub fn is_token(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| (0x21..=0x7E).contains(&b) && b != b'`')
}

fn is_table_id(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b[0].is_ascii_lowercase()
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

/// Splits one table line into its cells ([RULES/README] §4 "Cell splitting"): at every `|` not preceded by a
/// backslash; the empty strings before the first and after the last `|` dropped; each cell trimmed of spaces; `\|`
/// read as `|`.
pub fn split_cells(line: &str) -> Vec<String> {
    let bytes = line.as_bytes();
    let mut cuts = Vec::new();
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'|' && (i == 0 || bytes[i - 1] != b'\\') {
            cuts.push(i);
        }
    }
    let mut cells = Vec::new();
    for w in cuts.windows(2) {
        let raw = &line[w[0] + 1..w[1]];
        cells.push(raw.trim_matches(' ').replace("\\|", "|"));
    }
    cells
}

fn is_table_line(line: &str) -> bool {
    line.starts_with("| ") && line.ends_with(" |")
}

fn is_separator_cell(c: &str) -> bool {
    let inner = c.strip_prefix(':').unwrap_or(c);
    let inner = inner.strip_suffix(':').unwrap_or(inner);
    inner.len() >= 3 && inner.bytes().all(|b| b == b'-')
}

/// Scans one rule file ([RULES/README] §2, §3, §4): the byte rules, fenced blocks, marker lines, header, separator and
/// data rows, and the open points. Cells are split but not typed.
pub fn scan(file: &str, text: &str) -> Result<RawFile, ParseError> {
    let err = |line: usize, table: Option<&str>, what: String| ParseError {
        file: file.to_string(),
        line,
        table: table.map(str::to_string),
        column: None,
        what,
    };
    if text.as_bytes().starts_with(&[0xEF, 0xBB, 0xBF]) {
        return Err(err(
            1,
            None,
            "the file starts with a byte-order mark".into(),
        ));
    }
    if let Some(pos) = text.bytes().position(|b| b == b'\r') {
        let line = text[..pos].bytes().filter(|&b| b == b'\n').count() + 1;
        return Err(err(
            line,
            None,
            "a carriage return (0x0D); line ends are LF only".into(),
        ));
    }
    let lines: Vec<&str> = text.split('\n').collect();
    let mut tables: Vec<RawTable> = Vec::new();
    let mut open_points = Vec::new();
    let mut in_fence = false;
    let mut in_open_points = false;
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let n = i + 1;
        if line.trim_start_matches(' ').starts_with("```") {
            in_fence = !in_fence;
            i += 1;
            continue;
        }
        if in_fence {
            i += 1;
            continue;
        }
        if let Some(h) = line.strip_prefix("## ") {
            in_open_points = h == "Open points for the review";
        } else if in_open_points
            && let Some(dot) = line.find(". ")
            && dot > 0
            && line[..dot].bytes().all(|b| b.is_ascii_digit())
        {
            open_points.push(
                line[..dot]
                    .parse::<u32>()
                    .map_err(|e| err(n, None, format!("open point number: {e}")))?,
            );
        }
        if let Some(rest) = line.strip_prefix("<!-- table: ") {
            let id = rest
                .strip_suffix(" -->")
                .ok_or_else(|| err(n, None, format!("malformed marker line {line:?}")))?;
            if !is_table_id(id) {
                return Err(err(n, None, format!("bad table id {id:?}")));
            }
            let header_line = lines
                .get(i + 1)
                .copied()
                .ok_or_else(|| err(n, Some(id), "no header row after the marker".into()))?;
            if !is_table_line(header_line) {
                return Err(err(
                    n + 1,
                    Some(id),
                    "the line after the marker is not a header row".into(),
                ));
            }
            let sep_line = lines.get(i + 2).copied().unwrap_or("");
            if !(sep_line.starts_with('|') && sep_line.ends_with('|') && sep_line.len() > 1) {
                return Err(err(n + 2, Some(id), "no separator row".into()));
            }
            let header = split_cells(header_line);
            let sep = split_cells(sep_line);
            if sep.len() != header.len() || !sep.iter().all(|c| is_separator_cell(c)) {
                return Err(err(n + 2, Some(id), "malformed separator row".into()));
            }
            let mut rows = Vec::new();
            let mut j = i + 3;
            while j < lines.len() && lines[j].starts_with('|') {
                let l = lines[j];
                if l.contains('\t') {
                    return Err(err(j + 1, Some(id), "a tab (0x09) in a table line".into()));
                }
                if !is_table_line(l) {
                    return Err(err(
                        j + 1,
                        Some(id),
                        "a table line must start with '| ' and end with ' |'".into(),
                    ));
                }
                let cells = split_cells(l);
                if cells.len() != header.len() {
                    return Err(err(
                        j + 1,
                        Some(id),
                        format!("{} cells; the header has {}", cells.len(), header.len()),
                    ));
                }
                rows.push((j + 1, cells));
                j += 1;
            }
            if header_line.contains('\t') || sep_line.contains('\t') {
                return Err(err(n + 1, Some(id), "a tab (0x09) in a table line".into()));
            }
            tables.push(RawTable {
                id: id.to_string(),
                marker_line: n,
                header,
                rows,
            });
            i = j;
            continue;
        }
        i += 1;
    }
    if in_fence {
        return Err(err(
            lines.len(),
            None,
            "an unterminated fenced block".into(),
        ));
    }
    Ok(RawFile {
        tables,
        open_points,
    })
}

fn strip_backticks(s: &str) -> &str {
    if s.len() >= 2 && s.starts_with('`') && s.ends_with('`') {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

fn typed_ascii(s: &str, allow_list_spaces: bool) -> Result<(), String> {
    for (k, b) in s.bytes().enumerate() {
        if !(0x20..=0x7E).contains(&b) {
            return Err(format!("byte 0x{b:02X} outside printable ASCII"));
        }
        if b == b' ' && !(allow_list_spaces && k > 0 && s.as_bytes()[k - 1] == b',') {
            return Err("a space outside a ', ' list separator".into());
        }
    }
    Ok(())
}

/// Types one cell by its column ([RULES/README] §4 "Cell values", §5). `prefix` is the table's registered row-id
/// prefix, checked for an `id` cell.
pub fn type_cell(raw: &str, col: &Column, prefix: &str) -> Result<Cell, String> {
    if col.ty == ColType::Text {
        if raw.is_empty() {
            return Err("an empty text cell (write '-' for none)".into());
        }
        return Ok(Cell::Text(raw.to_string()));
    }
    if col.ty == ColType::Cite {
        if raw.is_empty() {
            return Err("an empty cite cell".into());
        }
        let parts: Vec<String> = raw.split("; ").map(str::to_string).collect();
        for p in &parts {
            if p != "-" && !p.starts_with('[') {
                return Err(format!(
                    "citation {p:?} does not start with a bracketed reference"
                ));
            }
        }
        return Ok(Cell::Cite(parts));
    }
    let v = strip_backticks(raw);
    if v.is_empty() {
        return Err("an empty typed cell".into());
    }
    typed_ascii(v, col.ty == ColType::Tokens)?;
    match &col.ty {
        ColType::Id => {
            let b = v.as_bytes();
            let ok = b.len() == 6
                && b[0].is_ascii_uppercase()
                && b[1].is_ascii_uppercase()
                && b[2] == b'-'
                && b[3..].iter().all(u8::is_ascii_digit);
            if !ok {
                return Err(format!(
                    "row id {v:?} does not match ^[A-Z]{{2}}-[0-9]{{3}}$"
                ));
            }
            if &v[..2] != prefix {
                return Err(format!(
                    "row id {v:?} does not carry the table's prefix {prefix}"
                ));
            }
            Ok(Cell::Id(v.to_string()))
        }
        ColType::Token => {
            if !is_token(v) {
                return Err(format!("{v:?} is not a token"));
            }
            Ok(Cell::Token(v.to_string()))
        }
        ColType::Tokens => {
            let items: Vec<String> = v.split(", ").map(str::to_string).collect();
            for t in &items {
                if !is_token(t) {
                    return Err(format!("{t:?} in a token list is not a token"));
                }
            }
            Ok(Cell::Tokens(items))
        }
        ColType::Enum(set) => {
            if !set.iter().any(|s| s == v) {
                return Err(format!("{v:?} is not one of {}", set.join("/")));
            }
            Ok(Cell::Enum(v.to_string()))
        }
        ColType::Int => {
            let ok = v.bytes().all(|b| b.is_ascii_digit()) && (v == "0" || !v.starts_with('0'));
            if !ok {
                return Err(format!(
                    "{v:?} is not a decimal integer without sign or leading zeros"
                ));
            }
            v.parse::<u64>().map(Cell::Int).map_err(|e| e.to_string())
        }
        ColType::Cite | ColType::Text => unreachable!("handled above"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cells_split_at_unescaped_pipes() {
        assert_eq!(
            split_cells("| a | `b\\|c` | d e |"),
            vec!["a".to_string(), "`b|c`".into(), "d e".into()]
        );
        assert_eq!(split_cells("| x |"), vec!["x".to_string()]);
    }

    #[test]
    fn columns_parse_with_shared_and_listed_enums() {
        let c = parse_column("basis:enum").unwrap();
        assert_eq!(
            c.ty,
            ColType::Enum(BASIS.iter().map(|s| s.to_string()).collect())
        );
        let c = parse_column("end:enum(dst-deleted/src-deleted)").unwrap();
        assert_eq!(
            c.ty,
            ColType::Enum(vec!["dst-deleted".into(), "src-deleted".into()])
        );
        assert!(parse_column("x:enum").is_err());
        assert!(parse_column("x:float").is_err());
    }

    #[test]
    fn typed_cells_follow_the_column_types() {
        let id = Column {
            name: "row".into(),
            ty: ColType::Id,
        };
        assert_eq!(
            type_cell("MR-012", &id, "MR"),
            Ok(Cell::Id("MR-012".into()))
        );
        assert!(type_cell("MR-012", &id, "SL").is_err());
        assert!(type_cell("MR-12", &id, "MR").is_err());
        let tok = Column {
            name: "x".into(),
            ty: ColType::Token,
        };
        assert_eq!(
            type_cell("`in_progress`", &tok, "XX"),
            Ok(Cell::Token("in_progress".into()))
        );
        assert!(type_cell("a b", &tok, "XX").is_err());
        assert!(type_cell("", &tok, "XX").is_err());
        let toks = Column {
            name: "x".into(),
            ty: ColType::Tokens,
        };
        assert_eq!(
            type_cell("a, b,c", &toks, "XX"),
            Ok(Cell::Tokens(vec!["a".into(), "b,c".into()]))
        );
        assert!(type_cell("a ,b", &toks, "XX").is_err());
        let int = Column {
            name: "x".into(),
            ty: ColType::Int,
        };
        assert_eq!(type_cell("0", &int, "XX"), Ok(Cell::Int(0)));
        assert!(type_cell("012", &int, "XX").is_err());
        assert!(type_cell("-1", &int, "XX").is_err());
    }

    #[test]
    fn the_scanner_refuses_bad_bytes_and_skips_fences() {
        assert!(scan("f.md", "a\r\nb").is_err());
        assert!(scan("f.md", "\u{feff}a").is_err());
        let text = "```\n<!-- table: x -->\n| row | note |\n|---|---|\n```\n<!-- table: y -->\n| row | note |\n|---|---|\n| YY-001 | - |\n\ntext\n## Open points for the review\n\n1. **One.** a\n2. b\n";
        let f = scan("f.md", text).unwrap();
        assert_eq!(f.tables.len(), 1);
        assert_eq!(f.tables[0].id, "y");
        assert_eq!(f.tables[0].rows.len(), 1);
        assert_eq!(f.open_points, vec![1, 2]);
        assert!(scan("f.md", "<!-- table: z -->\n\n| row | note |\n|---|---|\n").is_err());
        assert!(
            scan(
                "f.md",
                "<!-- table: z -->\n| row | note |\n|---|---|\n| ZZ-001 |\n"
            )
            .is_err()
        );
    }
}
