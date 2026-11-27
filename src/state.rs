// src/state.rs
use crate::api::{LibraryStory, UserProfile};
use crate::db::DbChapter;

/// Estado compartido de la aplicación.
#[derive(Debug, Clone)]
pub struct AppState {
    pub is_logged_in: bool,
    pub username: String,
    pub password: String,
    pub session_salt: [u8; 16],
    pub current_tab: i32,
    pub is_offline: bool,
    pub user_profile: Option<UserProfile>,
    pub home_stories: Vec<LibraryStory>,
    pub library_stories: Vec<LibraryStory>,
    pub opened_story_id: String,
    pub opened_story_title: String,
    pub opened_chapters: Vec<DbChapter>,
    pub current_chapter_index: usize,
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

impl AppState {
    pub fn new() -> Self {
        Self {
            is_logged_in: false,
            username: String::new(),
            password: String::new(),
            session_salt: [0u8; 16],
            current_tab: 0,
            is_offline: false,
            user_profile: None,
            home_stories: Vec::new(),
            library_stories: Vec::new(),
            opened_story_id: String::new(),
            opened_story_title: String::new(),
            opened_chapters: Vec::new(),
            current_chapter_index: 0,
        }
    }
}
