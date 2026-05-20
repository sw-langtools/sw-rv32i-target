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
pub enum BoardError {
    Io(String),
    Parse(String),
    MissingField(String),
    InvalidNumber(String),
    InvalidArray(String),
}

impl fmt::Display for BoardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BoardError::Io(message) => write!(f, "I/O error: {message}"),
            BoardError::Parse(message) => write!(f, "parse error: {message}"),
            BoardError::MissingField(field) => write!(f, "missing field: {field}"),
            BoardError::InvalidNumber(value) => write!(f, "invalid number: {value}"),
            BoardError::InvalidArray(value) => write!(f, "invalid array: {value}"),
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
}
