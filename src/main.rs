use std::collections::HashMap;
use teloxide::{prelude::*, utils::command::BotCommands};
use tokio::sync::OnceCell;

#[derive(Debug)]
pub struct Auth {
    pub token: String,
    pub project: String,
}

static CLIENT: OnceCell<reqwest::Client> = OnceCell::const_new();
static AUTH: OnceCell<Auth> = OnceCell::const_new();
static LANGUAGES: OnceCell<HashMap<String, String>> = OnceCell::const_new();

async fn get_languages(
    client: &reqwest::Client,
    auth: &Auth,
) -> reqwest::Result<HashMap<String, String>> {
    const URL: &str = "https://translation.googleapis.com/language/translate/v2/languages";

    let resp = client
        .post(URL)
        .header("Authorization", format!("Bearer {}", auth.token))
        .header("x-goog-user-project", &auth.project)
        .header("Content-Type", "application/json; charset=utf-8")
        .body(r#"{"target": "en"}"#)
        .send()
        .await?;
    let resp_json: serde_json::Value = resp.json().await?;
    log::trace!(
        "get_languages response: {}",
        serde_json::to_string_pretty(&resp_json).unwrap()
    );

    let result = resp_json["data"]["languages"]
        .as_array()
        .unwrap()
        .into_iter()
        .map(|item| {
            (
                item["name"].as_str().unwrap().to_string(),
                item["language"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    Ok(result)
}

async fn detect_language(
    client: &reqwest::Client,
    auth: &Auth,
    text: &str,
) -> reqwest::Result<String> {
    const URL: &str = "https://translation.googleapis.com/language/translate/v2/detect";

    let resp = client
        .post(URL)
        .header("Authorization", format!("Bearer {}", auth.token))
        .header("x-goog-user-project", &auth.project)
        .header("Content-Type", "application/json; charset=utf-8")
        .body(format!(r#"{{"q": "{}"}}"#, text))
        .send()
        .await?;
    let resp_json: serde_json::Value = resp.json().await?;
    log::trace!(
        "detect_language response: {}",
        serde_json::to_string_pretty(&resp_json).unwrap()
    );

    let result = resp_json["data"]["detections"][0][0]["language"]
        .as_str()
        .unwrap()
        .to_string();
    Ok(result)
}

async fn translate(
    client: &reqwest::Client,
    auth: &Auth,
    from: &str,
    to: &str,
    text: &str,
) -> reqwest::Result<String> {
    const URL: &str = "https://translation.googleapis.com/language/translate/v2";

    let resp = client
        .post(URL)
        .header("Authorization", format!("Bearer {}", auth.token))
        .header("x-goog-user-project", &auth.project)
        .header("Content-Type", "application/json; charset=utf-8")
        .body(
            serde_json::json!({
              "q": text,
              "source": from,
              "target": to,
              "format": "text"
            })
            .to_string(),
        )
        .send()
        .await?;
    let resp_json: serde_json::Value = resp.json().await?;
    log::trace!(
        "translate response: {}",
        serde_json::to_string_pretty(&resp_json).unwrap()
    );

    let result = resp_json["data"]["translations"][0]["translatedText"]
        .as_str()
        .unwrap_or_else(|| "<error>")
        .to_string();
    Ok(result)
}

fn load_auth_from_env() -> Auth {
    Auth {
        token: std::env::var("GCP_AUTH").unwrap(),
        project: std::env::var("GCP_PROJECT").unwrap(),
    }
}

#[derive(BotCommands, Clone)]
#[command(
    rename_rule = "lowercase",
    description = "These commands are supported:"
)]
enum Command {
    #[command(description = "Display this text.")]
    Help,
    #[command(description = "Get list of supported languages.")]
    Languages,
    #[command(description = "Detect language.")]
    DetectLanguage(String),
    #[command(
        description = "Translates text into given language. Input language is autodetected.",
        parse_with = "split"
    )]
    TranslateTo { language: String, text: String },
    #[command(
        description = "Translates text from given language into given language.",
        parse_with = "split"
    )]
    TranslateFromTo {
        from_language: String,
        to_language: String,
        text: String,
    },
}

async fn answer(bot: Bot, msg: Message, cmd: Command) -> ResponseResult<()> {
    match cmd {
        Command::Help => {
            bot.send_message(msg.chat.id, Command::descriptions().to_string())
                .await?
        }
        Command::Languages => {
            bot.send_message(
                msg.chat.id,
                format!(
                    "{:?}",
                    LANGUAGES
                        .get()
                        .unwrap()
                        .keys()
                        .into_iter()
                        .collect::<Vec<_>>()
                ),
            )
            .await?
        }
        Command::DetectLanguage(text) => {
            let result = detect_language(CLIENT.get().unwrap(), AUTH.get().unwrap(), &text).await;
            match result {
                Ok(lang) => bot.send_message(msg.chat.id, lang).await?,
                Err(_) => bot.send_message(msg.chat.id, "Internal error").await?,
            }
        }
        Command::TranslateTo {
            language: to_lang,
            text,
        } => {
            let to_lang = LANGUAGES
                .get()
                .unwrap()
                .get(&to_lang)
                .cloned()
                .unwrap_or(to_lang);
            let result = detect_language(CLIENT.get().unwrap(), AUTH.get().unwrap(), &text).await;
            let Ok(from_lang) = result else {
                bot.send_message(msg.chat.id, "Internal error").await?;
                return Ok(());
            };

            let result = translate(
                CLIENT.get().unwrap(),
                AUTH.get().unwrap(),
                &from_lang,
                &to_lang,
                &text,
            )
            .await;
            match result {
                Ok(lang) => bot.send_message(msg.chat.id, lang).await?,
                Err(_) => bot.send_message(msg.chat.id, "Internal error").await?,
            }
        }
        Command::TranslateFromTo {
            from_language: from_lang,
            to_language: to_lang,
            text,
        } => {
            let from_lang = LANGUAGES
                .get()
                .unwrap()
                .get(&from_lang)
                .cloned()
                .unwrap_or(from_lang);
            let to_lang = LANGUAGES
                .get()
                .unwrap()
                .get(&to_lang)
                .cloned()
                .unwrap_or(to_lang);

            let result = translate(
                CLIENT.get().unwrap(),
                AUTH.get().unwrap(),
                &from_lang,
                &to_lang,
                &text,
            )
            .await;
            match result {
                Ok(lang) => bot.send_message(msg.chat.id, lang).await?,
                Err(_) => bot.send_message(msg.chat.id, "Internal error").await?,
            }
        }
    };

    Ok(())
}

#[tokio::main]
async fn main() {
    pretty_env_logger::init();
    log::info!("Starting throw dice bot...");

    let client = reqwest::Client::new();
    CLIENT.set(client).unwrap();

    let auth = load_auth_from_env();
    AUTH.set(auth).unwrap();

    let languages = get_languages(CLIENT.get().unwrap(), AUTH.get().unwrap())
        .await
        .unwrap();
    LANGUAGES.set(languages).unwrap();

    let bot = Bot::from_env();

    Command::repl(bot, answer).await;
}
