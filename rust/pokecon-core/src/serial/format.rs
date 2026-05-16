use crate::serial::keys::{
    Button, CONVERT_HAT_3DS_CONTROLLER, CONVERT_HAT_DEFAULT, DIRECTION_CENTER, Direction, Hat,
    Stick, Tilt, Touchscreen,
};

/// Serial data format state (button, hat, sticks, touchscreen).
/// This struct is used to build up a frame and convert it to various serial protocols.
#[derive(Debug, Clone, Default)]
pub struct SendFormat {
    pub btn: u16,
    pub hat: u8,
    pub lx: u8,
    pub ly: u8,
    pub rx: u8,
    pub ry: u8,
    pub sx: u16,
    pub sy: u8,
}

impl SendFormat {
    pub fn new() -> Self {
        Self {
            btn: 0,
            hat: Hat::CENTER as u8,
            lx: DIRECTION_CENTER,
            ly: DIRECTION_CENTER,
            rx: DIRECTION_CENTER,
            ry: DIRECTION_CENTER,
            sx: 0,
            sy: 0,
        }
    }

    pub fn set_button(&mut self, btns: &[Button]) {
        for &btn in btns {
            self.btn |= btn.bits();
        }
    }

    pub fn unset_button(&mut self, btns: &[Button]) {
        for &btn in btns {
            self.btn &= !btn.bits();
        }
    }

    pub fn set_button_3ds_bits(&mut self, bits_3ds: u16) {
        self.btn |= bits_3ds;
    }

    pub fn unset_button_3ds_bits(&mut self, bits_3ds: u16) {
        self.btn &= !bits_3ds;
    }

    pub fn reset_all_buttons(&mut self) {
        self.btn = 0;
    }

    pub fn set_hat(&mut self, hats: &[Hat]) {
        if let Some(&hat) = hats.first() {
            self.hat = CONVERT_HAT_DEFAULT[hat as usize];
        }
    }

    pub fn unset_hat(&mut self) {
        self.hat = CONVERT_HAT_DEFAULT[Hat::CENTER as usize];
    }

    pub fn set_any_direction(&mut self, dirs: &[Direction]) {
        for d in dirs {
            match d.stick {
                Stick::Left => {
                    self.lx = d.x;
                    self.ly = 255 - d.y;
                }
                Stick::Right => {
                    self.rx = d.x;
                    self.ry = 255 - d.y;
                }
            }
        }
    }

    pub fn unset_direction(&mut self, tilts: &[Tilt]) {
        for tilt in tilts {
            match tilt {
                Tilt::Up | Tilt::Down => {
                    self.ly = DIRECTION_CENTER;
                    self.lx = Self::fix_other_axis(self.lx);
                }
                Tilt::Left | Tilt::Right => {
                    self.lx = DIRECTION_CENTER;
                    self.ly = Self::fix_other_axis(self.ly);
                }
                Tilt::RUp | Tilt::RDown => {
                    self.ry = DIRECTION_CENTER;
                    self.rx = Self::fix_other_axis(self.rx);
                }
                Tilt::RLeft | Tilt::RRight => {
                    self.rx = DIRECTION_CENTER;
                    self.ry = Self::fix_other_axis(self.ry);
                }
            }
        }
    }

    fn fix_other_axis(fix_target: u8) -> u8 {
        if fix_target == DIRECTION_CENTER {
            DIRECTION_CENTER
        } else if fix_target < DIRECTION_CENTER {
            0
        } else {
            255
        }
    }

    pub fn reset_all_directions(&mut self) {
        self.lx = DIRECTION_CENTER;
        self.ly = DIRECTION_CENTER;
        self.rx = DIRECTION_CENTER;
        self.ry = DIRECTION_CENTER;
    }

    pub fn set_touchscreen(&mut self, touchscreens: &[Touchscreen]) {
        if let Some(ts) = touchscreens.first() {
            self.sx = ts.x;
            self.sy = ts.y;
        }
    }

    pub fn unset_touchscreen(&mut self) {
        self.sx = 0;
        self.sy = 0;
    }

