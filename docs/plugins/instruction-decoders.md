# Instruction decoders

Instruction decoders allow translating n-bit signals into nice text representations.
They are based on the [instruction-decoder](https://github.com/ics-jku/instruction-decoder) crate.
They act on a single signal at a time and are stateless: each value is decoded on its own.

> **Note:** these were previously called just "decoders" and were read from a
> `decoders` directory. That name is now reserved for protocol decoders, which
> decode traffic spanning several signals over time. If you have an old
> `decoders` directory, rename it to `instruction-decoders` — Kahuna logs a
> warning if it finds one and will not load it.

To add additional instruction decoders to Kahuna, create an `instruction-decoders` directory in the config directory and add your decoders inside there.

| Os      | Path                                                                  |
|---------|-----------------------------------------------------------------------|
| Linux   | `~/.config/surfer/instruction-decoders/`                                        |
| Windows | `C:\Users\<Name>\AppData\Roaming\surfer-project\surfer\config\instruction-decoders\`  |
| macOS   | `/Users/<Name>/Library/Application Support/org.surfer-project.surfer/instruction-decoders/` |

To add a new instruction decoder, create a subdirectory inside `instruction-decoders` and add the required toml files.
An instruction decoder can consist of multiple toml files which will be merged.
You can also add project-specific instruction decoders by creating subdirectories in `.surfer/instruction-decoders`.

The instruction decoders show up as additional formats.

For simpler use cases, you may want to consider a [mapping](mapping) translator.
