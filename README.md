# The Moirai book (GitHub Pages branch)

This branch holds the public website that explains [Moirai](https://github.com/bluesteelll/moirai) to newcomers:
https://bluesteelll.github.io/moirai/ (English) and https://bluesteelll.github.io/moirai/ru/ (Russian).

It shares no history with `master`. The branch holds only the sources; GitHub Actions builds the pages on every push
and publishes them:

- `source/chapters/<slug>.md`: the English chapters, in Markdown;
- `source/chapters-ru/<slug>.md`: the Russian edition, translated from the English one;
- `assets/`: the stylesheet and the icon;
- `source/build.py`: turns the chapters into pages (English at the root, Russian under `ru/`) after checking links,
  anchors and forbidden content; its Markdown parser is pinned in `source/requirements.txt`;
- `.github/workflows/book.yml`: runs `source/build.py --check`, then the build, and publishes the result.

| File | Chapter |
|---|---|
| `index.md` | Home |
| `why.md` | 1. Why Moirai |
| `concepts.md` | 2. The graph |
| `tour.md` | 3. A tour |
| `versions.md` | 4. Branches & merge |
| `git-image.md` | 5. Git image |
| `file-links.md` | 6. File links |
| `lachesis.md` | 7. Lachesis |
| `agents.md` | 8. Agents |
| `engine.md` | 9. The engine |
| `building.md` | 10. Roadmap & status |
| `faq.md` | FAQ |
| `glossary.md` | Glossary |

## Editing

On GitHub: open a chapter under `source/chapters/` or `source/chapters-ru/` on the `gh-pages` branch, edit it with
the pencil button and commit. The site updates a minute or two later.

In a local clone: edit a chapter, commit and push `gh-pages`. To preview first, install the parser once, build the
pages (the generated `*.html` and `ru/` are ignored by git) and serve the folder:

```
python -m pip install --require-hashes -r source/requirements.txt
python source/build.py --check
python source/build.py
python -m http.server 8765
```

If the check fails, the run in the Actions tab fails, and its summary names the file, for most problems the line,
and the problem (at most ten per run; the log of the "Check the sources" step has all of them). The site keeps its last
good version. GitHub also emails the person who pushed. To rebuild without a change, open
the last run in the Actions tab and choose "Re-run all jobs". The two editions are separate: a change to one does
not change the other.

GitHub's own preview of a chapter is not the site: it shows the front matter as a table and `{#id}` as text, leaves
glossary entries as plain lines and drops the diagrams. Judge a change by the published page, or build locally.

## Writing a chapter

A chapter is ordinary Markdown (CommonMark with GitHub tables) after a front matter block:

```
---
title: Why Moirai
nav: Why Moirai
description: One sentence for search engines and link previews.
---

# Why Moirai

The first paragraph after the title is the lead, set larger.

## The problem {#the-problem}

Text, **bold**, *italic*, `code`, [a link to another chapter](tour.html#create-the-store).
```

- `title` is the browser tab (the home page's tab is fixed in `source/build.py`), `nav` the name in the
  contents, `description` the page summary. A value that contains `: ` goes in double quotes, all of it.
- `{#id}` after a `##` or `###` heading sets its anchor, which links and the "On this page" list use. Without it the
  anchor is made from the heading text. Keep the ids of existing headings: other chapters link to them. An id is a
  letter or digit (Latin or Cyrillic), then letters, digits, `-` and `_`; a `{#...}` left in the text, anywhere but
  after a heading or a glossary term, is an error, so a literal one goes in backticks.
- A link to another chapter is `[text](tour.html)` or `[text](tour.html#an-id)`, within the same edition.
- Code blocks are fenced; the language after the fence (`console`, `lq`, `moi`, `text`) sets the label. In a
  `console` block, lines that start with `$ ` are commands.
- A paragraph that is only italic text, right after a code block, is the block's caption:

  ````
  ```console
  $ moirai init
  ```

  *Designed output.*
  ````

- A callout is a quote that starts with a marker, as on GitHub: `[!NOTE]`, `[!TIP]` or `[!WARNING]` (also
  `[!IMPORTANT]` and `[!CAUTION]`). A line that is all bold, right after the marker, is its title; text after the
  marker on the same line is a title too. Without either, the callout has no title:

  ```
  > [!NOTE]
  > **Status**
  >
  > Moirai is not released yet.
  ```

- A glossary entry is a term with an id, then a line that starts with `: `. An empty line separates entries;
  without it the new term runs into the previous definition (the check reports the stray `{#id}`):

  ```
  Accelerator {#term-accelerator}
  : Something that saves a call but that nothing depends on.

  Actor {#term-actor}
  : ...
  ```

- In a table, a `|` inside a cell, code included, is written `\|`.
- A placeholder in angle brackets, such as `<store>`, goes in backticks; outside them it would be read as an HTML
  tag, and the check refuses it.
- Three kinds of HTML block pass through unchanged: the folding blocks (`<details>` with a `<summary>` line, an
  empty line, Markdown, an empty line, `</details>`), the diagrams (`<figure class="diagram">` with an inline
  `<svg>`, drawn with the stylesheet's `d-*` classes) and the cards on the home page (`<div class="cards">`). An HTML
  block ends at its first empty line, so a diagram or the cards have no empty lines inside, and their text is HTML
  (`<code>`, `<em>`), not Markdown. The `<summary>` line is HTML too: code in it is `<code>...</code>`, not
  backticks. Any other HTML is refused, except comments (`<!-- ... -->`, starting on their own line); so are
  images, media, scripts, event handlers and inline styles.
- A new chapter needs its name in `ORDER` in `source/build.py` (before `faq` to get a chapter number), a row in the
  table above, and, to appear on the home page, a card and a line in `index.md` of both editions.

The book describes the design; the specification in `docs/spec/` on `master` is authoritative.
