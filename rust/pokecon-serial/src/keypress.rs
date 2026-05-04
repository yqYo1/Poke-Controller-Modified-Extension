use crate::format::SendFormat;
use crate::keys::{Button, Direction, GamepadInput, Hat, Stick, Tilt, Touchscreen};
use crate::sender::Sender;
use tracing::warn;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SerialFormat {
    #[default]
    Default,
    Qingpi,
    _3dsController,
}

impl SerialFormat {
    pub fn as_str(&self) -> &'static str {
        match self {
            SerialFormat::Default => "Default",
            SerialFormat::Qingpi => "Qingpi",
            SerialFormat::_3dsController => "3DS Controller",
        }
    }
}

pub struct KeyPress {
    serial_data_format: SerialFormat,
    sender: Sender,
    format: SendFormat,
    hold_buttons: Vec<Button>,
    hold_hat: Hat,
    hold_left_stick: Option<Direction>,
    hold_right_stick: Option<Direction>,
    hold_touchscreen: Option<Touchscreen>,
    l_stick_changed: bool,
    r_stick_changed: bool,
}

impl KeyPress {
    pub fn new(sender: Sender) -> Self {
        Self {
            serial_data_format: SerialFormat::Default,
            sender,
            format: SendFormat::new(),
            hold_buttons: Vec::new(),
            hold_hat: Hat::CENTER,
            hold_left_stick: None,
            hold_right_stick: None,
            hold_touchscreen: None,
            l_stick_changed: false,
            r_stick_changed: false,
        }
    }

    pub fn set_serial_format(&mut self, format: SerialFormat) {
        self.serial_data_format = format;
    }

    pub fn get_serial_format(&self) -> SerialFormat {
        self.serial_data_format
    }

    pub fn sender(&self) -> &Sender {
        &self.sender
    }

    pub fn sender_mut(&mut self) -> &mut Sender {
        &mut self.sender
    }

    pub fn hold_buttons(&self) -> Vec<GamepadInput> {
        let mut result: Vec<GamepadInput> = self
            .hold_buttons
            .iter()
            .map(|&b| GamepadInput::SingleButton(b))
            .collect();

        if self.hold_hat != Hat::CENTER {
            result.push(GamepadInput::SingleHat(self.hold_hat));
        }
        if let Some(ref dir) = self.hold_left_stick {
            result.push(GamepadInput::SingleDirection(dir.clone()));
        }
        if let Some(ref dir) = self.hold_right_stick {
            result.push(GamepadInput::SingleDirection(dir.clone()));
        }
        if let Some(ref ts) = self.hold_touchscreen {
            result.push(GamepadInput::SingleTouchscreen(*ts));
        }
        result
    }

    pub fn format(&self) -> &SendFormat {
        &self.format
    }

    pub fn format_mut(&mut self) -> &mut SendFormat {
        &mut self.format
    }

    pub async fn input(&mut self, btns: &[GamepadInput]) -> Result<(), crate::sender::SerialError> {
        let all_btns = self.collect_inputs(btns);

        let buttons: Vec<Button> = all_btns.iter().flat_map(|g| g.buttons()).collect();
        let hats: Vec<Hat> = all_btns.iter().flat_map(|g| g.hats()).collect();
        let directions: Vec<Direction> = all_btns.iter().flat_map(|g| g.directions()).collect();
        let touchscreens: Vec<Touchscreen> =
            all_btns.iter().flat_map(|g| g.touchscreens()).collect();

        self.update_stick_changed(&directions);

        match self.serial_data_format {
            SerialFormat::_3dsController => {
                self.format.reset_all_buttons();
                for btn in &buttons {
                    let bits_3ds = crate::keys::convert_button_3ds(*btn);
                    self.format.set_button_3ds_bits(bits_3ds);
                }
                self.format.set_hat(&hats);
                self.format.set_any_direction(&directions);
                let list = self.format.convert_to_3ds();
                self.sender.write_list(&list, true).await?;
            }
            SerialFormat::Qingpi => {
                self.format.set_button(&buttons);
                self.format.set_hat(&hats);
                self.format.set_any_direction(&directions);
                self.format.set_touchscreen(&touchscreens);
                let list = self.format.convert_to_qingpi();
                self.sender.write_list(&list, true).await?;
            }
            SerialFormat::Default => {
                self.format.set_button(&buttons);
                self.format.set_hat(&hats);
                self.format.set_any_direction(&directions);
                let row = self
                    .format
                    .convert_to_default(self.l_stick_changed, self.r_stick_changed);
                self.sender.write_row(&row, true).await?;
            }
        }

        self.l_stick_changed = false;
        self.r_stick_changed = false;

        Ok(())
    }

