// Copyright 2026 NotVkontakte LLC (aka Lain)
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// use serde::Deserialize;
use std::env;

#[derive(Clone, Debug)]
pub struct Config {
    pub port: String,
    pub user_agent: String,
}

impl Config {
    pub fn load() -> Self {
        let port = env::var("PORT").unwrap_or_else(|_| "8080".to_string());
        let user_agent = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36".to_string(); // so silly!

        Self { port, user_agent }
    }
}