    /// Convert to Default format ASCII string:
    /// `0xBBBB H [LX LY] [RX RY]`
    /// `l_stick_changed` and `r_stick_changed` control whether stick data is included.
    pub fn convert_to_default(&self, l_stick_changed: bool, r_stick_changed: bool) -> String {
        let mut send_btn = (self.btn as u32) << 2;

        if l_stick_changed {
            send_btn |= 0x2;
        }
        if r_stick_changed {
            send_btn |= 0x1;
        }

        let mut result = format!("{:#08x}", send_btn);

        result.push(' ');
        result.push_str(&self.hat.to_string());

        if l_stick_changed {
            result.push_str(&format!(" {:x} {:x}", self.lx, self.ly));
        }
        if r_stick_changed {
            result.push_str(&format!(" {:x} {:x}", self.rx, self.ry));
        }

        result
    }

    /// Convert to Qingpi binary format (11 bytes fixed):
    /// `[0xAB, btn_lo, btn_hi, hat, lx, ly, 0x80, 0x80, sx_lo, sx_hi, sy]`
    pub fn convert_to_qingpi(&self) -> [u8; 11] {
        [
            0xAB,
            (self.btn & 0xFF) as u8,
            ((self.btn >> 8) & 0xFF) as u8,
            self.hat,
            self.lx,
            self.ly,
            DIRECTION_CENTER,
            DIRECTION_CENTER,
            (self.sx & 0xFF) as u8,
            ((self.sx >> 8) & 0xFF) as u8,
            self.sy,
        ]
    }

    /// Convert to 3DS Controller binary format (6 bytes fixed):
    /// `[0xA1, byte1, byte2, 0xA2, lx, ly]`
    pub fn convert_to_3ds(&self) -> [u8; 6] {
        let send_btn = self.btn as u32;
        let send_hat = CONVERT_HAT_3DS_CONTROLLER[self.hat as usize];

        let send_lx = if self.lx >= 128 {
            self.lx
        } else {
            127 - self.lx
        };
        let send_ly = if self.ly >= 128 {
            self.ly
        } else {
            127 - self.ly
        };

        [
            0xA1,
            (((send_btn & 0xF) << 4) | send_hat as u32) as u8,
            ((send_btn >> 4) & 0x3F) as u8,
            0xA2,
            send_lx,
            send_ly,
        ]
    }