    pub async fn input_end(
        &mut self,
        btns: &[GamepadInput],
    ) -> Result<(), crate::sender::SerialError> {
        let tilts: Vec<Tilt> = btns
            .iter()
            .flat_map(|g| g.directions())
            .flat_map(|d| d.get_tilting())
            .collect();

        let unset_hat = self.hold_hat == Hat::CENTER;
        let has_touchscreen = btns
            .iter()
            .any(|g| matches!(g, GamepadInput::SingleTouchscreen(_)));
        let unset_touchscreen = self.hold_touchscreen.is_none() || has_touchscreen;

        self.clear_stick_on_tilts(&tilts);

        match self.serial_data_format {
            SerialFormat::_3dsController => {
                let buttons: Vec<Button> = btns.iter().flat_map(|g| g.buttons()).collect();
                for btn in &buttons {
                    let bits_3ds = crate::keys::convert_button_3ds(*btn);
                    self.format.unset_button_3ds_bits(bits_3ds);
                }
                if unset_hat {
                    self.format.unset_hat();
                }
                self.format.unset_direction(&tilts);
                let list = self.format.convert_to_3ds();
                self.sender.write_list(&list, true).await?;
            }
            SerialFormat::Qingpi => {
                let buttons: Vec<Button> = btns.iter().flat_map(|g| g.buttons()).collect();
                self.format.unset_button(&buttons);
                if unset_hat {
                    self.format.unset_hat();
                }
                self.format.unset_direction(&tilts);
                if unset_touchscreen {
                    self.format.unset_touchscreen();
                }
                let list = self.format.convert_to_qingpi();
                self.sender.write_list(&list, true).await?;
            }
            SerialFormat::Default => {
                let buttons: Vec<Button> = btns.iter().flat_map(|g| g.buttons()).collect();
                self.format.unset_button(&buttons);
                if unset_hat {
                    self.format.unset_hat();
                }
                self.format.unset_direction(&tilts);
                let row = self
                    .format
                    .convert_to_default(self.l_stick_changed, self.r_stick_changed);
                self.sender.write_row(&row, true).await?;
            }
        }

        self.l_stick_changed = false;
        self.r_stick_changed = false;

        Ok(())
    }

    fn push_hold_input(&mut self, btn: &GamepadInput, to_input: &mut Vec<GamepadInput>) {
        match btn {
            GamepadInput::SingleButton(b) => {
                if self.hold_buttons.contains(b) {
                    warn!("{:?} is already in holding state", b);
                } else {
                    self.hold_buttons.push(*b);
                    to_input.push(btn.clone());
                }
            }
            GamepadInput::SingleHat(h) => {
                if *h == Hat::CENTER {
                    return;
                }
                if self.hold_hat == *h {
                    warn!("{:?} is already in holding state", h);
                } else {
                    self.hold_hat = *h;
                    to_input.push(btn.clone());
                }
            }
            GamepadInput::SingleDirection(d) => match d.stick {
                Stick::Left => {
                    if self.hold_left_stick.as_ref() == Some(d) {
                        warn!("{:?} is already in holding state", d);
                    } else {
                        self.hold_left_stick = Some(d.clone());
                        to_input.push(btn.clone());
                    }
                }
                Stick::Right => {
                    if self.hold_right_stick.as_ref() == Some(d) {
                        warn!("{:?} is already in holding state", d);
                    } else {
                        self.hold_right_stick = Some(d.clone());
                        to_input.push(btn.clone());
                    }
                }
            },
            GamepadInput::SingleTouchscreen(t) => {
                if self.hold_touchscreen.as_ref() == Some(t) {
                    warn!("{:?} is already in holding state", t);
                } else {
                    self.hold_touchscreen = Some(*t);
                    to_input.push(btn.clone());
                }
            }
            GamepadInput::Multiple(inputs) => {
                for input in inputs {
                    self.push_hold_input(input, to_input);
                }
            }
        }
    }

    pub async fn hold(&mut self, btns: &[GamepadInput]) -> Result<(), crate::sender::SerialError> {
        let mut to_input = Vec::new();

        for btn in btns {
            self.push_hold_input(btn, &mut to_input);
        }

        if !to_input.is_empty() {
            self.input(&to_input).await?;
        }

        Ok(())
    }

