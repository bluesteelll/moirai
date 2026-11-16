#!/usr/bin/env python3
"""Build the Moirai book.

Reads the page fragments in source/chapters/ (English) and source/chapters-ru/ (Russian) and writes complete static
pages: the English edition at the branch root, the Russian edition under ru/. Standard library only.

Usage: python source/build.py [--check]
  --check  build nothing; only run the checks (links, forbidden content, fragment format).
Exit status: 0 when every check passes, 1 when any error is found.
"""

import html
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
REPO = "https://github.com/bluesteelll/moirai"

ORDER = ["index", "why", "concepts", "tour", "versions", "git-image", "file-links", "lachesis", "agents", "engine",
         "building", "faq", "glossary"]
NUMBERED = ORDER[1:11]
AFTER_SEP = ["faq", "glossary"]

STRINGS = {
    "en": {
        "src": ROOT / "source" / "chapters",
        "out": ROOT,
        "up": "",
        "site": "Moirai book",
        "home_title": "Moirai — the book",
        "book": "book",
        "banner": "<strong>Preview.</strong> This book describes Moirai as it is designed. Nothing is released yet: "
                  "the project is in milestone M0, writing and testing its specification. Commands and outputs show "
                  "the designed behaviour and may change.",
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
        "banner": "<strong>Предварительная версия.</strong> Книга описывает Moirai таким, каким он спроектирован. "
                  "Ничего ещё не выпущено: проект на вехе M0, где пишется и проверяется его спецификация. Команды и "
                  "их вывод показывают задуманное поведение и могут измениться.",
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

LOGO = ('<svg viewBox="0 0 32 32" aria-hidden="true"><rect width="32" height="32" rx="9" fill="#17182b"/>'
        '<path d="M5 11c6-5 12 5 22 0" fill="none" stroke="#e0b45c" stroke-width="2.3" stroke-linecap="round"/>'
        '<path d="M5 16.5c6-5 12 5 22 0" fill="none" stroke="#8f8cf0" stroke-width="2.3" stroke-linecap="round"/>'
        '<path d="M5 22c6-5 9 3 14 1.6" fill="none" stroke="#e57a8c" stroke-width="2.3" stroke-linecap="round"/></svg>')

FONTS = ('<link rel="preconnect" href="https://fonts.googleapis.com">'
         '<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>'
         '<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Cormorant+Garamond:ital,wght@0,500;0,600;'
         '0,700;1,500&amp;family=IBM+Plex+Sans:ital,wght@0,400;0,500;0,600;1,400&amp;family=JetBrains+Mono:wght@400;600'
         '&amp;display=swap">')

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

HERO = ('<div class="hero" aria-hidden="true"><svg class="hero-threads" viewBox="0 0 720 150">'
        '<path class="t t1" d="M0,75 Q90,15 180,75 T360,75 T540,75 T720,75"/>'
        '<path class="t t2" d="M0,75 Q90,135 180,75 T360,75 T540,75 T720,75"/>'
        '<path class="t t3" d="M0,75 Q60,40 120,75 T240,75 T360,75 T480,75 T600,75"/>'
        '<circle class="bead" cx="120" cy="75" r="3.6"/><circle class="bead" cx="240" cy="75" r="3.6"/>'
        '<circle class="bead" cx="480" cy="75" r="3.6"/>'
        '<circle class="knot" cx="180" cy="75" r="7"/><circle class="knot" cx="360" cy="75" r="7"/>'
        '<circle class="knot" cx="540" cy="75" r="7"/>'
        '<path class="cut" d="M607,64 L621,86 M607,86 L621,64"/>'
        '</svg></div>')

LANG_LABEL = {"console": "terminal", "lq": "Lachesis", "moi": ".moi", "text": ""}

# Forbidden content: (pattern, message). Errors stop the build.
FORBIDDEN = [
    (r"Boyko", "names the author or the author's other project"),
    (r"<ORCH>|D:/orch|orch/moirai", "names the private orchestration state"),
    (r"\bR-\d{2,3}\b", "cites an internal ruling id"),
    (r"(?<![A-Za-z])[A-Za-z]:[\\/](?:Users|claude|moirai|orch|tmp)", "contains a local path"),
    (r"\bprivate/", "points at the private directory"),
    (r"Moirai-Commit|Moirai-Kind|Moirai-Hlc|refs/moirai/|\.moirai-image", "uses a pre-Klotho image name"),
    (r"<script|<img|<iframe|<link|<style", "contains a forbidden element"),
    (r"\sstyle=", "contains an inline style"),
    (r"<svg[^>]*\s(?:width|height)=", "gives an svg a fixed size"),
    (r"\s(?:fill|stroke|font-size|font-family|stroke-width)=\"", "sets a presentation attribute in an svg"),
]

META_RE = re.compile(r"^<!--meta (\{.*\}) -->\s*$")
ID_RE = re.compile(r'\sid="([^"]+)"')
HREF_RE = re.compile(r'\shref="([^"]+)"')
HEADING_RE = re.compile(r'<(h[23]) id="([^"]+)">(.*?)</\1>', re.S)
TAG_RE = re.compile(r"<[^>]+>")


def read_fragment(path):
    text = path.read_text(encoding="utf-8")
    first, _, body = text.partition("\n")
    m = META_RE.match(first.strip())
    if not m:
        raise ValueError(f"{path}: line 1 is not a <!--meta {{...}} --> line")
    meta = json.loads(m.group(1))
    for key in ("title", "nav", "description"):
        if not meta.get(key):
            raise ValueError(f"{path}: meta has no {key}")
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
    if slug == "index":
        content = content.replace("<h1", HERO + "<h1", 1)
    else:
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
    for slug, (meta, body) in pages.items():
        where = f"[{lang}] {slug}"
        for pat, msg in FORBIDDEN:
            for m in re.finditer(pat, body):
                line = body.count("\n", 0, m.start()) + 2
                errors.append(f"{where}:{line}: {msg}: {body[m.start():m.end() + 30]!r}")
        if body.count("<h1") != 1:
            errors.append(f"{where}: has {body.count('<h1')} h1 elements, expected 1")
        seen = set()
        for hid in ID_RE.findall(body):
            if hid in seen:
                errors.append(f"{where}: duplicate id {hid!r}")
            seen.add(hid)
        for tag, hid, inner in HEADING_RE.findall(body):
            pass
        for h in re.findall(r"<h[23](?![^>]*\sid=)[^>]*>", body):
            errors.append(f"{where}: heading without id: {h}")
        for href in HREF_RE.findall(body):
            if href.startswith(("https://", "http://", "mailto:")):
                if not href.startswith(REPO):
                    warnings.append(f"{where}: external link {href}")
                continue
            target, _, frag = href.partition("#")
            tslug = slug if target == "" else target[:-5] if target.endswith(".html") else None
            if tslug is None or tslug not in ORDER:
                errors.append(f"{where}: link to an unknown page: {href}")
                continue
            if tslug not in pages:
                errors.append(f"{where}: link to a missing page: {href}")
                continue
            if frag and frag not in ids[tslug]:
                errors.append(f"{where}: link to a missing anchor: {href}")
        if lang == "ru":
            latin = len(re.findall(r"[A-Za-z]", strip_tags(re.sub(r"<(pre|code|svg)\b.*?</\1>", "", body, flags=re.S))))
            cyr = len(re.findall(r"[А-Яа-яЁё]", strip_tags(body)))
            if cyr < latin:
                warnings.append(f"{where}: more Latin than Cyrillic letters outside code ({latin} vs {cyr})")


def load(lang):
    src = STRINGS[lang]["src"]
    pages = {}
    if not src.is_dir():
        return pages
    for slug in ORDER:
        p = src / f"{slug}.html"
        if p.exists():
            pages[slug] = read_fragment(p)
    return pages


def main():
    only_check = "--check" in sys.argv[1:]
    errors, warnings = [], []
    editions = {lang: load(lang) for lang in STRINGS}
    for lang, pages in editions.items():
        if not pages:
            continue
        missing = [s for s in ORDER if s not in pages]
        if missing:
            (errors if lang == "en" else warnings).append(f"[{lang}] missing pages: {', '.join(missing)}")
        check(lang, pages, errors, warnings)
    for w in warnings:
        print("warning:", w)
    for e in errors:
        print("error:", e)
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
