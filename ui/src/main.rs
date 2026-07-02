//! RuneRoster's real UI — profile list, add-account wizard, and character picker.
//!
//! Built with `iced`'s Elm-architecture pattern: `App` holds all state, `update` reacts to
//! `Message`s (including results of async `core` calls run via `Task::perform`), and `view`
//! renders the current `Screen`. See `rs-launcher-plan.md` for why this UI is fully custom
//! rather than modeled on Bolt's.
//!
//! The add-account flow needs two manual URL pastes (login, then consent) — not a UX choice,
//! a confirmed constraint of Jagex's OAuth server (see `runeroster_core::auth` module docs).
//!
//! Layout (profile list screen): OSRS news on the left, account management on the right,
//! side by side. Uses the built-in `CatppuccinMocha` theme plus rounded "card" containers
//! for a more modern look than iced's bare defaults.

use std::collections::HashMap;
use std::path::PathBuf;

use iced::widget::{button, column, container, image, row, rule, scrollable, text, text_input};
use iced::{Element, Length, Task, Theme};
use uuid::Uuid;

use runeroster_core::accounts::{add_profile_from_login, remove_profile, ProfileRegistry};
use runeroster_core::auth::{LoginFlow, LoginOutcome};
use runeroster_core::characters::Character;
use runeroster_core::launcher::{launch, LaunchTarget};
use runeroster_core::news::{fetch_latest_news, NewsItem};
use runeroster_core::runelite_config::copy_profile_settings_from_default;
use runeroster_core::session::{reconnect_profile, LaunchSession};

const NEWS_ITEM_COUNT: usize = 2;
const NEWS_COLUMN_WIDTH: f32 = 408.0;

fn profiles_path() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("RuneRoster")
        .join("profiles.json")
}

/// A card-style container: rounded background, subtle border — used throughout instead of
/// iced's bare default container to give the UI a more modern, less "raw toolkit" look.
fn card<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(content)
        .padding(16)
        .width(Length::Fill)
        .style(container::rounded_box)
        .into()
}

struct App {
    http: reqwest::Client,
    registry: ProfileRegistry,
    registry_path: PathBuf,
    runelite_path: Option<PathBuf>,
    screen: Screen,
    status: Option<String>,
    news: Vec<NewsItem>,
    news_error: Option<String>,
    news_images: HashMap<String, image::Handle>,
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
    CopySettingsFromDefault(Uuid),
    LaunchCharacter(usize),
    NewsFetched(Result<Vec<NewsItem>, String>),
    NewsImageFetched(String, Result<Vec<u8>, String>),
}

/// Downloads a news thumbnail's raw bytes so it can be decoded into an `image::Handle`.
/// Kept separate from `fetch_latest_news` since the feed's XML has no image bytes, only URLs.
async fn fetch_image_bytes(http: reqwest::Client, url: String) -> (String, Result<Vec<u8>, String>) {
    let result = async {
        let response = http.get(&url).send().await?.error_for_status()?;
        let bytes = response.bytes().await?;
        Ok::<Vec<u8>, reqwest::Error>(bytes.to_vec())
    }
    .await;
    (url, result.map_err(|e| e.to_string()))
}

