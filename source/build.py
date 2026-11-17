#!/usr/bin/env python3
"""Build the Moirai book.

Reads the Markdown chapters in source/chapters/ (English) and source/chapters-ru/ (Russian) and writes complete
static pages: the English edition at the branch root, the Russian edition under ru/. The Markdown parser is
markdown-it-py with mdit-py-plugins (source/requirements.txt); everything else is the standard library.

Usage: python source/build.py [--check]
  --check  build nothing; only run the checks (links, forbidden content, chapter format).
Exit status: 0 when every check passes, 1 when any error is found.

A chapter is CommonMark with GitHub tables, after a front matter block:

  ---
  title: The page title (the browser tab)
  nav: The short name in the contents
  description: One sentence for search engines and link previews
  ---

and these book conventions (README.md "Writing a chapter" has examples):
  - `# Title` once, at the top; the paragraph right after it is the lead.
  - `## Heading {#id}` or `### Heading {#id}` sets the anchor id; without `{#id}` one is made from the text.
  - A paragraph that is only `*italic text*`, right after a code block, is the block's caption.
  - `> [!NOTE]`, `> [!TIP]` or `> [!WARNING]` (also IMPORTANT and CAUTION) as the first line of a quote makes a
    callout. Text after the marker on that line is its title; otherwise a next line that is all `**bold**` is.
  - `Term {#id}` followed by `: definition` is a glossary entry (a definition list); entries are separated by an
    empty line.
  - The raw HTML blocks the book uses pass through: <details>/<summary>, the diagrams
    (<figure class="diagram"><svg>...) and the cards (<div class="cards">). A block ends at its first empty line.
    Any other HTML is an error (BOOK_HTML_RE), so that a placeholder such as <store> written outside backticks, or a
    diagram split by an empty line, cannot vanish from the page unnoticed. HTML comments are allowed.
Errors name the .md file and, for most problems, the line; `--check` under GitHub Actions also prints them as
annotations.
"""

import html
import json
import os
import re
import sys
import urllib.parse
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
REPO = "https://github.com/bluesteelll/moirai"

ORDER = ["index", "why", "concepts", "tour", "versions", "git-image", "file-links", "lachesis", "agents", "engine",
         "building", "faq", "glossary"]
AFTER_SEP = ["faq", "glossary"]
NUMBERED = ORDER[1:ORDER.index(AFTER_SEP[0])]   # every chapter between the home page and the reference pages

STRINGS = {
    "en": {
        "src": ROOT / "source" / "chapters",
        "out": ROOT,
        "up": "",
        "site": "Moirai book",
        "home_title": "Moirai — the book",
        "book": "book",
        "banner": "<strong>Preview.</strong> Moirai is in development, and its behaviour may change.",
        "toc": "On this page",
        "prev": "Previous",
        "next": "Next",
        "menu": "Contents",
        "skip": "Skip to content",
        "chapters": "Chapters",
        "chapter": "Chapter",
        "reference": "Reference",
        "theme": "Switch between light and dark theme",
        "footer": "Moirai is open source under the Apache License 2.0. Source and specification: "
                  f"<a href=\"{REPO}\">github.com/bluesteelll/moirai</a>. This book is a preview of the design; where it "
                  "and the specification in the repository differ, the specification is authoritative.",
    },
    "ru": {
        "src": ROOT / "source" / "chapters-ru",
        "out": ROOT / "ru",
        "up": "../",
        "site": "Книга о Moirai",
        "home_title": "Moirai — книга",
        "book": "книга",
        "banner": "<strong>Предварительная версия.</strong> Moirai в разработке, её поведение может измениться.",
        "toc": "На этой странице",
        "prev": "Назад",
        "next": "Далее",
        "menu": "Содержание",
        "skip": "К содержимому",
        "chapters": "Главы",
        "chapter": "Глава",
        "reference": "Справочник",
        "theme": "Переключить светлую или тёмную тему",
        "footer": "Moirai — открытый проект под лицензией Apache 2.0. Исходный код и спецификация: "
                  f"<a href=\"{REPO}\">github.com/bluesteelll/moirai</a>. Книга — предварительное описание замысла; "
                  "если она расходится со спецификацией в репозитории, верна спецификация.",
    },
}