    pub fn reset(&mut self) {
        self.btn = 0;
        self.hat = Hat::CENTER as u8;
        self.lx = DIRECTION_CENTER;
        self.ly = DIRECTION_CENTER;
        self.rx = DIRECTION_CENTER;
        self.ry = DIRECTION_CENTER;
        self.sx = 0;
        self.sy = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_defaults() {
        let fmt = SendFormat::new();
        assert_eq!(fmt.btn, 0);
        assert_eq!(fmt.hat, Hat::CENTER as u8);
        assert_eq!(fmt.lx, DIRECTION_CENTER);
        assert_eq!(fmt.ly, DIRECTION_CENTER);
        assert_eq!(fmt.rx, DIRECTION_CENTER);
        assert_eq!(fmt.ry, DIRECTION_CENTER);
        assert_eq!(fmt.sx, 0);
        assert_eq!(fmt.sy, 0);
    }

    #[test]
    fn test_set_button_single() {
        let mut fmt = SendFormat::new();
        fmt.set_button(&[Button::A]);
        assert_eq!(fmt.btn, Button::A.bits());
    }

    #[test]
    fn test_set_button_multiple() {
        let mut fmt = SendFormat::new();
        fmt.set_button(&[Button::A, Button::B, Button::X]);
        assert_eq!(
            fmt.btn,
            Button::A.bits() | Button::B.bits() | Button::X.bits()
        );
    }

    #[test]
    fn test_unset_button() {
        let mut fmt = SendFormat::new();
        fmt.set_button(&[Button::A, Button::B]);
        fmt.unset_button(&[Button::A]);
        assert_eq!(fmt.btn & Button::A.bits(), 0);
        assert_ne!(fmt.btn & Button::B.bits(), 0);
    }

    #[test]
    fn test_reset_all_buttons() {
        let mut fmt = SendFormat::new();
        fmt.set_button(&[Button::A, Button::B, Button::X]);
        fmt.reset_all_buttons();
        assert_eq!(fmt.btn, 0);
    }

    #[test]
    fn test_set_button_3ds_bits() {
        let mut fmt = SendFormat::new();
        fmt.set_button_3ds_bits(0x00FF);
        assert_eq!(fmt.btn, 0x00FF);
    }

    #[test]
    fn test_unset_button_3ds_bits() {
        let mut fmt = SendFormat::new();
        fmt.set_button_3ds_bits(0x00FF);
        fmt.unset_button_3ds_bits(0x00F0);
        assert_eq!(fmt.btn, 0x000F);
    }

    #[test]
    fn test_set_hat() {
        let mut fmt = SendFormat::new();
        fmt.set_hat(&[Hat::TOP]);
        assert_eq!(fmt.hat, CONVERT_HAT_DEFAULT[Hat::TOP as usize]);
    }

    #[test]
    fn test_unset_hat() {
        let mut fmt = SendFormat::new();
        fmt.set_hat(&[Hat::RIGHT]);
        fmt.unset_hat();
        assert_eq!(fmt.hat, CONVERT_HAT_DEFAULT[Hat::CENTER as usize]);
    }

    #[test]
    fn test_set_any_direction_left() {
        let mut fmt = SendFormat::new();
        let dir = Direction::from_xy(Stick::Left, 200, 100);
        fmt.set_any_direction(&[dir]);
        assert_eq!(fmt.lx, 200);
        assert_eq!(fmt.ly, 155); // 255 - 100
    }

    #[test]
    fn test_set_any_direction_right() {
        let mut fmt = SendFormat::new();
        let dir = Direction::from_xy(Stick::Right, 50, 200);
        fmt.set_any_direction(&[dir]);
        assert_eq!(fmt.rx, 50);
        assert_eq!(fmt.ry, 55); // 255 - 200
    }

    #[test]
    fn test_unset_direction_left_up() {
        let mut fmt = SendFormat::new();
        fmt.lx = 200;
        fmt.ly = 50;
        fmt.unset_direction(&[Tilt::Up]);
        assert_eq!(fmt.ly, DIRECTION_CENTER);
        assert_eq!(fmt.lx, 255); // fix_other_axis for > center
    }

    #[test]
    fn test_unset_direction_right_down() {
        let mut fmt = SendFormat::new();
        fmt.rx = 100;
        fmt.ry = 200;
        fmt.unset_direction(&[Tilt::RDown]);
        assert_eq!(fmt.ry, DIRECTION_CENTER);
        assert_eq!(fmt.rx, 0); // fix_other_axis for < center
    }

    #[test]
    fn test_reset_all_directions() {
        let mut fmt = SendFormat::new();
        fmt.lx = 10;
        fmt.ly = 20;
        fmt.rx = 30;
        fmt.ry = 40;
        fmt.reset_all_directions();
        assert_eq!(fmt.lx, DIRECTION_CENTER);
        assert_eq!(fmt.ly, DIRECTION_CENTER);
        assert_eq!(fmt.rx, DIRECTION_CENTER);
        assert_eq!(fmt.ry, DIRECTION_CENTER);
    }

    #[test]
    fn test_set_touchscreen() {
        let mut fmt = SendFormat::new();
        fmt.set_touchscreen(&[Touchscreen::new(500, 200)]);
        assert_eq!(fmt.sx, 500);
        assert_eq!(fmt.sy, 200);
    }

    #[test]
    fn test_unset_touchscreen() {
        let mut fmt = SendFormat::new();
        fmt.set_touchscreen(&[Touchscreen::new(100, 50)]);
        fmt.unset_touchscreen();
        assert_eq!(fmt.sx, 0);
        assert_eq!(fmt.sy, 0);
    }

    // ── Conversion tests ───────────────────────────────────────────────

    #[test]
    fn test_convert_to_default_no_sticks() {
        let mut fmt = SendFormat::new();
        fmt.set_button(&[Button::A, Button::B]);
        fmt.set_hat(&[Hat::TOP]);
        let result = fmt.convert_to_default(false, false);
        // btn << 2 = 0x0006 << 2 = 0x0018 → formatted as {:#08x} = "0x000018"
        assert!(result.starts_with("0x000018"), "got: {result}");
        assert!(result.contains(" 0")); // Hat::TOP = 0
    }

    #[test]
    fn test_convert_to_default_with_l_stick() {
        let mut fmt = SendFormat::new();
        fmt.set_button(&[Button::A]);
        fmt.lx = 200;
        fmt.ly = 100;
        let result = fmt.convert_to_default(true, false);
        // A=0x0004 << 2 = 0x0010, LSB=0x2 for l_stick → 0x0012 → "0x000012"
        assert!(result.starts_with("0x000012"), "got: {result}");
        assert!(result.contains("c8 64")); // 200 = 0xc8, 100 = 0x64
    }

    #[test]
    fn test_convert_to_default_with_both_sticks() {
        let mut fmt = SendFormat::new();
        fmt.btn = 0;
        fmt.lx = 200;
        fmt.ly = 100;
        fmt.rx = 50;
        fmt.ry = 200;
        let result = fmt.convert_to_default(true, true);
        // btn=0, both flags set → 0x0003 → "0x000003"
        assert!(result.starts_with("0x000003"), "got: {result}");
        assert!(result.contains("c8 64")); // lx ly
        assert!(result.contains("32 c8")); // rx ry
    }

    #[test]
    fn test_convert_to_qingpi() {
        let mut fmt = SendFormat::new();
        fmt.set_button(&[Button::A, Button::X]);
        fmt.set_hat(&[Hat::TOP]);
        fmt.set_touchscreen(&[Touchscreen::new(300, 100)]);
        let result = fmt.convert_to_qingpi();
        assert_eq!(result.len(), 11);
        assert_eq!(result[0], 0xAB); // magic byte
        assert_eq!(result[1], (Button::A.bits() | Button::X.bits()) as u8);
        assert_eq!(result[3], CONVERT_HAT_DEFAULT[Hat::TOP as usize]);
        assert_eq!(result[6], DIRECTION_CENTER); // rx default
        assert_eq!(result[7], DIRECTION_CENTER); // ry default
        // touchscreen: sx=300 → 0x012C → lo=0x2C, hi=0x01
        assert_eq!(result[8], 0x2C);
        assert_eq!(result[9], 0x01);
        assert_eq!(result[10], 100);
    }

    #[test]
    fn test_convert_to_qingpi_empty() {
        let fmt = SendFormat::new();
        let result = fmt.convert_to_qingpi();
        assert_eq!(result[1], 0); // no buttons
        assert_eq!(result[3], CONVERT_HAT_DEFAULT[Hat::CENTER as usize]);
    }

    #[test]
    fn test_convert_to_3ds() {
        let mut fmt = SendFormat::new();
        fmt.set_button_3ds_bits(1 | 2 | 4); // A + B + X
        let result = fmt.convert_to_3ds();
        assert_eq!(result.len(), 6);
        assert_eq!(result[0], 0xA1);
        assert_eq!(result[3], 0xA2);
        // byte1 = ((btn & 0xF) << 4) | hat → (7 << 4) | CENTER_CONVERTED = 0x70
        assert_eq!(result[1], 0x70);
    }

    #[test]
    fn test_convert_to_3ds_stick_inversion() {
        let mut fmt = SendFormat::new();
        fmt.lx = 200; // >= 128 → unchanged
        fmt.ly = 50; // < 128 → 127 - 50 = 77
        let result = fmt.convert_to_3ds();
        assert_eq!(result[4], 200);
        assert_eq!(result[5], 77);
    }

    #[test]
    fn test_reset() {
        let mut fmt = SendFormat::new();
        fmt.set_button(&[Button::A, Button::B]);
        fmt.set_hat(&[Hat::TOP]);
        fmt.reset();
        assert_eq!(fmt.btn, 0);
        assert_eq!(fmt.hat, Hat::CENTER as u8);
        assert_eq!(fmt.lx, DIRECTION_CENTER);
        assert_eq!(fmt.ly, DIRECTION_CENTER);
    }

    #[test]
    fn test_fix_other_axis() {
        assert_eq!(
            SendFormat::fix_other_axis(DIRECTION_CENTER),
            DIRECTION_CENTER
        );
        assert_eq!(SendFormat::fix_other_axis(0), 0);
        assert_eq!(SendFormat::fix_other_axis(255), 255);
        assert_eq!(SendFormat::fix_other_axis(50), 0);
        assert_eq!(SendFormat::fix_other_axis(200), 255);
    }
}
