use sw_rv32i_target::{
    BoardError, GenericGpioMmio, GpioTraceEvent, MmioBus, generic_gpio_led_blink_binding,
    load_blink_demo_file, load_board_file, parse_board_toml,
};

const ESP32_C_SERIES_BOARDS: &[&str] = &[
    "boards/esp32-c3-devkitm-1.toml",
    "boards/esp32-c5-devkitc-1.toml",
    "boards/esp32-c6-devkitc-1.toml",
];

#[test]
fn loads_esp32_c_series_placeholder_boards() {
    for path in ESP32_C_SERIES_BOARDS {
        let board = load_board_file(path).unwrap();
        assert!(board.id.starts_with("esp32-c"));
        assert!(board.arch.starts_with("rv32"));
        assert_eq!(board.led_gpio(), Some(8));
        assert!(board.supports_gpio(8));
        assert_eq!(board.mmio["gpio"].kind, "generic-gpio");
    }
}

#[test]
fn blink_demo_can_be_shared_by_boards_with_same_led_gpio() {
    let c3 = load_board_file("boards/esp32-c3-devkitm-1.toml").unwrap();
    let c5 = load_board_file("boards/esp32-c5-devkitc-1.toml").unwrap();
    let c6 = load_board_file("boards/esp32-c6-devkitc-1.toml").unwrap();

    let shared_led = c3.led_gpio();
    assert_eq!(shared_led, Some(8));
    assert_eq!(c5.led_gpio(), shared_led);
    assert_eq!(c6.led_gpio(), shared_led);
}

#[test]
fn blink_demo_runs_against_shared_led_alias_on_esp32_c_series() {
    let demo = load_blink_demo_file("demos/blink.toml").unwrap();
    for path in ESP32_C_SERIES_BOARDS {
        let board = load_board_file(path).unwrap();
        let gpio = demo.run(&board).unwrap();

        assert_eq!(gpio.read_pin(8), Ok(false));
        assert_eq!(gpio.trace().len(), 6);
        assert_eq!(
            &gpio.trace()[..2],
            &[
                GpioTraceEvent { pin: 8, high: true },
                GpioTraceEvent {
                    pin: 8,
                    high: false,
                },
            ]
        );
    }
}

#[test]
fn blink_like_mmio_writes_toggle_shared_board_led() {
    for path in ESP32_C_SERIES_BOARDS {
        let board = load_board_file(path).unwrap();
        let binding = generic_gpio_led_blink_binding(&board).unwrap();
        let mut bus = MmioBus::for_board(&board).unwrap();

        bus.write32(
            binding.gpio_base + GenericGpioMmio::SET_OFFSET,
            binding.led_mask,
        )
        .unwrap();
        assert_eq!(
            bus.read32(binding.gpio_base + GenericGpioMmio::READ_OFFSET),
            Ok(binding.led_mask)
        );
        bus.write32(
            binding.gpio_base + GenericGpioMmio::CLEAR_OFFSET,
            binding.led_mask,
        )
        .unwrap();

        assert_eq!(
            bus.read32(binding.gpio_base + GenericGpioMmio::READ_OFFSET),
            Ok(0)
        );
        assert_eq!(
            bus.gpio().unwrap().trace(),
            &[
                GpioTraceEvent {
                    pin: binding.led_pin,
                    high: true,
                },
                GpioTraceEvent {
                    pin: binding.led_pin,
                    high: false,
                },
            ]
        );
    }
}

#[test]
fn every_generic_gpio_led_board_can_bind_shared_blink_contract() {
    for path in ESP32_C_SERIES_BOARDS {
        let board = load_board_file(path).unwrap();
        let binding = generic_gpio_led_blink_binding(&board).unwrap();

        assert_eq!(binding.led_pin, 8);
        assert_eq!(binding.led_mask, 0x0000_0100);
        assert_eq!(binding.gpio_base, 0x6000_4000);
        assert!(board.supports_gpio(binding.led_pin));
    }
}

#[test]
fn blink_contract_reports_missing_led_alias_clearly() {
    let board = parse_board_toml(
        r#"
        id = "custom-no-led"
        family = "custom"
        arch = "rv32i"

        [memory]
        ram_base = "0x00000000"
        ram_size = "0x00001000"

        [gpio]
        pins = [8]

        [aliases]

        [mmio.gpio]
        kind = "generic-gpio"
        base = "0x60004000"
        "#,
    )
    .unwrap();

    let err = generic_gpio_led_blink_binding(&board).unwrap_err();
    assert_eq!(err, BoardError::UnknownAlias("led".to_string()));
    assert_eq!(err.to_string(), "unknown alias: led");
}

#[test]
fn blink_contract_reports_missing_or_unsupported_gpio_mmio_clearly() {
    let missing = parse_board_toml(
        r#"
        id = "custom-no-mmio"
        family = "custom"
        arch = "rv32i"

        [memory]
        ram_base = "0x00000000"
        ram_size = "0x00001000"

        [gpio]
        pins = [8]

        [aliases]
        led = 8
        "#,
    )
    .unwrap();
    let missing_err = generic_gpio_led_blink_binding(&missing).unwrap_err();
    assert_eq!(
        missing_err,
        BoardError::MissingMmioDevice("gpio".to_string())
    );
    assert_eq!(missing_err.to_string(), "missing MMIO device: gpio");

    let unsupported = parse_board_toml(
        r#"
        id = "custom-real-gpio"
        family = "custom"
        arch = "rv32i"

        [memory]
        ram_base = "0x00000000"
        ram_size = "0x00001000"

        [gpio]
        pins = [8]

        [aliases]
        led = 8

        [mmio.gpio]
        kind = "vendor-gpio"
        base = "0x60004000"
        "#,
    )
    .unwrap();
    let unsupported_err = generic_gpio_led_blink_binding(&unsupported).unwrap_err();
    assert_eq!(
        unsupported_err,
        BoardError::UnsupportedMmioKind("vendor-gpio".to_string())
    );
    assert_eq!(
        unsupported_err.to_string(),
        "unsupported MMIO kind: vendor-gpio"
    );
}

#[test]
fn blink_contract_reports_led_pins_outside_initial_gpio_mask() {
    let board = parse_board_toml(
        r#"
        id = "custom-high-led"
        family = "custom"
        arch = "rv32i"

        [memory]
        ram_base = "0x00000000"
        ram_size = "0x00001000"

        [gpio]
        pins = [32]

        [aliases]
        led = 32

        [mmio.gpio]
        kind = "generic-gpio"
        base = "0x60004000"
        "#,
    )
    .unwrap();

    let err = generic_gpio_led_blink_binding(&board).unwrap_err();
    assert_eq!(err, BoardError::GpioMaskOutOfRange(32));
    assert_eq!(err.to_string(), "GPIO pin cannot fit in 32-bit mask: 32");
}

#[test]
fn blink_contract_reports_led_alias_that_is_not_a_gpio_pin() {
    let board = parse_board_toml(
        r#"
        id = "custom-unknown-led"
        family = "custom"
        arch = "rv32i"

        [memory]
        ram_base = "0x00000000"
        ram_size = "0x00001000"

        [gpio]
        pins = [8]

        [aliases]
        led = 9

        [mmio.gpio]
        kind = "generic-gpio"
        base = "0x60004000"
        "#,
    )
    .unwrap();

    let err = generic_gpio_led_blink_binding(&board).unwrap_err();
    assert_eq!(err, BoardError::UnknownGpioPin(9));
    assert_eq!(err.to_string(), "unknown GPIO pin: 9");
}
