#!/usr/bin/env python3
"""Generate a VCD containing SPI, UART and I2C bus traffic.

The waveform is meant as a development fixture for Kahuna's protocol
decoders: every bus carries a known payload, so a decoder can be checked
against the expected bytes listed in ``EXPECTED`` below.

Usage:
    python3 examples/kahuna_protocols.py [output.vcd]
"""

import sys

TIMESCALE = "1ns"

# Payloads carried by each bus, for checking decoder output against.
EXPECTED = {
    "spi": [(0x9F, 0x00), (0x00, 0xEF), (0x00, 0x40), (0x00, 0x18)],  # (mosi, miso)
    "uart": [0x4B, 0x61, 0x68, 0x75, 0x6E, 0x61],  # "Kahuna"
    "i2c": [(0x50, "w", [0x00, 0x2A])],  # addr, dir, data
}

CLK_PERIOD = 10  # 100 MHz system clock
SPI_HALF = 50  # 10 MHz SCLK
UART_BIT = 1000  # 1 Mbaud
I2C_HALF = 1250  # 400 kHz SCL

SPI_START = 1_000
UART_START = 60_000
I2C_START = 130_000
END_TIME = 200_000


class Vcd:
    """Collects value changes and writes them out in time order."""

    def __init__(self):
        self.vars = []  # (scope_path, name, width, ident)
        self.events = {}  # time -> {ident: value string}
        self._next_id = 0

    def add_var(self, scope, name, width=1):
        ident = chr(ord("!") + self._next_id)
        self._next_id += 1
        self.vars.append((scope, name, width, ident))
        return ident

    def set(self, time, ident, value, width=1):
        assert time >= 0
        if width == 1:
            text = f"{value}{ident}"
        else:
            text = f"b{value:0{width}b} {ident}"
        self.events.setdefault(time, {})[ident] = text

    def write(self, path):
        with open(path, "w") as f:
            f.write(f"$timescale\n\t{TIMESCALE}\n$end\n")

            # Emit the scope tree. Scopes are given as tuples of names.
            scopes = sorted({v[0] for v in self.vars})
            open_scope = ()
            for scope in scopes:
                common = 0
                while (
                    common < len(open_scope)
                    and common < len(scope)
                    and open_scope[common] == scope[common]
                ):
                    common += 1
                for _ in range(len(open_scope) - common):
                    f.write("$upscope $end\n")
                for name in scope[common:]:
                    f.write(f"$scope module {name} $end\n")
                open_scope = scope
                for s, name, width, ident in self.vars:
                    if s != scope:
                        continue
                    suffix = "" if width == 1 else f" [{width - 1}:0]"
                    f.write(f"$var wire {width} {ident} {name}{suffix} $end\n")
            for _ in range(len(open_scope)):
                f.write("$upscope $end\n")

            f.write("$enddefinitions $end\n")

            times = sorted(self.events)
            first = times[0] if times else 0
            f.write(f"#{first}\n$dumpvars\n")
            for text in self.events[first].values():
                f.write(f"{text}\n")
            f.write("$end\n")
            for t in times[1:]:
                f.write(f"#{t}\n")
                for text in self.events[t].values():
                    f.write(f"{text}\n")


def gen_clock(vcd, ident, start, end, half_period, start_high=False):
    """Free-running clock between start and end."""
    level = 1 if start_high else 0
    t = start
    while t <= end:
        vcd.set(t, ident, level)
        level ^= 1
        t += half_period


def gen_spi(vcd, sclk, cs_n, mosi, miso, start):
    """SPI mode 0: sample on rising edge, MSB first, active-low chip select."""
    t = start
    vcd.set(0, cs_n, 1)
    vcd.set(0, sclk, 0)
    vcd.set(0, mosi, 0)
    vcd.set(0, miso, 0)

    vcd.set(t, cs_n, 0)
    t += SPI_HALF
    for tx, rx in EXPECTED["spi"]:
        for bit in range(7, -1, -1):
            # Drive both lines while SCLK is low, sample on the rising edge.
            vcd.set(t, mosi, (tx >> bit) & 1)
            vcd.set(t, miso, (rx >> bit) & 1)
            vcd.set(t + SPI_HALF, sclk, 1)
            vcd.set(t + 2 * SPI_HALF, sclk, 0)
            t += 2 * SPI_HALF
    t += SPI_HALF
    vcd.set(t, cs_n, 1)
    vcd.set(t, mosi, 0)
    vcd.set(t, miso, 0)
    return t