LOGO = ('<svg viewBox="0 0 32 32" aria-hidden="true"><rect width="32" height="32" rx="9" fill="#2a2b2f"/>'
        '<path d="M5 11c6-5 12 5 22 0" fill="none" stroke="#d6a55e" stroke-width="2.3" stroke-linecap="round"/>'
        '<path d="M5 16.5c6-5 12 5 22 0" fill="none" stroke="#e9e7e2" stroke-opacity="0.55" stroke-width="2.3" '
        'stroke-linecap="round"/>'
        '<path d="M5 22c6-5 9 3 14 1.6" fill="none" stroke="#e9e7e2" stroke-opacity="0.55" stroke-width="2.3" '
        'stroke-linecap="round"/></svg>')

FONTS = ('<link rel="preconnect" href="https://fonts.googleapis.com">'
         '<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>'
         '<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Golos+Text:wght@400..700'
         '&amp;family=JetBrains+Mono:wght@400..600&amp;display=swap">')

THEME_HEAD = ("<script>(function(){try{var t=localStorage.getItem('moirai-book-theme');"
              "if(t==='light'||t==='dark')document.documentElement.setAttribute('data-theme',t);}catch(e){}})();</script>")

THEME_TAIL = ("<script>(function(){var b=document.querySelector('.theme-toggle');if(!b)return;"
              "b.addEventListener('click',function(){var r=document.documentElement;"
              "var cur=r.getAttribute('data-theme')||(window.matchMedia&&matchMedia('(prefers-color-scheme: dark)')"
              ".matches?'dark':'light');var nx=cur==='dark'?'light':'dark';r.setAttribute('data-theme',nx);"
              "try{localStorage.setItem('moirai-book-theme',nx);}catch(e){}});})();</script>")

TOGGLE_ICONS = ('<svg class="sun" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" '
                'stroke-linecap="round" aria-hidden="true"><circle cx="12" cy="12" r="4.2"/><path d="M12 2.5v2.2M12 '
                '19.3v2.2M4.6 4.6l1.6 1.6M17.8 17.8l1.6 1.6M2.5 12h2.2M19.3 12h2.2M4.6 19.4l1.6-1.6M17.8 6.2l1.6-1.6"/>'
                '</svg><svg class="moon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" '
                'stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M20.5 14.2A8.5 8.5 0 0 1 '
                '9.8 3.5a8.5 8.5 0 1 0 10.7 10.7z"/></svg>')

LANG_LABEL = {"console": "terminal", "lq": "Lachesis", "moi": ".moi", "text": ""}

# Forbidden content: (pattern, message). Errors stop the build.
# Text patterns are searched in the chapter's own text (front matter and code included) and reported with its line.
FORBIDDEN_TEXT = [
    (r"Boyko", "names the author or the author's other project"),
    (r"<ORCH>|D:/orch|orch/moirai", "names the private orchestration state"),
    (r"\bR-\d{2,3}\b", "cites an internal ruling id"),
    (r"(?<![A-Za-z])[A-Za-z]:[\\/](?:Users|claude|moirai|orch|tmp)", "contains a local path"),
    (r"\bprivate/", "points at the private directory"),
    (r"Moirai-Commit|Moirai-Kind|Moirai-Hlc|refs/moirai/|\.moirai-image", "uses a pre-Klotho image name"),
]
# Markup patterns are searched in the rendered page, where the text of code is escaped, so `<link>` in code is fine.
FORBIDDEN_MARKUP = [
    (r"<(?:script|img|image|iframe|link|style|video|audio|object|embed|picture|source|foreignObject)\b",
     "contains a forbidden element (images, media, scripts and styles are refused)"),
    (r"<[A-Za-z][^>]*\son[a-z]+\s*=", "contains an event handler"),
    (r"<[A-Za-z][^>]*\sstyle\s*=", "contains an inline style"),
    (r"<svg[^>]*\s(?:width|height)\s*=", "gives an svg a fixed size"),
    (r"<[A-Za-z][^>]*\s(?:fill|stroke|font-size|font-family|stroke-width)\s*=", "sets a presentation attribute in an svg"),
]

FRONT_RE = re.compile(r"\A---[ \t]*\n(.*?)\n---[ \t]*(?:\n|\Z)", re.S)
META_KEYS = ("title", "nav", "description")
ID_RE = re.compile(r'\sid="([^"]+)"')
HREF_RE = re.compile(r'\shref="([^"]+)"')
HEADING_RE = re.compile(r'<(h[23]) id="([^"]+)">(.*?)</\1>', re.S)
TAG_RE = re.compile(r"<[^>]+>")
EXPLICIT_ID_RE = re.compile(r"\s*(?<!\\)\{#([^\W_][\w-]*)\}\s*$")   # a letter or digit (any script), then \w and -
TEMPLATE_IDS = {"main"}   # ids the page template itself uses
STRAY_ID_RE = re.compile(r"\{#[^}\n]*\}")
ALERT_RE = re.compile(r"^\[!(NOTE|TIP|IMPORTANT|WARNING|CAUTION)\](?:[ \t]*(.*))?$", re.I)
ALERT_KIND = {"note": "note", "important": "note", "tip": "tip", "warning": "warn", "caution": "warn"}
COLON_MARKER_RE = re.compile(r"^[ \t]*(?:(?:[-*+]|\d{1,9}[.)])[ \t]+)*(?:>[ \t]*)+\[!(?:NOTE|TIP|IMPORTANT|WARNING|CAUTION)\]:",
                             re.I | re.M)
