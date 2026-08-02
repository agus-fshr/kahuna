# Protocol decoders

A protocol decoder reads several signals at once and turns the traffic on them
into words, drawn as extra rows in the waveform view. This is what Kahuna means
by "decoder"; for the single-signal, stateless kind see
[instruction decoders](instruction-decoders).

Supported protocols: **SPI**.

## Adding a decoder

1. Add the bus's signals to the view. Grouping them first is convenient but not
   required.
2. Right-click the group (or any of the signals) and pick **Decode as ▸ SPI**.
3. The settings dialog opens with the roles already filled in, guessed from the
   signal names. Correct anything it got wrong and close it.

A row appears per data direction: `SPI MOSI` and `SPI MISO`. They behave like
any other row, so they can be renamed, recoloured, reordered and removed.
Removing every row of a decoder removes the decoder.

From the command line:

```text
decoder_add SPI
```

This binds against the selected items, or against everything displayed when
nothing is selected.

## Settings

Reopen the dialog at any time with **Decoder settings...** in a decoder row's
context menu.

| Setting | Meaning |
|---|---|
| SCLK, MOSI, MISO, CS | Which signal fills each role. Only SCLK is required; set a role to `-` to leave it unbound. |
| Mode | SPI mode 0-3, i.e. the CPOL/CPHA pair. Mode 0 samples the rising edge with SCLK idling low. |
| Bit order | Whether the first bit of a word is its most or least significant. |
| Word size | Bits per word, 1 to 64. Not restricted to 8. |
| Format | How decoded words are rendered: hexadecimal, decimal, binary or ASCII. |
| Chip select | Whether CS asserts low or high. With CS unbound, every clock edge is decoded. |

## Reading the output

A decoded row is not a waveform, and is drawn so that this is obvious at a
glance rather than only from its name:

* **Decoded words** are filled blocks with rounded corners and no trace
  outline, in a violet that no waveform uses (`decoder_value`). Waveforms are
  outlined shapes; decoded words are filled ones. Consecutive words are
  separated by a small gap so a run of them does not merge into one band.
* **Source signals** that a decoder reads are tinted amber (`decoder_source`)
  wherever they are displayed, so it is visible which waveforms feed a decoded
  row. Setting a color on a signal yourself always wins over this tint.
* **Row names** of decoded rows are italic, as dividers are.

Both colors are theme settings and can be overridden in a theme file. Themes
written before decoders existed keep working and fall back to the defaults.

Words are drawn as blocks spanning from the clock edge that latched their first
bit to the start of the next word.

Blocks drawn as errors mean the decoder could not trust that word:

* `N of M bits` — the transfer was cut short, either by chip select going
  inactive partway through a word or by the trace ending. The number says how
  many bits did arrive.
* `x` — an `x` or `z` was sampled on the data line. Only that word is affected;
  the ones after it still decode, because the bad bit still occupies its slot
  and the framing survives.

Error blocks are drawn in the theme's warning color rather than the decoder
color, so a bad word stands out from the good ones around it.

If a required role is unbound the rows stay empty, and the dialog says which
role is missing rather than failing silently.

## Adding another protocol

Each protocol is a module beside `spi.rs` in `libsurfer/src/decoders/`, plus a
variant in `Protocol` and `DecoderSettings`. A decoder receives `BitLane`s of
pre-extracted transitions rather than reading the waveform itself, which is
what lets the SPI decoder be unit tested without loading a file. Follow that
pattern and the tests come for free.
