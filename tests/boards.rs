use sw_rv32i_target::{GpioTraceEvent, load_blink_demo_file, load_board_file};

#[test]
fn loads_esp32_c_series_placeholder_boards() {
    for path in [
        "boards/esp32-c3-devkitm-1.toml",
        "boards/esp32-c5-devkitc-1.toml",
        "boards/esp32-c6-devkitc-1.toml",
    ] {
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
    for path in [
        "boards/esp32-c3-devkitm-1.toml",
        "boards/esp32-c5-devkitc-1.toml",
        "boards/esp32-c6-devkitc-1.toml",
    ] {
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
