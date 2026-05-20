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
```

Blink demos should bind to logical aliases such as `led`, not directly to a
board id. Boards that map `led` to the same GPIO pin can share the same resolved
demo configuration; boards with different pins can reuse the same demo source
with different board bindings.

The first board files are placeholders for ESP32-C3, ESP32-C5, and ESP32-C6
development boards. They intentionally use a generic GPIO MMIO kind until a
register-accurate device model is added.

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

## Next Steps

- Connect the generic GPIO trace model to an emulator MMIO bus.
- Add board-specific register maps where accuracy matters.
