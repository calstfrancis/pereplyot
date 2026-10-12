# Pereplyot v0.17.1 "Steady Page"

Install via Flatpak:

```bash
flatpak remote-add --user calstfrancis \
  https://calstfrancis.github.io/flatpak/calstfrancis.flatpakrepo
flatpak install calstfrancis io.github.calstfrancis.Pereplyot
```

Already installed? Update with:

```bash
flatpak update io.github.calstfrancis.Pereplyot
```

---

### What's new

**Reading mode stays responsive on a long book.** On a 600-page book the window used to freeze for
up to a second while the text loaded and again at the first click or key press. The text view was
making GTK lay out the whole book at every redraw; that is fixed. A far jump forward on a very long
book is smoother but still takes a couple of seconds.

**The page counter follows your scrolling in Reading mode.** The page number and chapter used to
stay on page 1 however far you scrolled.

---

### Full changelog

See [CHANGELOG.md](CHANGELOG.md).
