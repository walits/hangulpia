# Hangulpia

**One script, every language.**

Hangul is a designed writing system — built from the shape of the human
mouth and throat, with a rulebook precise enough to derive from first
principles. Hangulpia is teaching it to carry sounds beyond Korean,
starting with Japanese.

🌐 **[hangulpia.com](https://hangulpia.com)** — try the live sentence
converter in your browser (same engine as the downloadable app, compiled
to WebAssembly — not a reimplementation)

## What's here

A working macOS input method that lets you type Japanese using a regular
Dubeolsik (두벌식) Korean keyboard layout, converting Hangul to Hiragana
(and kanji, where the vocabulary has a verified reading) in real time.

| | |
|---|---|
| 🖥️ **Try it now** | [hangulpia.com](https://hangulpia.com) — no install, runs in-browser |
| 📦 **Download** | [Latest macOS build](https://hangulpia.com/downloads/HangulJapaneseIME-latest.zip) (Apple Silicon, macOS 13+) |
| 📖 **How it works** | [`docs/MODEL.md`](docs/MODEL.md) — how the engine is built, trained, and runs on both platforms |

## Repository layout

This is a monorepo — the product, the homepage, and the docs all live
together and move together.

```
core/       Rust workspace — the conversion engine + platform bindings
  crates/
    hangul/       Hangul syllable decomposition
    japanese/     Romaji → Hiragana tables
    db/           PhoneticMap + BeamDecoder (the actual engine) + vocabulary
    macos-ime/    Swift InputMethodKit app, bridged to the Rust engine via C FFI
    windows-ime/  Windows port (in progress, not yet distributed)
    wasm/         wasm-bindgen build of the same engine, for the homepage
    cli/          Benchmark/diagnostic binaries used during development
homepage/   Static site served at hangulpia.com (GitHub Pages, $0/month)
docs/       Engine documentation + the original research paper
infra/      Small AWS Lambda for anonymous download-click counting
```

## Quick start

**Just want to use it?** Go to [hangulpia.com](https://hangulpia.com) —
download the macOS app, or try the converter directly in your browser.

**Building from source:**

```bash
cd core
cargo test -p ime-db                    # run the engine's test suite
cargo build --release -p ime-macos      # build the Rust core for macOS

cd crates/macos-ime
./dist.sh                               # build + package the macOS app
```

Full build/run instructions for every surface (Rust API, CLI, WASM,
macOS app) are in [`docs/MODEL.md`](docs/MODEL.md).

## How it's built

Not a neural model — a statistical hangul↔hiragana alignment table
(`PhoneticMap`) learned from a small, hand-verified vocabulary, layered
with explicit grammar rules for the patterns statistics alone can't
catch (topic/subject particles, verb conjugation endings). The exact
same Rust engine runs natively in the macOS app (via C FFI) and in the
browser (via WebAssembly) — one implementation, two interfaces, always
in sync. See [`docs/MODEL.md`](docs/MODEL.md) for the full architecture,
the development history, and the honest list of current limitations.

## Roadmap

Japanese is live. Hangul has done this before: in 2009, the Cia-Cia
language of Buton Island, Indonesia — with no writing system of its
own — adopted Hangul because it captured their sounds best. Vietnamese,
Thai, and Mongolian are in research; English, Chinese, Spanish, French,
and German are planned. See the [roadmap on the homepage](https://hangulpia.com/#roadmap).

## Contributing

Linguist, phonetician, or just interested in an under-served language?
Fork it, open a PR, or [say hi](mailto:walits.co@gmail.com).

## License

MIT