    fn push_hold_end_input(&mut self, btn: &GamepadInput, to_input_end: &mut Vec<GamepadInput>) {
        match btn {
            GamepadInput::SingleButton(b) => {
                if let Some(pos) = self.hold_buttons.iter().position(|x| x == b) {
                    self.hold_buttons.remove(pos);
                    to_input_end.push(btn.clone());
                } else {
                    warn!("{:?} is not in holding state", b);
                }
            }
            GamepadInput::SingleHat(h) => {
                if self.hold_hat == *h {
                    self.hold_hat = Hat::CENTER;
                    to_input_end.push(btn.clone());
                } else {
                    warn!("{:?} is not in holding state", h);
                }
            }
            GamepadInput::SingleDirection(d) => match d.stick {
                Stick::Left => {
                    if self.hold_left_stick.as_ref() == Some(d) {
                        self.hold_left_stick = None;
                        to_input_end.push(btn.clone());
                    } else {
                        warn!("{:?} is not in holding state", d);
                    }
                }
                Stick::Right => {
                    if self.hold_right_stick.as_ref() == Some(d) {
                        self.hold_right_stick = None;
                        to_input_end.push(btn.clone());
                    } else {
                        warn!("{:?} is not in holding state", d);
                    }
                }
            },
            GamepadInput::SingleTouchscreen(t) => {
                if self.hold_touchscreen.as_ref() == Some(t) {
                    self.hold_touchscreen = None;
                    to_input_end.push(btn.clone());
                } else {
                    warn!("{:?} is not in holding state", t);
                }
            }
            GamepadInput::Multiple(inputs) => {
                for input in inputs {
                    self.push_hold_end_input(input, to_input_end);
                }
            }
        }
    }

    pub async fn hold_end(
        &mut self,
        btns: &[GamepadInput],
    ) -> Result<(), crate::sender::SerialError> {
        let mut to_input_end = Vec::new();

        for btn in btns {
            self.push_hold_end_input(btn, &mut to_input_end);
        }

        if !to_input_end.is_empty() {
            self.input_end(&to_input_end).await?;
        }

        Ok(())
    }

    pub async fn neutral(&mut self) -> Result<(), crate::sender::SerialError> {
        let hold_btn = self.hold_buttons();
        self.hold_buttons.clear();
        self.hold_hat = Hat::CENTER;
        self.hold_left_stick = None;
        self.hold_right_stick = None;
        self.hold_touchscreen = None;

        if !hold_btn.is_empty() {
            self.input_end(&hold_btn).await?;
        }

        Ok(())
    }

    pub async fn end(&mut self) -> Result<(), crate::sender::SerialError> {
        match self.serial_data_format {
            SerialFormat::Qingpi | SerialFormat::_3dsController => Ok(()),
            SerialFormat::Default => self.sender.write_row("end", false).await,
        }
    }

    pub async fn serial_command_direct_send(
        &mut self,
        serial_commands: &[String],
        wait_times: &[f64],
    ) -> Result<(), crate::sender::SerialError> {
        if serial_commands.len() != wait_times.len() {
            warn!(
                "Mismatched lengths: serial_commands={}, wait_times={}",
                serial_commands.len(),
                wait_times.len()
            );
        }
        for (wtime, row) in wait_times.iter().zip(serial_commands.iter()) {
            tokio::time::sleep(tokio::time::Duration::from_millis((wtime * 1000.0) as u64)).await;
            self.sender.write_row_wo_counter(row).await?;
        }
        Ok(())
    }

    fn collect_inputs(&self, btns: &[GamepadInput]) -> Vec<GamepadInput> {
        let mut all_btns = self.hold_buttons();
        all_btns.extend_from_slice(btns);
        all_btns
    }

    fn update_stick_changed(&mut self, directions: &[Direction]) {
        for d in directions {
            match d.stick {
                Stick::Left => {
                    self.l_stick_changed = self.format.lx != d.x || self.format.ly != 255 - d.y;
                }
                Stick::Right => {
                    self.r_stick_changed = self.format.rx != d.x || self.format.ry != 255 - d.y;
                }
            }
        }
    }

    fn clear_stick_on_tilts(&mut self, tilts: &[Tilt]) {
        for tilt in tilts {
            match tilt {
                Tilt::Up | Tilt::Down | Tilt::Left | Tilt::Right => {
                    self.l_stick_changed = true;
                }
                Tilt::RUp | Tilt::RDown | Tilt::RLeft | Tilt::RRight => {
                    self.r_stick_changed = true;
                }
            }
        }
    }

    #[allow(dead_code)]
    pub(crate) fn reset_internal_state(&mut self) {
        self.format.reset();
        self.hold_buttons.clear();
        self.hold_hat = Hat::CENTER;
        self.hold_left_stick = None;
        self.hold_right_stick = None;
        self.hold_touchscreen = None;
        self.l_stick_changed = false;
        self.r_stick_changed = false;
    }
}
