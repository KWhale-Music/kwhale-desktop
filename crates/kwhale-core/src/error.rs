use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

/// RFC 7807 `application/problem+json` error payload returned by KWhale Core v1.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Problem {
    #[serde(default)]
    pub r#type: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub status: Option<u16>,
    #[serde(default)]
    pub detail: Option<String>,
    #[serde(default)]
    pub instance: Option<String>,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub fields: HashMap<String, String>,
}

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("{message}")]
    Api {
        status: u16,
        problem: Option<Box<Problem>>,
        message: String,
    },

    #[error("Запрос на сторонний хост заблокирован: {host} (разрешён: {allowed})")]
    HostPolicyViolation { host: String, allowed: String },

    #[error("Некорректный адрес сервера: {0}")]
    InvalidServerUrl(String),

    #[error("Небезопасный HTTP-адрес запрещён без явного подтверждения: {0}")]
    InsecureHttpDisallowed(String),

    #[error("Сетевая ошибка: {0}")]
    Network(#[from] reqwest::Error),

    #[error("Ошибка разбора данных: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Ошибка ввода-вывода: {0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Other(String),
}

impl CoreError {
    pub fn api(status: u16, body: &str) -> Self {
        let problem = serde_json::from_str::<Problem>(body).ok().map(Box::new);
        let message = problem
            .as_ref()
            .and_then(|p| p.detail.clone().or_else(|| p.title.clone()))
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| format!("HTTP {status}"));
        Self::Api {
            status,
            problem,
            message,
        }
    }

    pub fn status_code(&self) -> Option<u16> {
        match self {
            Self::Api { status, .. } => Some(*status),
            _ => None,
        }
    }

    pub fn is_not_found(&self) -> bool {
        self.status_code() == Some(404)
    }

    pub fn is_unauthorized(&self) -> bool {
        self.status_code() == Some(401)
    }

    pub fn is_client_error(&self) -> bool {
        matches!(self.status_code(), Some(400..=499))
    }

    /// Maps API / network failures to clear Russian user-facing messages matching AuthActivity.kt.
    pub fn auth_message(&self) -> String {
        match self {
            Self::Api {
                problem, message, ..
            } => {
                if let Some(p) = problem {
                    match p.code.as_deref() {
                        Some("invalid_invite") => {
                            return "Токен авторизации недействителен или истёк".to_string();
                        }
                        Some("conflict") => {
                            return "Этот email уже зарегистрирован".to_string();
                        }
                        Some("invalid_credentials") => {
                            return "Неверный email или пароль".to_string();
                        }
                        _ => {}
                    }
                    if p.fields.contains_key("password") {
                        return "Пароль: минимум 8 символов".to_string();
                    }
                    if p.fields.contains_key("email") {
                        return "Проверьте email".to_string();
                    }
                    if p.fields.contains_key("inviteToken") {
                        return "Проверьте токен авторизации".to_string();
                    }
                    if p.fields.contains_key("displayName") {
                        return "Укажите имя".to_string();
                    }
                    if let Some(detail) = &p.detail {
                        return detail.clone();
                    }
                    if let Some(title) = &p.title {
                        return title.clone();
                    }
                }
                message.clone()
            }
            Self::Network(e) if e.is_connect() => {
                "Не удалось подключиться к серверу.".to_string()
            }
            Self::Network(e) if e.is_timeout() => {
                "Превышено время ожидания ответа от сервера.".to_string()
            }
            _ => self.to_string(),
        }
    }

    pub fn user_message(&self) -> String {
        self.auth_message()
    }
}

pub type CoreResult<T> = Result<T, CoreError>;
