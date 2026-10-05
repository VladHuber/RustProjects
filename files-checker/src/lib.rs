//! Чистая, независимая от файловой системы и сети логика наблюдателя.
//!
//! Здесь собраны небольшие функции, которые легко покрыть тестами:
//! фильтрация путей по glob, дебаунс повторных событий и формирование
//! текста уведомления. Плюс безопасная (без паник) обёртка над Telegram API.

use glob::Pattern;
use notify::EventKind;
use reqwest::Url;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs;
use std::path::Path;
use urlencoding::encode;

/// Базовый адрес Telegram Bot API.
const TELEGRAM_API_URL: &str = "https://api.telegram.org/";

/// Нужно ли игнорировать событие по этому пути.
///
/// Путь игнорируется, если он подходит под glob-шаблон `--ignore`
/// или если это сам файл лога.
pub fn is_ignored(pattern: &Pattern, path: &Path, log_file: &str) -> bool {
    if pattern.matches_path(path) {
        return true;
    }

    path.file_name()
        .is_some_and(|name| name == OsStr::new(log_file))
}

/// Дебаунс: вернуть `true`, если событие по пути `path` нужно обработать сейчас.
///
/// Запоминает время обработки в `last_events`; повторные события внутри
/// окна `debounce_secs` отбрасываются.
pub fn should_process_event(
    last_events: &mut HashMap<String, u64>,
    path: &str,
    now: u64,
    debounce_secs: u64,
) -> bool {
    let should_process = match last_events.get(path) {
        Some(last_time) => now.saturating_sub(*last_time) > debounce_secs,
        None => true,
    };

    if should_process {
        last_events.insert(path.to_string(), now);
    }

    should_process
}

/// Текст уведомления для события; `None` для типов событий, о которых не сообщаем.
pub fn event_message(kind: &EventKind, path: &str) -> Option<String> {
    match kind {
        EventKind::Modify(_) => Some(format!("Изменен файл: {path}")),
        EventKind::Remove(_) => Some(format!("Удален файл: {path}")),
        EventKind::Create(_) => Some(format!("Добавлен файл: {path}")),
        _ => None,
    }
}

/// Прочитать токен бота из JSON-конфига.
///
/// Ошибка возвращается как текст, а не как паника: уведомления — необязательная
/// возможность, и её отсутствие не должно ронять программу.
pub fn load_telegram_token(path: &Path) -> Result<String, String> {
    let data_str = fs::read_to_string(path)
        .map_err(|e| format!("не удалось прочитать {}: {e}", path.display()))?;

    let data: Value = serde_json::from_str(&data_str)
        .map_err(|e| format!("некорректный JSON в {}: {e}", path.display()))?;

    data.get("tg_api_key")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_string)
        .ok_or_else(|| format!("в {} нет непустого поля \"tg_api_key\"", path.display()))
}

/// Отправитель уведомлений в Telegram.
pub struct TelegramNotifier {
    token: String,
    chat_id: String,
    client: reqwest::blocking::Client,
    base_url: Url,
}

impl TelegramNotifier {
    /// Создать отправитель. Ничего не отправляет и не ходит в сеть.
    pub fn new(token: String, chat_id: String) -> Result<Self, String> {
        let base_url = Url::parse(TELEGRAM_API_URL)
            .map_err(|e| format!("некорректный базовый URL Telegram API: {e}"))?;

        Ok(Self {
            token,
            chat_id,
            client: reqwest::blocking::Client::new(),
            base_url,
        })
    }

    /// Отправить сообщение. Ошибка сети или API возвращается вызывающему коду.
    pub fn send(&self, message: &str) -> Result<(), String> {
        let encoded_token = encode(&self.token);
        let url = self
            .base_url
            .join(&format!("bot{encoded_token}/sendMessage"))
            .map_err(|e| format!("не удалось собрать URL Telegram API: {e}"))?;

        let response = self
            .client
            .post(url)
            .header("Content-Type", "application/json")
            .json(&json!({ "chat_id": self.chat_id, "text": message }))
            .send()
            .map_err(|e| format!("ошибка сети при отправке в Telegram: {e}"))?;

        if !response.status().is_success() {
            return Err(format!(
                "Telegram API ответил статусом {}",
                response.status()
            ));
        }

        Ok(())
    }
}

/// Отправить уведомление, если отправитель настроен.
///
/// Отсутствие отправителя — норма (Telegram опционален), ошибка отправки
/// только логируется.
pub fn send_telegram(notifier: Option<&TelegramNotifier>, message: &str) {
    if let Some(notifier) = notifier {
        if let Err(e) = notifier.send(message) {
            log::error!("Не удалось отправить уведомление в Telegram: {e}");
        }
    }
}
