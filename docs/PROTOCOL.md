# AM Infinity settings protocol

Everything the app sends to the mouse, and where each value was learned.
This information was worked out only to make the mouse configurable from Linux
(interoperability); no AngryMiao code is included in this project.

Sources:
- **AM35model**: the `AM35model.FunctionArea` module inside AngryMiao's Windows app AM Master 1.3.6.
- **WebPage**: the `WebPage.AM35Area` module in the same app.
- **hid.pyd**: the HID helper library that ships with AM Master.
- **Web page**: AngryMiao's online configurator that AM Master shows in its window
  (components `CMouseAm35Dpi`, `CMouseAm35Param` and the key list `c3f0`).
- **Tested**: checked against a real AM Infinity (firmware v1.7.6) on Linux.

## Connection

| What | Value | Source |
|---|---|---|
| USB vendor | 0x0E8D | AM35model `FunctionArea.__init__` (VID 3725) |
| USB product reached "through the receiver" | 0x0703: every command uses type byte **0x80** | AM35model `_ensure_connection_manager` (1795 when `AM35_D_CON`) + `remote_checker` (`AM35_D_CON == 1`); **tested**: with type 0 the DPI, rate and button questions get no reply, with 0x80 all answer |
| USB product reached directly | 0x0880: type byte 0 | AM35model `_ensure_connection_manager` (2176 when `AM35_CON`) |
| HID interface | usage page 0xFF13, usage 1 | AM35model `ConnectionManager._get_device_path` |
| Command report | id 6, 62 bytes: `06, len, type, frame…` | AM35model `send_trigger_command` |
| Zero padding | a shorter command is copied and zero-filled up to the device's output report length before it is written | hid.pyd `hid_write` |
| Type byte | 0x00 for the receiver itself, 0x80 for the mouse behind it | AM35model `send_trigger_command` (remote → 128) |
| Reply frame kinds | `05 5B` (answer) or `05 5D` (notification); both carry the command id at bytes 7..8, and several frames can share one report | tested: the DPI read answers `05 5D 25 00 C5 30 …`, the version `05 5B …` then `05 5D … "v1.7.6"`, a save `05 5D 03 00 10 30 00` |
| Reply report | id 7, 62 bytes, fetched only with GET_REPORT: byte 0 is set to 7 and IOCTL **0xB01A2** (IOCTL_HID_GET_INPUT_REPORT) fills the buffer, report id kept at byte 0. Linux equivalent: HIDIOCGINPUT | hid.pyd `get_input_report` |
| Empty reply | byte 1 = 0 means "nothing yet" | AM35model `ConnectionManager.read` |
| Wait before each fetch | 50 ms | AM35model `get_race_response` / `ConnectionManager.read` default `timeout_ms = 50` |
| Give up after | 5 s | AM35model `FunctionArea.DEFAULT_TIMEOUT = 5` |
| Old replies cleared first | up to 10 | AM35model `_clear_old_responses(max_clear = 10)` |
| Retry a read that got no answer | up to 4 tries, waiting 0.08 s, then ×1.5 each time | AM35model `retry_on_fail(4, 0.08, 1.5)`; a setting that still can't be read is reported and the controls stay locked (tested: one debounce read went unanswered once in five) |
| Gap between packets of a long command | 1 ms | AM35model `PACKET_INTERVAL = 0.001` (every command here fits one packet) |
| Frame | `05 5A len_lo len_hi cmd_lo cmd_hi data…`, len = 2 + data length | every AM35model setter |
| Reply match | reply bytes 7..8 = command bytes 4..5; data starts at byte 10 | AM35model `on_send` |

## Resolution (DPI)

| What | Value | Source |
|---|---|---|
| Levels | 1 to 8 | web page `CMouseAm35Dpi` slider (min 1, max 8) |
| DPI range | 50 to 30000 in steps of 50 | min and step: web page `CMouseAm35Dpi`; the page stops at 26000, but the mouse itself stores 30000 as level 8, so 30000 is used |
| Default levels | 800, 1600, 2400, 3200, 5000, 10000, 20000, 26000 | web page `mouseAM35` store defaults |
| Read all | `C5 30` → [10]=levels, [11]=active, [12..28]=X ×8, [28..44]=Y ×8 (u16 little-endian) | AM35model `get_dpi_data` |
| Set one level | `C4 30 level xlo xhi ylo yhi` | AM35model `set_dpixy` |
| Set active level | `C7 30 level` | AM35model `set_dpi_c` |
| Set number of levels | `C6 30 n` | AM35model `set_dpi_loop` |
| DPI quick switch (side buttons + wheel) | `EA 30 18` on, `EA 30 00` off; read `EB 30` ([10]==0x18 → on) | AM35model `set_dpi_button` / `get_dpi_button`, WebPage `AM35Area.set_dpi_button` |
| Same X and Y | on when every X equals its Y | WebPage `AM35Area.get_dpi` (DPIsyn) |

