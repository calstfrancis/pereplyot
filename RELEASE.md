# Pereplyot v0.15.0 "Common Room"

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

**One window.** The library used to be a window of its own that disappeared behind the reader. It
is now the first tab of the reader window — pinned, always visible — with Library, History, Notes,
Notebooks and Search inside it. Opening a book adds a tab beside it, a click on Library takes you
home, and closing the last book lands you there. A book can still be popped out into a window of
its own, and opening a file from another app goes straight to the book.

**Fixed: a crash.** Adding a document to the Library with the + button in History closed
Pereplyot every time.

**Your place is kept.** Switching to another tab and back leaves a book where you left it.

---

### Full changelog

See [CHANGELOG.md](CHANGELOG.md).
