# Pereplyot v0.16.1 "Light Load"

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

**Big EPUBs unpack in the background.** A book that has to be unpacked first no longer freezes the
window while it is: it says it is unpacking and opens when ready, and a second click doesn't start a
second reader.

**Heavily marked books open at once.** The notes sidebars build 300 cards at a time with a "Show more"
row, so a book with thousands of highlights no longer makes the window slow to open.

---

### Full changelog

See [CHANGELOG.md](CHANGELOG.md).
