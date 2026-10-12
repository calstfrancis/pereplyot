# Pereplyot v0.15.1 "Tight Binding"

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

A hardening release, from a deliberate attempt to break Pereplyot with hostile files, random input and
fuzzing.

**A crash fixed.** A scanned book whose page boxes were broken could send Reading mode into endless
recursion and take the whole app down.

**EPUBs stay offline.** A book's remote images, stylesheets and frames used to be fetched, telling
whoever hosted them that you had opened it, and a frame or redirect could start your browser on its own.
Neither happens now; only a link you click opens outside. An EPUB that would unpack to more than 4 GB
is refused.

**Open several files at once.** Selecting a few files and choosing Open With Pereplyot opens all of
them (it used to open none), and a relative path works even when Pereplyot is already running.

**Safer data.** History keeps 1,000 documents instead of 50, so notes and links on older books stay
reachable, and a damaged history file is set aside rather than reset. The Markdown notes folder never
overwrites a file that isn't its own. Closing a book no longer quits the app while you are using the
Library.

**Smoother.** Library thumbnails are made in the background, very large PDFs are no longer read into
memory whole, Ctrl+W closes a tab and Ctrl+1 goes to the Library, and the window title names the book
on screen.

---

### Full changelog

See [CHANGELOG.md](CHANGELOG.md).
