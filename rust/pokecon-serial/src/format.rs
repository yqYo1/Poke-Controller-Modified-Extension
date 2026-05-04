use crate::keys::{
    Button, Direction, Hat, Stick, Tilt, Touchscreen, CONVERT_HAT_3DS_CONTROLLER,
    CONVERT_HAT_DEFAULT, DIRECTION_CENTER,
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
