use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent};

use crate::config::Config;
use crate::log::{LogLevel, LogLine};
use crate::ssh::Credentials;
use crate::steps::Step;

pub const LOG_CAPACITY: usize = 5000;
/// Typed word required to confirm a destructive step, shown in the modal.
pub const CONFIRM_WORD: &str = "UNINSTALL";

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CredField {
    Ssh,
    Sudo,
}

pub struct CredForm {
    pub ssh_password: String,
    pub sudo_password: String,
    pub focus: CredField,
}

impl CredForm {
    fn new() -> Self {
        Self { ssh_password: String::new(), sudo_password: String::new(), focus: CredField::Ssh }
    }

    fn field_mut(&mut self) -> &mut String {
        match self.focus {
            CredField::Ssh => &mut self.ssh_password,
            CredField::Sudo => &mut self.sudo_password,
        }
    }

    /// Returns true once the user presses Enter (submit).
    fn handle_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Enter => return true,
            KeyCode::Tab | KeyCode::Down | KeyCode::Up => {
                self.focus = match self.focus {
                    CredField::Ssh => CredField::Sudo,
                    CredField::Sudo => CredField::Ssh,
                };
            }
            KeyCode::Backspace => {
                self.field_mut().pop();
            }
            KeyCode::Char(c) => self.field_mut().push(c),
            _ => {}
        }
        false
    }
}

pub struct ConfirmState {
    pub step: Step,
    pub input: String,
}

pub enum Action {
    None,
    Quit,
    CredentialsSubmitted(Credentials),
    Dispatch(Step),
    ProbeAll,
}

pub struct App {
    pub config: Arc<Config>,
    pub cred_form: Option<CredForm>,
    pub selected: usize,
    pub log: VecDeque<LogLine>,
    pub follow: bool,
    pub scroll_up: usize,
    pub running: Option<Step>,
    pub confirm: Option<ConfirmState>,
    /// None = not probed yet, Some(true/false) = last probe result.
    pub node_status: HashMap<String, Option<bool>>,
    pub quit: bool,
}

impl App {
    pub fn new(config: Arc<Config>) -> Self {
        let mut node_status = HashMap::new();
        for n in &config.nodes {
            node_status.insert(n.name.clone(), None);
        }
        App {
            config,
            cred_form: Some(CredForm::new()),
            selected: 0,
            log: VecDeque::with_capacity(LOG_CAPACITY),
            follow: true,
            scroll_up: 0,
            running: None,
            confirm: None,
            node_status,
            quit: false,
        }
    }

    pub fn push_log(&mut self, line: LogLine) {
        if self.log.len() >= LOG_CAPACITY {
            self.log.pop_front();
        }
        self.log.push_back(line);
    }

    pub fn step_finished(&mut self, step: Step, result: Result<(), String>) {
        self.running = None;
        match result {
            Ok(()) => self.push_log(LogLine { node: String::new(), level: LogLevel::Success, text: format!("{} finished", step.title()) }),
            Err(e) => self.push_log(LogLine { node: String::new(), level: LogLevel::Error, text: format!("{} failed: {e}", step.title()) }),
        }
    }

    pub fn set_probe(&mut self, node: &str, reachable: bool) {
        self.node_status.insert(node.to_string(), Some(reachable));
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Action {
        if let Some(form) = self.cred_form.as_mut() {
            if form.handle_key(key) {
                let ssh_password = (!form.ssh_password.is_empty()).then(|| form.ssh_password.clone());
                let sudo_password = (!form.sudo_password.is_empty()).then(|| form.sudo_password.clone());
                self.cred_form = None;
                return Action::CredentialsSubmitted(Credentials { ssh_password, sudo_password });
            }
            return Action::None;
        }

        if let Some(confirm) = self.confirm.as_mut() {
            match key.code {
                KeyCode::Esc => self.confirm = None,
                KeyCode::Enter => {
                    if confirm.input == CONFIRM_WORD {
                        let step = confirm.step;
                        self.confirm = None;
                        return Action::Dispatch(step);
                    }
                }
                KeyCode::Backspace => {
                    confirm.input.pop();
                }
                KeyCode::Char(c) => confirm.input.push(c),
                _ => {}
            }
            return Action::None;
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => return Action::Quit,
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected = self.selected.checked_sub(1).unwrap_or(Step::MENU.len() - 1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.selected = (self.selected + 1) % Step::MENU.len();
            }
            KeyCode::PageUp => {
                self.scroll_up = self.scroll_up.saturating_add(10);
                self.follow = false;
            }
            KeyCode::PageDown => {
                self.scroll_up = self.scroll_up.saturating_sub(10);
                if self.scroll_up == 0 {
                    self.follow = true;
                }
            }
            KeyCode::End => {
                self.scroll_up = 0;
                self.follow = true;
            }
            KeyCode::Char('r') => return Action::ProbeAll,
            KeyCode::Enter if self.running.is_none() => {
                let step = Step::MENU[self.selected];
                if step.destructive() {
                    self.confirm = Some(ConfirmState { step, input: String::new() });
                } else {
                    self.running = Some(step);
                    return Action::Dispatch(step);
                }
            }
            _ => {}
        }
        Action::None
    }
}