BREAKS = ("softbreak", "hardbreak")
# The raw HTML blocks the book uses. Any other HTML in a chapter is a mistake: most often a placeholder such as
# <store> written outside backticks, which a browser would hide, or a diagram split by an empty line.
BOOK_HTML_RE = re.compile(r"\s*(?:<!--|</?details\b|</?summary\b|<figure class=\"diagram\">|<div class=\"cards\">)",
                          re.I)
HTML_PROBLEM = ("is raw HTML, which the page would hide or break. A placeholder goes in backticks (`<store>`); if "
                "it belongs to a diagram or the cards, an empty line cut it off: remove the empty lines inside that "
                "block")
SOURCES = {}    # path -> the chapter's text, for the checks that report lines of the .md file
PROBLEMS = {}   # path -> [(line, message)] found while rendering
FAILED = {}     # lang -> slugs whose chapter could not be read


class ChapterError(ValueError):
    pass


def rel(path):
    try:
        return path.resolve().relative_to(ROOT).as_posix()
    except ValueError:
        return str(path)


def read_front_matter(path, text):
    m = FRONT_RE.match(text)
    if not m:
        raise ChapterError(f"{rel(path)}:1: the file must start with a front matter block: ---, then title, nav and "
                           f"description lines, then ---")
    meta = {}
    for n, line in enumerate(m.group(1).split("\n"), 2):
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        key, sep, value = line.partition(":")
        key, value = key.strip(), value.strip()
        if not sep or key not in META_KEYS:
            raise ChapterError(f"{rel(path)}:{n}: expected one of 'title: ...', 'nav: ...' or 'description: ...'")
        if value[:1] == '"':
            try:
                value = json.loads(value)
                value.encode("utf-8")
            except UnicodeEncodeError:
                raise ChapterError(f"{rel(path)}:{n}: an escape such as \\ud800 is not a character") from None
            except ValueError:
                if len(value) > 1 and value.endswith('"'):
                    msg = 'a quoted value has a bad escape: inside quotes, write a backslash as \\\\ and a quote as \\"'
                else:
                    msg = 'a value that starts with " must be one quoted string, "like this"; quote the whole value'
                raise ChapterError(f"{rel(path)}:{n}: {msg}") from None
        elif value[:1] == "'" and value[-1:] == "'" and len(value) > 1:
            value = value[1:-1].replace("''", "'")
        meta[key] = value
    for key in META_KEYS:
        if not meta.get(key):
            raise ChapterError(f"{rel(path)}: the front matter has no {key}")
    return meta, text[m.end():], text[:m.end()].count("\n")


def slugify(text, taken):
    base = re.sub(r"[^\w]+", "-", text.lower(), flags=re.U).strip("-_") or "section"
    slug, n = base, 2
    while slug in taken:
        slug, n = f"{base}-{n}", n + 1
    return slug


def _inline_text(tok):
    return "".join(c.content for c in tok.children or [] if c.type in ("text", "code_inline"))


def _strip_explicit_id(inline):
    """Remove a trailing {#id} from an inline token; return the id or None."""
    m = EXPLICIT_ID_RE.search(inline.content)
    if not m:
        return None
    inline.content = inline.content[:m.start()]
    last = inline.children[-1] if inline.children else None
    if last is not None and last.type == "text":
        lm = re.search(r"\s*\{#[\w-]*\}\s*$", last.content)
        if lm:
            last.content = last.content[:lm.start()]
    return m.group(1)


def _whole(kids, opener, closer):
    """True when the inline children are exactly one opener...closer span (one *em* or one **strong**)."""
    if not kids or kids[0].type != opener or kids[-1].type != closer:
        return False
    depth = 0
    for k, c in enumerate(kids):
        depth += {opener: 1, closer: -1}.get(c.type, 0)
        if depth == 0 and k != len(kids) - 1:
            return False
    return True