## Buttons

| What | Value | Source |
|---|---|---|
| Buttons | 1 Left, 2 Right, 3 Wheel click, 4 Forward, 5 Back, 6 Wheel up, 7 Wheel down | web page key list `c3f0` (`0x01`…`0x07`) |
| Read one | `C3 30 n` → [10]=button, [11]=type; type 6: [12..14] media code, [14] key code, [15] modifiers; type 11: [12] action, [13..15] target | AM35model `get_key` |
| Simple assignment | `C2 30 n type` | AM35model `change_key` |
| Types | 1 Left, 2 Right, 3 Middle, 4 Forward, 5 Back, 12 Wheel up, 13 Wheel down, 11 DPI, 14 Profile switch, 6 key/media, 8 macro | web page `c3f0` lists `c` and `a` |
| DPI actions | 0 DPI +, 1 DPI −, 3 DPI cycle | web page `c3f0` list `a` |
| DPI assignment | `C2 30 n 0B action 00 00` (length 07) | layout from AM35model `get_key` ([12] action, [13..15] target); AM Master 1.3.6 itself sends only `C2 30 n 0B`. **Tested**: button 7 set to DPI − answered status 00 and read back `0B 01 00 00`, then restored to wheel down (type 13) |
| Media key | `C2 30 n 06 code_lo code_hi 00 mods 00` | AM35model `change_key` (consumer key, little-endian, 3 bytes) |
| Keyboard key | `C2 30 n 06 00 00 usage mods 00` | AM35model `change_key` (standard key, big-endian, 3 bytes) |
| Modifier bits | Ctrl 1, Shift 2, Alt 4, Super 8 | AM35model `change_key` BIT_WEIGHTS + web page `comboCtrl/Shift/Alt/Win` |
| Media codes | Play/Pause 0xCD, Stop 0xB7, Previous 0xB5, Next 0xB6, Mute 0xE2, Volume + 0xE9, Volume − 0xEA, Calculator 0x192, Email 0x18A, My computer 0x194, Refresh 0x227, Search 0x221, Home 0x223, Brightness + 0x6F, Brightness − 0x70, Media player 0x183 | web page `c3f0` list `a` |
| Macros | shown when the mouse has one, not editable here | AM35model `change_key` type 8 (left out of this version) |

## Performance

| What | Value | Source |
|---|---|---|
| Polling rate | 125, 250, 500, 1000, 2000, 4000, 8000 Hz; `10 30 lo hi` (Hz, little-endian); read `11 30` [10..12] | web page `CMouseAm35Param`; AM35model `set_report_rate` / `get_report_rate` |
| Default polling rate | 8000 Hz | web page `mouseAM35` store |
| Debounce | 7 steps; cable/2.4G value depends on the polling rate: 125–1000 Hz → 10,8,6,4,3,2,0; 2000 → 20,16,12,8,4,2,0; 4000 → 39,31,23,15,7,3,0; 8000 → 79,63,47,31,15,7,0; Bluetooth 10,8,6,4,3,2,0 | web page `wireDebounceGearsMap`, `rf24gDebounceGearsMap`, `btDebounceGears` |
| Debounce step labels | 10, 8, 6, 4, 3, 2, 0 ms | derived from the Bluetooth / 1000 Hz row (one tick = 1 ms at 1000 Hz) |
| Debounce command | `E8 30 cable 2.4G bluetooth`; read `E9 30` [10],[11],[12] | AM35model `set_debounce`, `get_sensor_config` |
| Lift-off distance | 0 low, 1 middle, 2 high; `CC 30 v`; read `CD 30` [10] | web page radio values; AM35model `set_lod` |
| Motion sync | 1 on, 0 off; `D2 30 v`; read `D3 30` | AM35model `set_motion_sync` |
| Angle snapping | 1 on, 0 off; `D4 30 v`; read `D5 30` | AM35model `set_angle_snapping` |
| Ripple control | 1 on, 0 off; `D6 30 v`; read `D7 30` | AM35model `set_ripple_ctrl` |
| Game (FPS) mode | 1 on, 0 off; `DE 30 v`; read `DF 30` [9] | AM35model `set_fps` / `get_fps` |
| Factory reset | `CE 30` | AM35model `restore` |
| Firmware version | `07 1C 00` → text from byte 9 | AM35model `get_version` |

## Left out of this version
Lighting colours, macros editing, wireless sleep timers, firmware updates and the 2.4G
dongle path (the code is ready for the dongle's product id, but only the cable was seen).
