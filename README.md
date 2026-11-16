# The Moirai book (GitHub Pages branch)

This branch holds the public website that explains [Moirai](https://github.com/bluesteelll/moirai) to newcomers:
https://bluesteelll.github.io/moirai/ (English) and https://bluesteelll.github.io/moirai/ru/ (Russian).

It shares no history with `master`. The pages are generated from fragments:

- `source/chapters/<slug>.html`: the English page fragments (line 1 is a `<!--meta {...} -->` line);
- `source/chapters-ru/<slug>.html`: the Russian edition, translated from the English one;
- `assets/`: the stylesheet and the icon;
- `source/build.py`: writes the English pages at the branch root and the Russian ones under `ru/`, after checking
  links, anchors and forbidden content (standard-library Python 3, no dependencies).

Rebuild after an edit:

```
python source/build.py --check
python source/build.py
```

The book describes the design; the specification in `docs/spec/` on `master` is authoritative.
