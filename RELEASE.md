# Pereplyot v0.4.0 "Shared Binding"

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

**Kartoteka and Sputnik now launch this app directly to open a document**, instead of each
embedding their own pinned copy of the reader — one reader implementation to keep current,
not three copies that quietly drift apart. Opening a citation from Kartoteka, or a course
material or library reading from Sputnik, launches Pereplyot with its annotations, reading
position, and page-numbering override routed straight into that app's own vault or
material store — behaving exactly as it always has, just with this app's reader doing the
work instead of a stale embedded one. If Pereplyot isn't installed, both apps fall back to
opening the file with your system's default handler instead.

**Fixed:** closing the reader via its window's own close button — not an explicit tab
close — silently never saved your reading position. The most common way anyone actually
closes a reader, now fixed.

---

### Full changelog

See [CHANGELOG.md](CHANGELOG.md).