def _trim(kids):
    """Drop line breaks and empty text at both ends of a run of inline children."""
    while kids and (kids[0].type in BREAKS or (kids[0].type == "text" and not kids[0].content.strip())):
        kids = kids[1:]
    while kids and (kids[-1].type in BREAKS or (kids[-1].type == "text" and not kids[-1].content.strip())):
        kids = kids[:-1]
    return kids


def _split_line(kids):
    """Split inline children at the first line break outside any span: (the first line, the rest)."""
    depth = 0
    for k, c in enumerate(kids):
        if c.type.endswith("_open"):
            depth += 1
        elif c.type.endswith("_close"):
            depth -= 1
        elif c.type in BREAKS and depth == 0:
            return kids[:k], kids[k + 1:]
    return kids, []


def _line(state, tok):
    """The .md line where a block token starts."""
    return state.env.get("line0", 0) + (tok.map[0] + 1 if tok.map else 0)


def _child_line(state, inline, needle, nth=0):
    """The .md line of the nth occurrence of needle in an inline token's own source text."""
    pos = -1
    for _ in range(nth + 1):
        pos = inline.content.find(needle, pos + 1)
        if pos < 0:
            return _line(state, inline)
    return _line(state, inline) + inline.content.count("\n", 0, pos)


def _problem(state, line, message):
    state.env.setdefault("problems", []).append((line, message))


SUMMARY_TAGS = {"summary", "code", "em", "strong", "b", "i", "kbd", "br"}


def _check_html(state):
    for t in state.tokens:
        if t.type == "html_block" and not BOOK_HTML_RE.match(t.content):
            _problem(state, _line(state, t), f"{t.content.strip().splitlines()[0][:60]} {HTML_PROBLEM}")
        if t.type == "html_block" and re.match(r"\s*</?(?:details|summary)\b", t.content):
            # A folding block: <details> and its <summary> line are HTML, and the Markdown comes after an empty line.
            for k, line in enumerate(t.content.rstrip("\n").split("\n")):
                where = _line(state, t) + k
                if not re.fullmatch(r"\s*(?:<details\b[^>]*>|</details>|<summary>.*</summary>)\s*", line):
                    _problem(state, where, f"{line.strip()[:60]}: a <details> block takes only its <summary> line "
                                           f"before an empty line; put an empty line after the <summary> line")
                elif "`" in line:
                    _problem(state, where, "the <summary> line is HTML: write code there as <code>...</code>, "
                                           "not in backticks")
                else:
                    for tag in re.findall(r"</?([A-Za-z][A-Za-z0-9-]*)", line):
                        if tag.lower() not in SUMMARY_TAGS | {"details"}:
                            _problem(state, where, f"<{tag}> in the <summary> line is not shown: the line is HTML, "
                                                   f"so write a placeholder as &lt;{tag}&gt; or in <code>")
        if t.type == "inline":
            seen = {}
            for c in t.children or []:
                if c.type == "html_inline":
                    nth = seen[c.content] = seen.get(c.content, -1) + 1
                    if not c.content.startswith("<!--"):
                        _problem(state, _child_line(state, t, c.content, nth), f"{c.content[:60]} {HTML_PROBLEM}")


def _check_stray_ids(state):
    """A {#...} left in the text: a malformed id, a glossary entry run into the previous one, or a literal; and a
    comment that the parser did not take as one."""
    for t in state.tokens:
        if t.type != "inline":
            continue
        seen = {}
        for c in t.children or []:
            if c.type != "text":
                continue
            for m in re.finditer(r"<!--|" + STRAY_ID_RE.pattern, c.content):
                needle = m.group(0)
                nth = seen[needle] = seen.get(needle, -1) + 1
                line = _child_line(state, t, needle, nth)
                if needle == "<!--":
                    _problem(state, line, "this comment would be shown on the page: start <!-- at the beginning of "
                                          "its own line and close it with --> before the next empty line")
                else:
                    _problem(state, line, f"{needle} is left in the text. An id goes at the end of a heading or a "
                                          f"glossary term and is a letter or digit, then letters, digits, - and _; a "
                                          f"glossary entry needs an empty line before it; a literal {{#...}} goes in "
                                          f"backticks")


