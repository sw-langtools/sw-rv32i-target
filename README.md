# sw-rv32i-target

Board and target metadata for RISC-V demos.

## Board TOML

Each board lives in `boards/*.toml` and describes enough metadata for the
emulator and future loaders to bind demos to board capabilities:

```toml
id = "esp32-c3-devkitm-1"
family = "esp32-c3"
arch = "rv32imc"

[memory]
ram_base = "0x3fc80000"
ram_size = "0x00060000"

[gpio]
pins = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10]

[aliases]
led = 8

[mmio.gpio]
kind = "generic-gpio"
base = "0x60004000"

[mmio.uart0]
kind = "generic-uart"
base = "0x60000000"
```

Blink demos should bind to logical aliases such as `led`, not directly to a
board id. Boards that map `led` to the same GPIO pin can share the same resolved
demo configuration; boards with different pins can reuse the same demo source
with different board bindings.

The first board files are placeholders for ESP32-C3, ESP32-C5, and ESP32-C6
development boards. They intentionally use a generic GPIO MMIO kind until a
register-accurate device model is added.

The CH32V003 placeholder uses `arch = "rv32ec"` so board-driven demos exercise
the RV32E profile. Its `led` alias is modeled as a flat generic GPIO pin for the
shared emulator blink contract; real CH32V003 board variants use port/pin names
such as GPIOC pin 1 or a board LED connected through jumpers, which will need a
more specific pin model later.

## Blink Demos

Reusable demos live in `demos/*.toml` and bind to logical board signals:

```toml
id = "blink"
signal = "led"
cycles = 3
```

`BlinkDemo::run` resolves `signal = "led"` through the selected board's
`[aliases]` table and records a deterministic GPIO trace. For the initial
ESP32-C3/C5/C6 placeholders, all three boards map `led = 8`, so the same blink
demo produces the same GPIO8 high/low trace for each board.

## Generic GPIO MMIO

Boards with `[mmio.gpio] kind = "generic-gpio"` can be attached to `MmioBus`.
The generic GPIO block is intentionally small and not register-accurate:

- `base + 0x00`: write-one-to-set GPIO pins
- `base + 0x04`: write-one-to-clear GPIO pins
- `base + 0x08`: read GPIO output state

This gives emulator-facing code a stable MMIO contract for early blink demos
without pulling in ESP-IDF or modeling ESP32-specific GPIO registers yet.

## Generic UART MMIO

Boards can declare a generic UART separately from GPIO:

```toml
[mmio.uart0]
kind = "generic-uart"
base = "0x60000000"
```

The generic UART block is intentionally small and not register-accurate:

- `base + 0x00`: write low byte to TX output
- `base + 0x04`: read status; bit 0 means TX ready

This gives emulator-facing code a stable hello-style output contract. It is not
an ESP32 UART model yet: real ESP32-C3 UART support needs the actual register
map, FIFO/status behavior, clock/reset setup, and loader/runtime conventions.

## Emulator Integration

`sw-rv32i-emulator` can opt into board MMIO by running through its `Machine`
wrapper with a target `MmioBus`. Normal memory-only APIs still trap on
out-of-bounds addresses; the `Machine` path routes word-width load/store
instructions outside RAM to `MmioBus`, which is enough for early blink-style
programs to write the generic GPIO set/clear registers and inspect the GPIO
trace.

The emulator's `board_blink` example loads the ESP32-C3/C5/C6 placeholder board
files plus the CH32V003 RV32E placeholder, resolves each board's `led` alias,
runs one shared blink program through `Machine`, and prints the resulting GPIO
trace:

```bash
cargo run --example board_blink
```

Boards opt into that shared demo when `generic_gpio_led_blink_binding` can
resolve a `led` alias, a `generic-gpio` MMIO device named `gpio`, and a GPIO pin
that fits the current 32-bit mask model. Contract tests cover these requirements
so future board TOML files fail with specific errors instead of silently falling
out of the shared demo path.

## Next Steps

- Add board-specific register maps where accuracy matters.
- Add reusable `.s` fixture files for board demos instead of generating source strings in examples.
