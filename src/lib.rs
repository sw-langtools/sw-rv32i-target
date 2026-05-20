//! Board and target metadata for RISC-V demos.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::Path;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Board {
    pub id: String,
    pub family: String,
    pub arch: String,
    pub memory: MemoryMap,
    pub gpio: Gpio,
    pub aliases: BTreeMap<String, u32>,
    pub mmio: BTreeMap<String, MmioDevice>,
}

impl Board {
    pub fn led_gpio(&self) -> Option<u32> {
        self.aliases.get("led").copied()
    }

    pub fn alias_gpio(&self, alias: &str) -> Option<u32> {
        self.aliases.get(alias).copied()
    }

    pub fn supports_gpio(&self, pin: u32) -> bool {
        self.gpio.pins.contains(&pin)
    }

    pub fn mmio_base(&self, name: &str) -> Option<u32> {
        self.mmio.get(name).map(|device| device.base)
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct GenericGpioBlinkBinding {
    pub gpio_base: u32,
    pub led_pin: u32,
    pub led_mask: u32,
}

pub fn generic_gpio_led_blink_binding(
    board: &Board,
) -> Result<GenericGpioBlinkBinding, BoardError> {
    let led_pin = board
        .led_gpio()
        .ok_or_else(|| BoardError::UnknownAlias("led".to_string()))?;
    let led_mask = 1u32
        .checked_shl(led_pin)
        .ok_or(BoardError::GpioMaskOutOfRange(led_pin))?;
    if !board.supports_gpio(led_pin) {
        return Err(BoardError::UnknownGpioPin(led_pin));
    }
    let gpio = board
        .mmio
        .get("gpio")
        .ok_or_else(|| BoardError::MissingMmioDevice("gpio".to_string()))?;
    if gpio.kind != "generic-gpio" {
        return Err(BoardError::UnsupportedMmioKind(gpio.kind.clone()));
    }
    Ok(GenericGpioBlinkBinding {
        gpio_base: gpio.base,
        led_pin,
        led_mask,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemoryMap {
    pub ram_base: u32,
    pub ram_size: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Gpio {
    pub pins: Vec<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MmioDevice {
    pub kind: String,
    pub base: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GpioState {
    pins: BTreeMap<u32, bool>,
    trace: Vec<GpioTraceEvent>,
}

impl GpioState {
    pub fn for_board(board: &Board) -> Self {
        let pins = board
            .gpio
            .pins
            .iter()
            .copied()
            .map(|pin| (pin, false))
            .collect();
        Self {
            pins,
            trace: Vec::new(),
        }
    }

    pub fn write_pin(&mut self, pin: u32, high: bool) -> Result<(), BoardError> {
        let state = self
            .pins
            .get_mut(&pin)
            .ok_or(BoardError::UnknownGpioPin(pin))?;
        *state = high;
        self.trace.push(GpioTraceEvent { pin, high });
        Ok(())
    }

    pub fn write_alias(
        &mut self,
        board: &Board,
        alias: &str,
        high: bool,
    ) -> Result<(), BoardError> {
        let pin = board
            .alias_gpio(alias)
            .ok_or_else(|| BoardError::UnknownAlias(alias.to_string()))?;
        self.write_pin(pin, high)
    }

    pub fn read_pin(&self, pin: u32) -> Result<bool, BoardError> {
        self.pins
            .get(&pin)
            .copied()
            .ok_or(BoardError::UnknownGpioPin(pin))
    }

    pub fn trace(&self) -> &[GpioTraceEvent] {
        &self.trace
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MmioBus {
    gpio: Option<GenericGpioMmio>,
}

impl MmioBus {
    pub fn for_board(board: &Board) -> Result<Self, BoardError> {
        let gpio = match board.mmio.get("gpio") {
            Some(device) if device.kind == "generic-gpio" => Some(GenericGpioMmio::new(
                device.base,
                GpioState::for_board(board),
            )),
            Some(device) => return Err(BoardError::UnsupportedMmioKind(device.kind.clone())),
            None => None,
        };
        Ok(Self { gpio })
    }

    pub fn write32(&mut self, addr: u32, value: u32) -> Result<(), BoardError> {
        if let Some(gpio) = &mut self.gpio {
            if gpio.contains(addr) {
                return gpio.write32(addr, value);
            }
        }
        Err(BoardError::UnknownMmioAddress(addr))
    }

    pub fn read32(&self, addr: u32) -> Result<u32, BoardError> {
        if let Some(gpio) = &self.gpio {
            if gpio.contains(addr) {
                return gpio.read32(addr);
            }
        }
        Err(BoardError::UnknownMmioAddress(addr))
    }

    pub fn gpio(&self) -> Option<&GpioState> {
        self.gpio.as_ref().map(GenericGpioMmio::state)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GenericGpioMmio {
    base: u32,
    state: GpioState,
}

impl GenericGpioMmio {
    pub const SET_OFFSET: u32 = 0x00;
    pub const CLEAR_OFFSET: u32 = 0x04;
    pub const READ_OFFSET: u32 = 0x08;
    pub const SIZE: u32 = 0x0c;

    pub fn new(base: u32, state: GpioState) -> Self {
        Self { base, state }
    }

    pub fn contains(&self, addr: u32) -> bool {
        addr >= self.base && addr < self.base + Self::SIZE
    }

    pub fn state(&self) -> &GpioState {
        &self.state
    }

    pub fn write32(&mut self, addr: u32, value: u32) -> Result<(), BoardError> {
        match addr.checked_sub(self.base) {
            Some(Self::SET_OFFSET) => self.write_mask(value, true),
            Some(Self::CLEAR_OFFSET) => self.write_mask(value, false),
            _ => Err(BoardError::UnknownMmioAddress(addr)),
        }
    }

    pub fn read32(&self, addr: u32) -> Result<u32, BoardError> {
        match addr.checked_sub(self.base) {
            Some(Self::READ_OFFSET) => {
                let mut value = 0;
                for (pin, high) in &self.state.pins {
                    if *high && *pin < 32 {
                        value |= 1 << pin;
                    }
                }
                Ok(value)
            }
            _ => Err(BoardError::UnknownMmioAddress(addr)),
        }
    }

    fn write_mask(&mut self, value: u32, high: bool) -> Result<(), BoardError> {
        for pin in 0..32 {
            if value & (1 << pin) != 0 && self.state.pins.contains_key(&pin) {
                self.state.write_pin(pin, high)?;
            }
        }
        Ok(())
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct GpioTraceEvent {
    pub pin: u32,
    pub high: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlinkDemo {
    pub id: String,
    pub signal: String,
    pub cycles: u32,
}

impl BlinkDemo {
    pub fn run(&self, board: &Board) -> Result<GpioState, BoardError> {
        let mut gpio = GpioState::for_board(board);
        for _ in 0..self.cycles {
            gpio.write_alias(board, &self.signal, true)?;
            gpio.write_alias(board, &self.signal, false)?;
        }
        Ok(gpio)
    }
}

pub fn load_blink_demo_file(path: impl AsRef<Path>) -> Result<BlinkDemo, BoardError> {
    let text = fs::read_to_string(path).map_err(|err| BoardError::Io(err.to_string()))?;
    parse_blink_demo_toml(&text)
}

pub fn parse_blink_demo_toml(text: &str) -> Result<BlinkDemo, BoardError> {
    let mut id = None;
    let mut signal = None;
    let mut cycles = None;
    for (index, raw_line) in text.lines().enumerate() {
        let line_no = index + 1;
        let line = strip_comment(raw_line).trim();
        if line.is_empty() {
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| BoardError::Parse(format!("line {line_no}: expected key = value")))?;
        match key.trim() {
            "id" => id = Some(parse_string(value.trim(), line_no)?),
            "signal" => signal = Some(parse_string(value.trim(), line_no)?),
            "cycles" => cycles = Some(parse_number(value.trim())?),
            other => {
                return Err(BoardError::Parse(format!(
                    "line {line_no}: unknown blink key {other}"
                )));
            }
        }
    }
    Ok(BlinkDemo {
        id: id.ok_or_else(|| BoardError::MissingField("id".to_string()))?,
        signal: signal.ok_or_else(|| BoardError::MissingField("signal".to_string()))?,
        cycles: cycles.ok_or_else(|| BoardError::MissingField("cycles".to_string()))?,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BoardError {
    Io(String),
    Parse(String),
    MissingField(String),
    InvalidNumber(String),
    InvalidArray(String),
    UnknownAlias(String),
    UnknownGpioPin(u32),
    MissingMmioDevice(String),
    UnknownMmioAddress(u32),
    UnsupportedMmioKind(String),
    GpioMaskOutOfRange(u32),
}

impl fmt::Display for BoardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BoardError::Io(message) => write!(f, "I/O error: {message}"),
            BoardError::Parse(message) => write!(f, "parse error: {message}"),
            BoardError::MissingField(field) => write!(f, "missing field: {field}"),
            BoardError::InvalidNumber(value) => write!(f, "invalid number: {value}"),
            BoardError::InvalidArray(value) => write!(f, "invalid array: {value}"),
            BoardError::UnknownAlias(alias) => write!(f, "unknown alias: {alias}"),
            BoardError::UnknownGpioPin(pin) => write!(f, "unknown GPIO pin: {pin}"),
            BoardError::MissingMmioDevice(name) => write!(f, "missing MMIO device: {name}"),
            BoardError::UnknownMmioAddress(addr) => write!(f, "unknown MMIO address: 0x{addr:08x}"),
            BoardError::UnsupportedMmioKind(kind) => write!(f, "unsupported MMIO kind: {kind}"),
            BoardError::GpioMaskOutOfRange(pin) => {
                write!(f, "GPIO pin cannot fit in 32-bit mask: {pin}")
            }
        }
    }
}

impl std::error::Error for BoardError {}

pub fn load_board_file(path: impl AsRef<Path>) -> Result<Board, BoardError> {
    let text = fs::read_to_string(path).map_err(|err| BoardError::Io(err.to_string()))?;
    parse_board_toml(&text)
}

pub fn parse_board_toml(text: &str) -> Result<Board, BoardError> {
    let mut parser = BoardParser::default();
    parser.parse(text)?;
    parser.finish()
}

#[derive(Default)]
struct BoardParser {
    section: Section,
    id: Option<String>,
    family: Option<String>,
    arch: Option<String>,
    ram_base: Option<u32>,
    ram_size: Option<u32>,
    gpio_pins: Option<Vec<u32>>,
    aliases: BTreeMap<String, u32>,
    mmio: BTreeMap<String, MmioDevice>,
}

impl BoardParser {
    fn parse(&mut self, text: &str) -> Result<(), BoardError> {
        for (index, raw_line) in text.lines().enumerate() {
            let line_no = index + 1;
            let line = strip_comment(raw_line).trim();
            if line.is_empty() {
                continue;
            }
            if line.starts_with('[') {
                self.section = parse_section(line, line_no)?;
                continue;
            }
            let (key, value) = line.split_once('=').ok_or_else(|| {
                BoardError::Parse(format!("line {line_no}: expected key = value"))
            })?;
            self.set_value(key.trim(), value.trim(), line_no)?;
        }
        Ok(())
    }

    fn set_value(&mut self, key: &str, value: &str, line_no: usize) -> Result<(), BoardError> {
        match &self.section {
            Section::Root => match key {
                "id" => self.id = Some(parse_string(value, line_no)?),
                "family" => self.family = Some(parse_string(value, line_no)?),
                "arch" => self.arch = Some(parse_string(value, line_no)?),
                _ => {
                    return Err(BoardError::Parse(format!(
                        "line {line_no}: unknown root key {key}"
                    )));
                }
            },
            Section::Memory => match key {
                "ram_base" => self.ram_base = Some(parse_number(value)?),
                "ram_size" => self.ram_size = Some(parse_number(value)?),
                _ => {
                    return Err(BoardError::Parse(format!(
                        "line {line_no}: unknown memory key {key}"
                    )));
                }
            },
            Section::Gpio => match key {
                "pins" => self.gpio_pins = Some(parse_number_array(value)?),
                _ => {
                    return Err(BoardError::Parse(format!(
                        "line {line_no}: unknown gpio key {key}"
                    )));
                }
            },
            Section::Aliases => {
                self.aliases.insert(key.to_string(), parse_number(value)?);
            }
            Section::Mmio(name) => {
                let entry = self.mmio.entry(name.clone()).or_insert_with(|| MmioDevice {
                    kind: String::new(),
                    base: 0,
                });
                match key {
                    "kind" => entry.kind = parse_string(value, line_no)?,
                    "base" => entry.base = parse_number(value)?,
                    _ => {
                        return Err(BoardError::Parse(format!(
                            "line {line_no}: unknown mmio key {key}"
                        )));
                    }
                }
            }
        }
        Ok(())
    }

    fn finish(self) -> Result<Board, BoardError> {
        let board = Board {
            id: self
                .id
                .ok_or_else(|| BoardError::MissingField("id".to_string()))?,
            family: self
                .family
                .ok_or_else(|| BoardError::MissingField("family".to_string()))?,
            arch: self
                .arch
                .ok_or_else(|| BoardError::MissingField("arch".to_string()))?,
            memory: MemoryMap {
                ram_base: self
                    .ram_base
                    .ok_or_else(|| BoardError::MissingField("memory.ram_base".to_string()))?,
                ram_size: self
                    .ram_size
                    .ok_or_else(|| BoardError::MissingField("memory.ram_size".to_string()))?,
            },
            gpio: Gpio {
                pins: self
                    .gpio_pins
                    .ok_or_else(|| BoardError::MissingField("gpio.pins".to_string()))?,
            },
            aliases: self.aliases,
            mmio: self.mmio,
        };
        for (name, device) in &board.mmio {
            if device.kind.is_empty() {
                return Err(BoardError::MissingField(format!("mmio.{name}.kind")));
            }
        }
        Ok(board)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
enum Section {
    #[default]
    Root,
    Memory,
    Gpio,
    Aliases,
    Mmio(String),
}

fn parse_section(line: &str, line_no: usize) -> Result<Section, BoardError> {
    let name = line
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .ok_or_else(|| BoardError::Parse(format!("line {line_no}: malformed section")))?;
    match name {
        "memory" => Ok(Section::Memory),
        "gpio" => Ok(Section::Gpio),
        "aliases" => Ok(Section::Aliases),
        _ if name.starts_with("mmio.") => Ok(Section::Mmio(name["mmio.".len()..].to_string())),
        _ => Err(BoardError::Parse(format!(
            "line {line_no}: unknown section {name}"
        ))),
    }
}

fn strip_comment(line: &str) -> &str {
    line.split_once('#').map_or(line, |(before, _)| before)
}

fn parse_string(value: &str, line_no: usize) -> Result<String, BoardError> {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .map(str::to_string)
        .ok_or_else(|| BoardError::Parse(format!("line {line_no}: expected quoted string")))
}

fn parse_number(value: &str) -> Result<u32, BoardError> {
    let value = value.trim().trim_matches('"');
    if let Some(hex) = value.strip_prefix("0x") {
        u32::from_str_radix(hex, 16).map_err(|_| BoardError::InvalidNumber(value.to_string()))
    } else {
        value
            .parse::<u32>()
            .map_err(|_| BoardError::InvalidNumber(value.to_string()))
    }
}

fn parse_number_array(value: &str) -> Result<Vec<u32>, BoardError> {
    let inner = value
        .trim()
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .ok_or_else(|| BoardError::InvalidArray(value.to_string()))?;
    if inner.trim().is_empty() {
        return Ok(Vec::new());
    }
    inner.split(',').map(parse_number).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_board_toml_with_gpio_alias_and_mmio() {
        let board = parse_board_toml(
            r#"
            id = "demo"
            family = "demo-family"
            arch = "rv32imc"

            [memory]
            ram_base = "0x3fc80000"
            ram_size = "0x00060000"

            [gpio]
            pins = [0, 1, 8]

            [aliases]
            led = 8

            [mmio.gpio]
            kind = "generic-gpio"
            base = "0x60004000"
            "#,
        )
        .unwrap();

        assert_eq!(board.id, "demo");
        assert_eq!(board.arch, "rv32imc");
        assert_eq!(board.memory.ram_base, 0x3fc8_0000);
        assert_eq!(board.led_gpio(), Some(8));
        assert!(board.supports_gpio(8));
        assert_eq!(board.mmio["gpio"].kind, "generic-gpio");
    }

    #[test]
    fn rejects_missing_required_fields() {
        assert_eq!(
            parse_board_toml("id = \"demo\"").unwrap_err(),
            BoardError::MissingField("family".to_string())
        );
    }

    #[test]
    fn gpio_state_toggles_aliases_and_records_trace() {
        let board = parse_board_toml(
            r#"
            id = "demo"
            family = "demo-family"
            arch = "rv32imc"
            [memory]
            ram_base = "0x0"
            ram_size = "0x100"
            [gpio]
            pins = [8]
            [aliases]
            led = 8
            "#,
        )
        .unwrap();
        let demo = BlinkDemo {
            id: "blink".to_string(),
            signal: "led".to_string(),
            cycles: 2,
        };

        let gpio = demo.run(&board).unwrap();
        assert_eq!(gpio.read_pin(8), Ok(false));
        assert_eq!(
            gpio.trace(),
            &[
                GpioTraceEvent { pin: 8, high: true },
                GpioTraceEvent {
                    pin: 8,
                    high: false
                },
                GpioTraceEvent { pin: 8, high: true },
                GpioTraceEvent {
                    pin: 8,
                    high: false
                },
            ]
        );
    }

    #[test]
    fn blink_demo_toml_parses_logical_signal() {
        assert_eq!(
            parse_blink_demo_toml(
                r#"
                id = "blink"
                signal = "led"
                cycles = 3
                "#
            ),
            Ok(BlinkDemo {
                id: "blink".to_string(),
                signal: "led".to_string(),
                cycles: 3,
            })
        );
    }

    #[test]
    fn generic_gpio_mmio_records_set_and_clear_writes() {
        let board = parse_board_toml(
            r#"
            id = "demo"
            family = "demo-family"
            arch = "rv32imc"
            [memory]
            ram_base = "0x0"
            ram_size = "0x100"
            [gpio]
            pins = [8]
            [aliases]
            led = 8
            [mmio.gpio]
            kind = "generic-gpio"
            base = "0x1000"
            "#,
        )
        .unwrap();
        let mut bus = MmioBus::for_board(&board).unwrap();

        bus.write32(0x1000 + GenericGpioMmio::SET_OFFSET, 1 << 8)
            .unwrap();
        assert_eq!(
            bus.read32(0x1000 + GenericGpioMmio::READ_OFFSET),
            Ok(1 << 8)
        );
        bus.write32(0x1000 + GenericGpioMmio::CLEAR_OFFSET, 1 << 8)
            .unwrap();
        assert_eq!(bus.read32(0x1000 + GenericGpioMmio::READ_OFFSET), Ok(0));
        assert_eq!(
            bus.gpio().unwrap().trace(),
            &[
                GpioTraceEvent { pin: 8, high: true },
                GpioTraceEvent {
                    pin: 8,
                    high: false
                },
            ]
        );
    }
}
