//! `toyou-login`: the Google sign-in window, launched by toyou. See `src/login.rs`.

// Shares the session code with the main app; parts of it are only used there.
#![allow(dead_code)]

#[path = "../auth.rs"]
mod auth;
#[path = "../login.rs"]
mod login;

fn main() {
    login::run();
}