def _book_rules(state):
    """The book's conventions, applied to the token stream (see the module docstring)."""
    toks = state.tokens
    _check_html(state)

    # Anchor ids: {#id} at the end of a heading or a glossary term; a heading without one gets one made from its text.
    taken = set(TEMPLATE_IDS)
    for i, t in enumerate(toks):
        if t.type in ("heading_open", "dt_open") and i + 1 < len(toks) and toks[i + 1].type == "inline":
            hid = _strip_explicit_id(toks[i + 1])
            if hid:
                t.attrSet("id", hid)
                taken.add(hid)
    for i, t in enumerate(toks):
        if t.type == "heading_open" and t.tag in ("h2", "h3") and not t.attrGet("id"):
            hid = slugify(_inline_text(toks[i + 1]), taken)
            t.attrSet("id", hid)
            taken.add(hid)
    _check_stray_ids(state)

    for i, t in enumerate(toks):
        nxt = toks[i + 1] if i + 1 < len(toks) else None
        # The lead: the paragraph right after the h1.
        if t.type == "heading_close" and t.tag == "h1" and nxt is not None and nxt.type == "paragraph_open":
            nxt.attrJoin("class", "lead")
        # A caption: a paragraph that is one *emphasis*, right after a code block.
        if (t.type in ("fence", "code_block") and nxt is not None and nxt.type == "paragraph_open"
                and _whole(toks[i + 2].children or [], "em_open", "em_close")):
            nxt.attrJoin("class", "caption")
            toks[i + 2].children = toks[i + 2].children[1:-1]
            # In a tight list the parser hides paragraph tags; a caption keeps its own.
            nxt.hidden = toks[i + 3].hidden = False

    # Callouts: a block quote whose first line is [!NOTE], [!TIP], [!WARNING], [!IMPORTANT] or [!CAUTION]. The
    # title is the rest of that line, or else a next line that is all **bold**; the rest is the body.
    i = 0
    while i < len(toks):
        t = toks[i]
        if (t.type == "blockquote_open" and i + 2 < len(toks) and toks[i + 1].type == "paragraph_open"
                and toks[i + 2].type == "inline" and toks[i + 2].children):
            p_open, inline, p_close = toks[i + 1], toks[i + 2], toks[i + 3]
            first = inline.children[0]
            m = ALERT_RE.match(first.content) if first.type == "text" else None
            if m:
                close, depth = i + 1, 1
                while True:
                    depth += {"blockquote_open": 1, "blockquote_close": -1}.get(toks[close].type, 0)
                    if not depth:
                        break
                    close += 1
                t.tag = toks[close].tag = "div"
                t.attrSet("class", f"callout {ALERT_KIND[m.group(1).lower()]}")
                first_line, body = _split_line(inline.children)
                first.content = (m.group(2) or "").lstrip()
                title = _trim([first] + first_line[1:])
                if not title:
                    second_line, after = _split_line(_trim(body))
                    second_line = _trim(second_line)
                    if _whole(second_line, "strong_open", "strong_close"):
                        title, body = second_line[1:-1], after
                body = _trim(body)
                if title:
                    p_open.attrSet("class", "callout-title")
                    inline.children = title
                    if body:
                        b_open, b_inline, b_close = p_open.copy(), inline.copy(), p_close.copy()
                        b_open.attrs = {}
                        b_inline.children = body
                        toks[i + 4:i + 4] = [b_open, b_inline, b_close]
                elif body:
                    inline.children = body
                else:
                    del toks[i + 1:i + 4]
        i += 1


_MD = None


def markdown():
    global _MD
    if _MD is None:
        try:
            from markdown_it import MarkdownIt
            from mdit_py_plugins.deflist import deflist_plugin
        except ImportError:
            sys.exit("error: the Markdown parser is not installed. Install it once with:\n"
                     "  python -m pip install --require-hashes -r source/requirements.txt")
        md = MarkdownIt("commonmark", {"html": True, "langPrefix": "lang-"}).enable("table").use(deflist_plugin)
        md.core.ruler.push("book", _book_rules)
        for rule in ("fence", "code_block"):
            default = md.renderer.rules[rule]

            def block(self, tokens, idx, options, env, _default=default):
                # The parser ends every code block with a newline; the pages never had one.
                return _default(tokens, idx, options, env).replace("\n</code></pre>", "</code></pre>")
            md.add_render_rule(rule, block)
        _MD = md
    return _MD


def _mask_fences(text):
    """Blank the lines inside fenced code blocks, keeping the line count."""
    out, fence = [], None
    for line in text.split("\n"):
        m = re.match(r"^[ \t>]*(?:(?:[-*+]|\d{1,9}[.)])[ \t]+)?[ \t]*(`{3,}|~{3,})", line)
        if fence is None:
            if m:
                fence = m.group(1)
                out.append("")
            else:
                out.append(line)
        else:
            if m and m.group(1)[0] == fence[0] and len(m.group(1)) >= len(fence):
                fence = None
            out.append("")
    return "\n".join(out)


