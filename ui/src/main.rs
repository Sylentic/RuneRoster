//! RuneRoster's real UI — profile list, add-account wizard, and character picker.
//!
//! Built with `iced`'s Elm-architecture pattern: `App` holds all state, `update` reacts to
//! `Message`s (including results of async `core` calls run via `Task::perform`), and `view`
//! renders the current `Screen`. See `rs-launcher-plan.md` for why this UI is fully custom
//! rather than modeled on Bolt's.
//!
//! The add-account flow needs two manual URL pastes (login, then consent) — not a UX choice,
//! a confirmed constraint of Jagex's OAuth server (see `runeroster_core::auth` module docs).

use std::path::PathBuf;

use iced::widget::{button, column, container, row, scrollable, text, text_input};
use iced::{Element, Length, Task, Theme};
use uuid::Uuid;

use runeroster_core::accounts::{add_profile_from_login, remove_profile, ProfileRegistry};
use runeroster_core::auth::{LoginFlow, LoginOutcome};
use runeroster_core::characters::Character;
use runeroster_core::launcher::{launch, LaunchTarget};
use runeroster_core::session::{reconnect_profile, LaunchSession};

fn profiles_path() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("RuneRoster")
        .join("profiles.json")
}

struct App {
    http: reqwest::Client,
    registry: ProfileRegistry,
    registry_path: PathBuf,
    runelite_path: Option<PathBuf>,
    screen: Screen,
    status: Option<String>,
}

enum Screen {
    ProfileList,
    Busy(String),
    AddAccount(AddAccountStep),
    CharacterPicker {
        profile_id: Uuid,
        session_id: String,
        characters: Vec<Character>,
    },
}

enum AddAccountStep {
    EnteringLoginRedirect {
        flow: LoginFlow,
        login_url: String,
        input: String,
    },
    EnteringConsentRedirect {
        flow: LoginFlow,
        consent_url: String,
        input: String,
    },
    EnteringDisplayName {
        outcome: LoginOutcome,
        input: String,
    },
}

#[derive(Debug, Clone)]
enum Message {
    StartAddAccount,
    CancelAddAccount,
    OpenUrl(String),
    LoginRedirectInputChanged(String),
    SubmitLoginRedirect,
    LoginStepDone(Result<(LoginFlow, String), String>),
    ConsentRedirectInputChanged(String),
    SubmitConsentRedirect,
    ConsentStepDone(Result<LoginOutcome, String>),
    DisplayNameInputChanged(String),
    SubmitDisplayName,
    LaunchProfile(Uuid),
    ReconnectDone(Uuid, Result<(String, Vec<Character>), String>),
    RemoveProfile(Uuid),
    LaunchCharacter(usize),
}