def gen_uart(vcd, tx, start):
    """8N1, LSB first, idle high."""
    vcd.set(0, tx, 1)
    t = start
    for byte in EXPECTED["uart"]:
        vcd.set(t, tx, 0)  # start bit
        t += UART_BIT
        for bit in range(8):
            vcd.set(t, tx, (byte >> bit) & 1)
            t += UART_BIT
        vcd.set(t, tx, 1)  # stop bit
        t += UART_BIT
        t += UART_BIT * 2  # idle between bytes
    return t


def gen_i2c(vcd, scl, sda, start):
    """Single write transaction: START, addr+W, ACK, data bytes, STOP."""
    vcd.set(0, scl, 1)
    vcd.set(0, sda, 1)
    t = start

    def bit(value):
        """One SCL pulse carrying `value` on SDA."""
        nonlocal t
        vcd.set(t, sda, value)
        vcd.set(t + I2C_HALF // 2, scl, 1)
        vcd.set(t + I2C_HALF // 2 + I2C_HALF, scl, 0)
        t += I2C_HALF * 2

    addr, direction, data = EXPECTED["i2c"][0]

    # START: SDA falls while SCL is high.
    vcd.set(t, sda, 0)
    t += I2C_HALF // 2
    vcd.set(t, scl, 0)
    t += I2C_HALF // 2

    frame = (addr << 1) | (0 if direction == "w" else 1)
    for i in range(7, -1, -1):
        bit((frame >> i) & 1)
    bit(0)  # ACK from the target
    for byte in data:
        for i in range(7, -1, -1):
            bit((byte >> i) & 1)
        bit(0)  # ACK

    # STOP: SDA rises while SCL is high.
    vcd.set(t, sda, 0)
    vcd.set(t + I2C_HALF // 2, scl, 1)
    vcd.set(t + I2C_HALF, sda, 1)
    return t + I2C_HALF


def main():
    out = sys.argv[1] if len(sys.argv) > 1 else "examples/kahuna_protocols.vcd"
    vcd = Vcd()

    clk = vcd.add_var(("tb",), "clk")
    rst_n = vcd.add_var(("tb",), "rst_n")
    counter = vcd.add_var(("tb",), "counter", 8)

    sclk = vcd.add_var(("tb", "spi"), "sclk")
    cs_n = vcd.add_var(("tb", "spi"), "cs_n")
    mosi = vcd.add_var(("tb", "spi"), "mosi")
    miso = vcd.add_var(("tb", "spi"), "miso")

    uart_tx = vcd.add_var(("tb", "uart"), "tx")

    scl = vcd.add_var(("tb", "i2c"), "scl")
    sda = vcd.add_var(("tb", "i2c"), "sda")

    gen_clock(vcd, clk, 0, END_TIME, CLK_PERIOD // 2)

    # Reset is asserted (low) for the first 20 clock cycles.
    vcd.set(0, rst_n, 0)
    vcd.set(20 * CLK_PERIOD, rst_n, 1)

    # A byte counter ticking on the rising clock edge once out of reset.
    value = 0
    vcd.set(0, counter, 0, 8)
    for t in range(20 * CLK_PERIOD, END_TIME, CLK_PERIOD):
        value = (value + 1) & 0xFF
        vcd.set(t, counter, value, 8)

    gen_spi(vcd, sclk, cs_n, mosi, miso, SPI_START)
    gen_uart(vcd, uart_tx, UART_START)
    gen_i2c(vcd, scl, sda, I2C_START)

    vcd.set(END_TIME, clk, 0)
    vcd.write(out)
    print(f"wrote {out}")


if __name__ == "__main__":
    main()