def read_fragment(path):
    data = path.read_bytes()
    if data.startswith(b"\xef\xbb\xbf"):
        data = data[3:]
    try:
        text = data.decode("utf-8").replace("\r\n", "\n").replace("\r", "\n")
    except UnicodeDecodeError as e:
        line = data[:e.start].count(b"\n") + 1
        raise ChapterError(f"{rel(path)}:{line}: the file is not UTF-8 text; save it as UTF-8") from None
    SOURCES[path] = text
    meta, source, line0 = read_front_matter(path, text)
    problems = []
    for m in COLON_MARKER_RE.finditer(_mask_fences(source)):
        problems.append((line0 + source.count("\n", 0, m.start()) + 1,
                         "write a callout marker without a colon, as > [!NOTE]; with the colon it vanishes"))
    env = {"line0": line0, "problems": problems}
    body = markdown().render(source, env)
    PROBLEMS[path] = env["problems"]
    return meta, body.strip() + "\n"


def strip_tags(s):
    return html.unescape(TAG_RE.sub("", s)).strip()


def decorate(body):
    # Wrap tables so they scroll on narrow screens.
    body = re.sub(r"<table\b", '<div class="table-wrap"><table', body)
    body = body.replace("</table>", "</table></div>")

    # Heading anchors.
    def anchor(m):
        tag, hid, inner = m.group(1), m.group(2), m.group(3)
        return f'<{tag} id="{hid}">{inner}<a class="anchor" href="#{hid}" aria-hidden="true" tabindex="-1">#</a></{tag}>'
    body = HEADING_RE.sub(anchor, body)

    # Code blocks: a language label and highlighted prompt lines in console sessions.
    def code(m):
        lang, inner = m.group(1), m.group(2)
        label = LANG_LABEL.get(lang, "")
        if lang == "console":
            lines = inner.split("\n")
            out = []
            for line in lines:
                if line.startswith("$ "):
                    out.append(f'<span class="cmd"><span class="prompt">$ </span>{line[2:]}</span>')
                else:
                    out.append(line)
            inner = "\n".join(out)
        attr = f' data-lang="{label}"' if label else ""
        return f'<pre{attr}><code class="lang-{lang}">{inner}</code></pre>'
    body = re.sub(r'<pre><code class="lang-([a-z]+)">(.*?)</code></pre>', code, body, flags=re.S)
    return body


def nav_list(pages, current, lang):
    items = []
    for slug in ORDER:
        if slug not in pages:
            continue
        if slug == AFTER_SEP[0]:
            items.append('<li class="nav-sep" role="separator"></li>')
        num = str(NUMBERED.index(slug) + 1) if slug in NUMBERED else ""
        cur = ' aria-current="page"' if slug == current else ""
        nav = html.escape(pages[slug][0]["nav"])
        items.append(f'<li><a href="{slug}.html"{cur}><span class="nav-num">{num}</span><span>{nav}</span></a></li>')
    return '<ul class="nav-list">' + "".join(items) + "</ul>"