impl App {
    fn new() -> Self {
        let registry_path = profiles_path();
        let registry = ProfileRegistry::load(&registry_path).unwrap_or_default();
        let runelite_path = runeroster_platform::find_runelite();
        Self {
            http: reqwest::Client::new(),
            registry,
            registry_path,
            runelite_path,
            screen: Screen::ProfileList,
            status: None,
        }
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::StartAddAccount => {
                let (flow, login_url) = LoginFlow::start();
                self.screen = Screen::AddAccount(AddAccountStep::EnteringLoginRedirect {
                    flow,
                    login_url,
                    input: String::new(),
                });
                self.status = None;
                Task::none()
            }
            Message::CancelAddAccount => {
                self.screen = Screen::ProfileList;
                Task::none()
            }
            Message::OpenUrl(url) => {
                if let Err(e) = open::that(&url) {
                    self.status = Some(format!("Couldn't open browser: {e}"));
                }
                Task::none()
            }
            Message::LoginRedirectInputChanged(value) => {
                if let Screen::AddAccount(AddAccountStep::EnteringLoginRedirect {
                    input, ..
                }) = &mut self.screen
                {
                    *input = value;
                }
                Task::none()
            }
            Message::SubmitLoginRedirect => {
                let previous =
                    std::mem::replace(&mut self.screen, Screen::Busy("Logging in...".into()));
                if let Screen::AddAccount(AddAccountStep::EnteringLoginRedirect {
                    flow,
                    input,
                    ..
                }) = previous
                {
                    let http = self.http.clone();
                    Task::perform(
                        async move { flow.submit_login_redirect(&http, &input).await },
                        |result| Message::LoginStepDone(result.map_err(|e| e.to_string())),
                    )
                } else {
                    self.screen = previous;
                    Task::none()
                }
            }
            Message::LoginStepDone(result) => {
                match result {
                    Ok((flow, consent_url)) => {
                        self.screen = Screen::AddAccount(AddAccountStep::EnteringConsentRedirect {
                            flow,
                            consent_url,
                            input: String::new(),
                        });
                    }
                    Err(e) => {
                        self.screen = Screen::ProfileList;
                        self.status = Some(format!("Login failed: {e}"));
                    }
                }
                Task::none()
            }
            Message::ConsentRedirectInputChanged(value) => {
                if let Screen::AddAccount(AddAccountStep::EnteringConsentRedirect {
                    input, ..
                }) = &mut self.screen
                {
                    *input = value;
                }
                Task::none()
            }
            Message::SubmitConsentRedirect => {
                let previous = std::mem::replace(
                    &mut self.screen,
                    Screen::Busy("Finishing login...".into()),
                );
                if let Screen::AddAccount(AddAccountStep::EnteringConsentRedirect {
                    flow,
                    input,
                    ..
                }) = previous
                {
                    let http = self.http.clone();
                    Task::perform(
                        async move { flow.submit_consent_redirect(&http, &input).await },
                        |result| Message::ConsentStepDone(result.map_err(|e| e.to_string())),
                    )
                } else {
                    self.screen = previous;
                    Task::none()
                }
            }
            Message::ConsentStepDone(result) => {
                match result {
                    Ok(outcome) => {
                        self.screen = Screen::AddAccount(AddAccountStep::EnteringDisplayName {
                            outcome,
                            input: String::new(),
                        });
                    }
                    Err(e) => {
                        self.screen = Screen::ProfileList;
                        self.status = Some(format!("Consent step failed: {e}"));
                    }
                }
                Task::none()
            }
            Message::DisplayNameInputChanged(value) => {
                if let Screen::AddAccount(AddAccountStep::EnteringDisplayName {
                    input, ..
                }) = &mut self.screen
                {
                    *input = value;
                }
                Task::none()
            }
            Message::SubmitDisplayName => {
                let previous =
                    std::mem::replace(&mut self.screen, Screen::Busy("Saving profile...".into()));
                if let Screen::AddAccount(AddAccountStep::EnteringDisplayName {
                    outcome,
                    input,
                }) = previous
                {
                    let result = add_profile_from_login(
                        &mut self.registry,
                        &self.registry_path,
                        input,
                        &outcome.session_id,
                    );
                    self.status = Some(match result {
                        Ok(profile) => format!("Added profile \"{}\".", profile.display_name),
                        Err(e) => format!("Failed to save profile: {e}"),
                    });
                } else {
                    self.status = Some("Add-account flow was in an unexpected state.".into());
                }
                self.screen = Screen::ProfileList;
                Task::none()
            }
            Message::LaunchProfile(id) => {
                self.screen = Screen::Busy("Reconnecting...".into());
                self.status = None;
                let http = self.http.clone();
                Task::perform(
                    async move { reconnect_profile(&http, id).await },
                    move |result| {
                        Message::ReconnectDone(
                            id,
                            result
                                .map(|session| (session.session_id, session.characters))
                                .map_err(|e| e.to_string()),
                        )
                    },
                )
            }
            Message::ReconnectDone(profile_id, result) => {
                match result {
                    Ok((session_id, characters)) => {
                        self.screen = Screen::CharacterPicker {
                            profile_id,
                            session_id,
                            characters,
                        };
                    }
                    Err(e) => {
                        self.screen = Screen::ProfileList;
                        self.status = Some(format!(
                            "Couldn't reconnect (you may need to log in again): {e}"
                        ));
                    }
                }
                Task::none()
            }
            Message::RemoveProfile(id) => {
                self.status = Some(
                    match remove_profile(&mut self.registry, &self.registry_path, id) {
                        Ok(()) => "Profile removed.".into(),
                        Err(e) => format!("Failed to remove profile: {e}"),
                    },
                );
                Task::none()
            }
            Message::LaunchCharacter(index) => {
                self.status = Some(self.try_launch_character(index));
                self.screen = Screen::ProfileList;
                Task::none()
            }
        }
    }

    /// Spawns RuneLite for the currently-picked character. Returns a status message rather
    /// than a `Result` since this is only ever used to populate `self.status`.
    fn try_launch_character(&self, index: usize) -> String {
        let Screen::CharacterPicker {
            profile_id,
            session_id,
            characters,
        } = &self.screen
        else {
            return "No character picker was active.".into();
        };
        let Some(character) = characters.get(index) else {
            return "No character at that index.".into();
        };
        let Some(profile) = self.registry.profiles.iter().find(|p| p.id == *profile_id) else {
            return "Profile no longer exists.".into();
        };
        let Some(runelite_path) = &self.runelite_path else {
            return "RuneLite not found in the default install location.".into();
        };

        let launch_session = LaunchSession::for_character(session_id.clone(), character);
        match launch(
            LaunchTarget::WindowsExe(runelite_path.clone()),
            profile,
            &launch_session,
        ) {
            Ok(()) => format!("Launched RuneLite as \"{}\".", launch_session.display_name),
            Err(e) => format!("Failed to launch RuneLite: {e}"),
        }
    }

    fn view(&self) -> Element<'_, Message> {
        match &self.screen {
            Screen::ProfileList => self.view_profile_list(),
            Screen::Busy(message) => container(text(message.clone())).padding(20).into(),
            Screen::AddAccount(step) => Self::view_add_account(step),
            Screen::CharacterPicker { characters, .. } => Self::view_character_picker(characters),
        }
    }

    fn view_profile_list(&self) -> Element<'_, Message> {
        let mut list = column![].spacing(10);
        for profile in &self.registry.profiles {
            list = list.push(
                row![
                    text(profile.display_name.clone()).width(Length::Fill),
                    button("Launch").on_press(Message::LaunchProfile(profile.id)),
                    button("Remove").on_press(Message::RemoveProfile(profile.id)),
                ]
                .spacing(10),
            );
        }

        let mut content = column![text("RuneRoster").size(28), list]
            .spacing(20)
            .push(button("Add Account").on_press(Message::StartAddAccount));

        if self.runelite_path.is_none() {
            content = content.push(text(
                "Warning: RuneLite was not found in the default install location.",
            ));
        }
        if let Some(status) = &self.status {
            content = content.push(text(status.clone()));
        }

        scrollable(content.padding(20)).into()
    }

    fn view_add_account(step: &AddAccountStep) -> Element<'_, Message> {
        let content = match step {
            AddAccountStep::EnteringLoginRedirect {
                login_url, input, ..
            } => column![
                text("Step 1: Log in").size(20),
                text("Open the login page, log in, then paste the URL it redirects to."),
                button("Open login page").on_press(Message::OpenUrl(login_url.clone())),
                text_input("Paste redirect URL here", input)
                    .on_input(Message::LoginRedirectInputChanged),
                row![
                    button("Continue").on_press(Message::SubmitLoginRedirect),
                    button("Cancel").on_press(Message::CancelAddAccount),
                ]
                .spacing(10),
            ],
            AddAccountStep::EnteringConsentRedirect {
                consent_url, input, ..
            } => column![
                text("Step 2: Consent").size(20),
                text(
                    "Open the consent page, approve access, then paste the URL it redirects \
                     to (starts with http://localhost/#...)."
                ),
                button("Open consent page").on_press(Message::OpenUrl(consent_url.clone())),
                text_input("Paste redirect URL here", input)
                    .on_input(Message::ConsentRedirectInputChanged),
                row![
                    button("Continue").on_press(Message::SubmitConsentRedirect),
                    button("Cancel").on_press(Message::CancelAddAccount),
                ]
                .spacing(10),
            ],
            AddAccountStep::EnteringDisplayName { outcome, input } => column![
                text("Step 3: Name this profile").size(20),
                text(format!(
                    "Login succeeded — {} character(s) found.",
                    outcome.characters.len()
                )),
                text_input("Profile label (your own name for it)", input)
                    .on_input(Message::DisplayNameInputChanged),
                row![
                    button("Save").on_press(Message::SubmitDisplayName),
                    button("Cancel").on_press(Message::CancelAddAccount),
                ]
                .spacing(10),
            ],
        };

        content.spacing(15).padding(20).into()
    }

    fn view_character_picker(characters: &[Character]) -> Element<'_, Message> {
        let mut list = column![].spacing(10);
        for (index, character) in characters.iter().enumerate() {
            list = list.push(
                row![
                    text(character.display_name.clone()).width(Length::Fill),
                    button("Launch").on_press(Message::LaunchCharacter(index)),
                ]
                .spacing(10),
            );
        }

        column![text("Pick a character").size(20), list]
            .spacing(15)
            .padding(20)
            .into()
    }
}

fn theme(_state: &App) -> Theme {
    Theme::Dark
}

fn main() -> iced::Result {
    iced::application(App::new, App::update, App::view)
        .title("RuneRoster")
        .theme(theme)
        .run()
}
