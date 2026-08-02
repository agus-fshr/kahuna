# Protocol decoders

A protocol decoder reads several signals at once and turns the traffic on them
into words, drawn as extra rows in the waveform view. This is what Kahuna means
by "decoder"; for the single-signal, stateless kind see
[instruction decoders](instruction-decoders).

Supported protocols: **SPI**, **I2C**, **UART**.

## Adding a decoder

1. Add the bus's signals to the view. Grouping them first is convenient but not
   required.
2. Right-click the group (or any of the signals) and pick **Decode as ▸ SPI**
   (or **I2C**, or **UART**).
3. The settings dialog opens with the roles already filled in, guessed from the
   signal names. Correct anything it got wrong and close it.

The rows that appear depend on the protocol: SPI produces `SPI MOSI` and
`SPI MISO`, I2C produces `I2C Bus`, UART produces `UART Data`. They behave like
any other row, so they can be renamed, recoloured, reordered and removed.
Removing every row of a decoder removes the decoder.

From the command line:

```text
decoder_add SPI
decoder_add I2C
decoder_add UART
```

This binds against the selected items, or against everything displayed when
nothing is selected.

## Settings

Reopen the dialog at any time with **Decoder settings...** in a decoder row's
context menu. Roles marked `*` are required; set any role to `-` to unbind it.

### SPI

| Setting | Meaning |
|---|---|
| SCLK, MOSI, MISO, CS | Which signal fills each role. Only SCLK is required. |
| Mode | SPI mode 0-3, i.e. the CPOL/CPHA pair. Mode 0 samples the rising edge with SCLK idling low. |
| Bit order | Whether the first bit of a word is its most or least significant. |
| Word size | Bits per word, 1 to 64. Not restricted to 8. |
| Format | How decoded words are rendered: hexadecimal, decimal, binary or ASCII. |
| Chip select | Whether CS asserts low or high. With CS unbound, every clock edge is decoded. |

### I2C

Both SCL and SDA are required. The output is a token stream rather than a flat
run of bytes, because the framing is most of what makes an I2C trace readable:

```text
S    50 W    A    00    A    2A    A    P
```

`S` start, `Sr` repeated start, `P` stop, `A` acknowledge, `N` not acknowledged.

| Setting | Meaning |
|---|---|
| Address | `7-bit + R/W` splits the address frame into an address and a direction; `Raw frame` shows the byte as it went over the wire. |
| Format | How byte values are rendered. |

### UART

Decodes one line. A full-duplex link is two decoders, which is also how logic
analysers model it: the two directions have independent framing and may even
run at different rates.

| Setting | Meaning |
|---|---|
| Line | The signal to decode. Required. |
| Bit period | Length of one bit in timescale ticks. Leave **Measure** ticked to take it from the narrowest pulse on the line, which is correct whenever the traffic contains a bit differing from both its neighbours. |
| Data bits | 5 to 9. |
| Parity | None, even or odd. A mismatch is reported as `parity`. |
| Stop bits | 1 or 2. Every stop bit must be at the idle level. |
| Bit order | LSB first is conventional. |
| Idle level | The level the line rests at. Set it low for an inverted line; the data bits invert with it. |
| Format | ASCII by default, since serial traffic is usually text. |

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

## Performance

A decode result is cached and reused, so panning and zooming rebuild only the
on-screen blocks rather than re-reading the bound signals. The cached result is
discarded when the settings or role bindings change, when the waveform is
reloaded, or when a bound signal finishes loading, since a decode that ran
before its inputs existed would otherwise stay empty forever.

## Adding another protocol

Each protocol is a module beside `spi.rs` in `libsurfer/src/decoders/`, plus a
variant in `Protocol` and `DecoderSettings`. A decoder receives `BitLane`s of
pre-extracted transitions rather than reading the waveform itself, which is
what lets the SPI decoder be unit tested without loading a file. Follow that
pattern and the tests come for free.