def render(lang, slug, pages, other_exists):
    s = STRINGS[lang]
    meta, body = pages[slug]
    up = s["up"]
    present = [x for x in ORDER if x in pages]
    i = present.index(slug)
    prev_slug = present[i - 1] if i > 0 else None
    next_slug = present[i + 1] if i + 1 < len(present) else None

    toc_items = [(hid, strip_tags(inner)) for tag, hid, inner in HEADING_RE.findall(body) if tag == "h2"]
    toc = ""
    if toc_items:
        toc = (f'<aside class="toc" aria-label="{s["toc"]}"><p class="toc-title">{s["toc"]}</p><ul>'
               + "".join(f'<li><a href="#{hid}">{html.escape(t)}</a></li>' for hid, t in toc_items)
               + "</ul></aside>")

    pager = ['<nav class="pager">']
    if prev_slug:
        pager.append(f'<a class="prev" href="{prev_slug}.html"><span class="dir">← {s["prev"]}</span>'
                     f'<span class="ttl">{html.escape(pages[prev_slug][0]["nav"])}</span></a>')
    if next_slug:
        pager.append(f'<a class="next" href="{next_slug}.html"><span class="dir">{s["next"]} →</span>'
                     f'<span class="ttl">{html.escape(pages[next_slug][0]["nav"])}</span></a>')
    pager.append("</nav>")

    if lang == "en":
        en_href, ru_href = f"{slug}.html", f"ru/{slug}.html"
        switch = '<span aria-current="true">EN</span>'
        switch += f'<a href="{ru_href}" hreflang="ru" lang="ru">RU</a>' if other_exists else ""
    else:
        en_href, ru_href = f"../{slug}.html", f"{slug}.html"
        switch = f'<a href="{en_href}" hreflang="en" lang="en">EN</a><span aria-current="true">RU</span>'

    title = s["home_title"] if slug == "index" else f'{meta["title"]} · {s["site"]}'
    alternates = ""
    if other_exists:
        base = "https://bluesteelll.github.io/moirai/"
        alternates = (f'<link rel="alternate" hreflang="en" href="{base}{slug}.html">'
                      f'<link rel="alternate" hreflang="ru" href="{base}ru/{slug}.html">')

    nav = nav_list(pages, slug, lang)
    content = decorate(body)
    if slug != "index":
        label = (f'{s["chapter"]} {NUMBERED.index(slug) + 1}' if slug in NUMBERED else s["reference"])
        content = content.replace("<h1", f'<p class="eyebrow">{label}</p><h1', 1)
    body_class = ' class="home"' if slug == "index" else ""
    return f"""<!doctype html>
<html lang="{lang}">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{html.escape(title)}</title>
<meta name="description" content="{html.escape(meta["description"])}">
<meta name="color-scheme" content="light dark">
<meta property="og:title" content="{html.escape(meta["title"])}">
<meta property="og:description" content="{html.escape(meta["description"])}">
<meta property="og:type" content="article">
<link rel="icon" href="{up}assets/favicon.svg" type="image/svg+xml">
{THEME_HEAD}
{FONTS}
<link rel="stylesheet" href="{up}assets/style.css">
{alternates}
</head>
<body{body_class}>
<a class="skip-link" href="#main">{s["skip"]}</a>
<header class="topbar">
<a class="brand" href="index.html">{LOGO}<span class="brand-name">Moirai</span><span class="brand-sub">{s["book"]}</span></a>
<span class="spacer"></span>
<nav class="topbar-links" aria-label="Site">
<a class="hide-narrow" href="{REPO}">GitHub</a>
<span class="lang-switch">{switch}</span>
<button class="theme-toggle" type="button" aria-label="{s["theme"]}" title="{s["theme"]}">{TOGGLE_ICONS}</button>
</nav>
</header>
<div class="layout">
<aside class="sidebar"><p class="sidebar-title">{s["menu"]}</p><nav aria-label="{s["chapters"]}">{nav}</nav></aside>
<main class="content" id="main">
<details class="mobile-nav"><summary>{s["menu"]}</summary><nav aria-label="{s["chapters"]}">{nav}</nav></details>
<article>
<div class="preview-banner" role="note"><span class="dot" aria-hidden="true"></span><div>{s["banner"]}</div></div>
{content}
{"".join(pager)}
<footer class="site-footer"><p>{s["footer"]}</p></footer>
</article>
</main>
{toc}
</div>
{THEME_TAIL}
</body>
</html>
"""


def check(lang, pages, errors, warnings):
    ids = {slug: set(ID_RE.findall(body)) for slug, (meta, body) in pages.items()}
    failed = FAILED.get(lang, set())
    for slug, (meta, body) in pages.items():
        path = STRINGS[lang]["src"] / f"{slug}.md"
        where = rel(path)
        source = SOURCES.get(path, "")
        prose = _mask_fences(source)   # the same lines, with code blocks blanked: where a mistake can be

        def at(text, last=False):
            """':line' of the first (or last) place the text occurs outside code blocks, or '' when it does not."""
            k = prose.rfind(text) if last else prose.find(text)
            return f":{prose.count(chr(10), 0, k) + 1}" if text and k >= 0 else ""

        for line, msg in PROBLEMS.get(path, []):
            errors.append(f"{where}:{line}: {msg}")
        for pat, msg in FORBIDDEN_TEXT:
            for m in re.finditer(pat, source):
                errors.append(f"{where}:{source.count(chr(10), 0, m.start()) + 1}: {msg}: "
                              f"{source[m.start():m.end() + 30]!r}")
        for pat, msg in FORBIDDEN_MARKUP:
            for m in re.finditer(pat, body, re.I):
                errors.append(f"{where}{at(m.group(0))}: {msg}: {body[m.start():m.end() + 30]!r}")
        if body.count("<h1") != 1:
            errors.append(f"{where}: has {body.count('<h1')} level-1 headings (# Title), expected 1")
        for m in re.finditer(r"<blockquote>\s*<p>(\[![A-Za-z][^\]\n]*\])", body):
            errors.append(f"{where}{at(m.group(1))}: an unknown callout marker {m.group(1)}; use NOTE, TIP, "
                          f"IMPORTANT, WARNING or CAUTION")
        seen = set(TEMPLATE_IDS)
        for hid in ID_RE.findall(body):
            if hid in seen:
                errors.append(f"{where}{at('{#' + hid + '}', last=True)}: two elements have the id {hid!r}"
                              + (" (the page itself uses it)" if hid in TEMPLATE_IDS else ""))
            seen.add(hid)
        for h in re.findall(r"<h[23](?![^>]*\sid=)[^>]*>", body):
            errors.append(f"{where}: heading without id: {h}")
        for href in HREF_RE.findall(body):
            if href.startswith(("https://", "http://", "mailto:")):
                if not href.startswith(REPO):
                    warnings.append(f"{where}{at('(' + href)}: external link {href}")
                continue
            target, _, frag = html.unescape(href).partition("#")
            target = target[2:] if target.startswith("./") else target
            frag = urllib.parse.unquote(frag)
            shown = target + (f"#{frag}" if frag else "")
            line = at("(" + shown) or at("(" + html.unescape(href))
            tslug = slug if target == "" else target[:-5] if target.endswith(".html") else None
            if tslug is None or tslug not in ORDER:
                errors.append(f"{where}{line}: link to an unknown page: {shown}")
                continue
            if tslug not in pages:
                if tslug not in failed:
                    errors.append(f"{where}{line}: link to a missing page: {shown}")
                continue
            if frag and frag not in ids[tslug]:
                errors.append(f"{where}{line}: link to a missing anchor: {shown}")
        if lang == "ru":
            latin = len(re.findall(r"[A-Za-z]", strip_tags(re.sub(r"<(pre|code|svg)\b.*?</\1>", "", body, flags=re.S))))
            cyr = len(re.findall(r"[А-Яа-яЁё]", strip_tags(body)))
            if cyr < latin:
                warnings.append(f"{where}: more Latin than Cyrillic letters outside code ({latin} vs {cyr})")