impl App {
    fn new() -> (Self, Task<Message>) {
        let registry_path = profiles_path();
        let registry = ProfileRegistry::load(&registry_path).unwrap_or_default();
        let runelite_path = runeroster_platform::find_runelite();
        let http = reqwest::Client::new();

        let news_task = {
            let http = http.clone();
            Task::perform(
                async move { fetch_latest_news(&http, NEWS_ITEM_COUNT).await },
                |result| Message::NewsFetched(result.map_err(|e| e.to_string())),
            )
        };

        (
            Self {
                http,
                registry,
                registry_path,
                runelite_path,
                screen: Screen::ProfileList,
                status: None,
                news: Vec::new(),
                news_error: None,
                news_images: HashMap::new(),
            },
            news_task,
        )
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
            Message::CopySettingsFromDefault(id) => {
                self.status = Some(
                    match copy_profile_settings_from_default(&id.to_string()) {
                        Ok(()) => {
                            "Copied Default's plugin settings into this profile. \
                             Restart RuneLite for this profile to see the change."
                                .into()
                        }
                        Err(e) => format!(
                            "Couldn't copy settings (make sure RuneLite isn't running for \
                             this profile, and that it's been launched at least once): {e}"
                        ),
                    },
                );
                Task::none()
            }
            Message::LaunchCharacter(index) => {
                self.status = Some(self.try_launch_character(index));
                self.screen = Screen::ProfileList;
                Task::none()
            }
            Message::NewsFetched(result) => {
                match result {
                    Ok(items) => {
                        let image_tasks = items
                            .iter()
                            .filter_map(|item| item.image_url.clone())
                            .map(|url| {
                                let http = self.http.clone();
                                Task::perform(fetch_image_bytes(http, url), |(url, result)| {
                                    Message::NewsImageFetched(url, result)
                                })
                            })
                            .collect::<Vec<_>>();
                        self.news = items;
                        self.news_error = None;
                        return Task::batch(image_tasks);
                    }
                    Err(e) => self.news_error = Some(e),
                }
                Task::none()
            }
            Message::NewsImageFetched(url, result) => {
                if let Ok(bytes) = result {
                    self.news_images.insert(url, image::Handle::from_bytes(bytes));
                }
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
        let news_section = column![text("OSRS News").size(20), self.view_news(),]
            .spacing(16)
            .width(Length::Fixed(NEWS_COLUMN_WIDTH));

        let mut profile_list = column![].spacing(12);
        for profile in &self.registry.profiles {
            profile_list = profile_list.push(card(
                column![
                    text(profile.display_name.clone()).size(16),
                    row![
                        button("Launch").on_press(Message::LaunchProfile(profile.id)),
                        button("Copy settings from Default")
                            .on_press(Message::CopySettingsFromDefault(profile.id)),
                        button("Remove").on_press(Message::RemoveProfile(profile.id)),
                    ]
                    .spacing(8),
                ]
                .spacing(8),
            ));
        }

        let mut accounts_section = column![
            text("Accounts").size(20),
            profile_list,
            button("Add Account").on_press(Message::StartAddAccount),
        ]
        .spacing(16)
        .width(Length::FillPortion(1));

        if self.runelite_path.is_none() {
            accounts_section = accounts_section.push(text(
                "Warning: RuneLite was not found in the default install location.",
            ));
        }
        if let Some(status) = &self.status {
            accounts_section = accounts_section.push(text(status.clone()));
        }

        let body = row![news_section, rule::vertical(1), accounts_section].spacing(24);

        let content = column![text("RuneRoster").size(30), body]
            .spacing(24)
            .padding(24);

        scrollable(content).into()
    }

    fn view_news(&self) -> Element<'_, Message> {
        if let Some(error) = &self.news_error {
            return text(format!("Couldn't load news: {error}")).into();
        }
        if self.news.is_empty() {
            return text("Loading news...").into();
        }

        let mut list = column![].spacing(16);
        for item in &self.news {
            let mut item_content = column![].spacing(6);

            if let Some(handle) = item
                .image_url
                .as_ref()
                .and_then(|url| self.news_images.get(url))
            {
                item_content = item_content.push(
                    container(
                        image(handle.clone())
                            .width(Length::Fill)
                            .content_fit(iced::ContentFit::Cover),
                    )
                    .height(168)
                    .clip(true),
                );
            }

            item_content = item_content.push(
                column![
                    text(item.category.clone()).size(12),
                    text(item.title.clone()).size(18),
                    text(item.description.clone()).size(14),
                    button("Read more").on_press(Message::OpenUrl(item.link.clone())),
                ]
                .spacing(6),
            );

            list = list.push(card(item_content));
        }
        list.into()
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
    Theme::CatppuccinMocha
}

fn main() -> iced::Result {
    iced::application(App::new, App::update, App::view)
        .title("RuneRoster")
        .theme(theme)
        .run()
}