def load(lang, errors, warnings):
    src = STRINGS[lang]["src"]
    pages = {}
    if not src.is_dir():
        return pages
    for slug in ORDER:
        p = src / f"{slug}.md"
        if p.exists():
            try:
                pages[slug] = read_fragment(p)
            except ChapterError as e:
                errors.append(str(e))
                FAILED.setdefault(lang, set()).add(slug)
    for p in sorted(src.iterdir()):
        if p.is_file() and p.stem not in ORDER:
            warnings.append(f"{rel(p)} is not built: add its name to ORDER in source/build.py")
    return pages


def report(kind, items, annotate):
    """Print each message; under GitHub Actions also as annotations, which a step shows at most ten of per kind."""
    ci = annotate and os.environ.get("GITHUB_ACTIONS") == "true"
    for k, text in enumerate(items):
        print(f"{kind}: {text}")
        if ci and (k < 9 or len(items) == 10):
            print(f"::{kind}::" + text.replace("%", "%25").replace("\r", "%0D").replace("\n", "%0A"))
    if ci and len(items) > 10:
        print(f"::{kind}::{len(items) - 9} more {kind}s: see the log of this step")


def main():
    # Messages quote Russian text; never let a non-UTF-8 console or pipe turn them into a crash.
    sys.stdout.reconfigure(errors="backslashreplace")
    only_check = "--check" in sys.argv[1:]
    errors, warnings = [], []
    editions = {lang: load(lang, errors, warnings) for lang in STRINGS}
    for lang, pages in editions.items():
        if not pages:
            if lang == "en" and not FAILED.get(lang):
                errors.append(f"no English chapters in {rel(STRINGS[lang]['src'])}")
            continue
        missing = [s for s in ORDER if s not in pages and s not in FAILED.get(lang, ())]
        if missing:
            (errors if lang == "en" else warnings).append(f"[{lang}] missing pages: {', '.join(missing)}")
        check(lang, pages, errors, warnings)
    report("warning", warnings, annotate=only_check)
    report("error", errors, annotate=only_check)
    if errors:
        print(f"FAILED: {len(errors)} error(s), {len(warnings)} warning(s)")
        return 1
    if only_check:
        print(f"CHECK OK: {sum(len(p) for p in editions.values())} pages, {len(warnings)} warning(s)")
        return 0
    written = 0
    for lang, pages in editions.items():
        if not pages:
            continue
        other = "ru" if lang == "en" else "en"
        out = STRINGS[lang]["out"]
        out.mkdir(parents=True, exist_ok=True)
        for slug in pages:
            other_exists = slug in editions[other]
            (out / f"{slug}.html").write_text(render(lang, slug, pages, other_exists), encoding="utf-8", newline="\n")
            written += 1
    print(f"BUILD OK: {written} pages, {len(warnings)} warning(s)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
