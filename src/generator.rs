use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose, Engine as _};
use image::ImageFormat;
use serde_json::json;

use crate::logging::{compact, log, Level};

const DEFAULT_SYSTEM_PROMPT: &str = "
Когда отвечаешь не переспрашивай что дальше делать, не делай предложений. Ты генерируешь описание для поста в соцсеть.
Ты професиональный составитель текстов для описания картин.
Картины написаны акварелью.
пишешь от имени юной художницы, девушки. Она рисует картины и открытки акварелью.
Первый вариант: стиль легкий и нежный. слог простой.
Описывай что нарисовано во вложении. Не используй сложно подчиненные предложения.
На основании описания вложения сделай текст в виде увлекательной истории.
Должно получиться четыре абзаца текста
";

/// Определяет MIME‑тип по сигнатуре изображения.
/// Функция определяет MIME‑тип изображения по его байтам.
fn guess_mime(bytes: &[u8]) -> &'static str {
    match image::guess_format(bytes) {
        Ok(ImageFormat::Jpeg) => "image/jpeg",
        Ok(ImageFormat::Png) => "image/png",
        Ok(ImageFormat::WebP) => "image/webp",
        Ok(ImageFormat::Gif) => "image/gif",
        Ok(ImageFormat::Bmp) => "image/bmp",
        Ok(ImageFormat::Tiff) => "image/tiff",
        _ => "image/jpeg",
    }
}

/// Генерирует подпись через OpenAI Vision: отправляем картинку как data URL
/// и системный промпт под акварельные работы. Результат укорачиваем,
/// чтобы уложиться в лимит подписи Telegram.
/// Функция генерирует подпись с помощью OpenAI Vision по данным `stats` и байтам изображения.
pub async fn generate_caption_openai_vision(bytes: &[u8]) -> Result<String> {
    let api_key = std::env::var("OPENAI_API_KEY").context("переменная OPENAI_API_KEY не задана")?;
    let model = std::env::var("OPENAI_VISION_MODEL").unwrap_or_else(|_| {
        std::env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-5.2".to_string())
    });
    let base =
        std::env::var("OPENAI_BASE").unwrap_or_else(|_| "https://api.openai.com".to_string());
    log("openai", "vision", Level::Debug, "Запрос к OpenAI Vision")
        .data("model", model.clone())
        .data("base", base.clone())
        .print();

    let system = std::env::var("OPENAI_SYSTEM_PROMPT").unwrap_or_else(|_| {
        log(
            "openai",
            "vision",
            Level::Error,
            "Отсутствует OPENAI_SYSTEM_PROMPT",
        )
        .print();
        DEFAULT_SYSTEM_PROMPT.to_string()
    });

    // Инлайн‑вставка изображения через data URL, чтобы обойтись без внешнего хостинга
    let mime = guess_mime(bytes);
    let b64 = general_purpose::STANDARD.encode(bytes);
    let data_url = format!("data:{};base64,{}", mime, b64);

    // Тело Chat Completions запроса (Vision поддерживается через тип content=image_url)
    let body = json!({
        "model": model,
        "temperature": 0.9,
        "max_tokens": 400,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": [
                {"type": "image_url", "image_url": {"url": data_url}}
            ]}
        ]
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/v1/chat/completions", base))
        .bearer_auth(api_key)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .json(&body)
        .send()
        .await
        .context("ошибка запроса к OpenAI Vision")?;

    let status = resp.status();
    let val: serde_json::Value = resp.json().await.context("некорректный JSON от OpenAI")?;
    if !status.is_success() {
        log("openai", "vision", Level::Warn, "Ошибка OpenAI Vision")
            .data("status", status.to_string())
            .data("body", compact(&val.to_string(), 200))
            .print();
        return Err(anyhow!("openai error: {}", val));
    }
    // Достаём текст ассистента и ограничиваем ~1000 символов
    let content = val["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| anyhow!("openai response missing content"))?;
    let capped = content.chars().take(1000).collect::<String>();
    log(
        "openai",
        "vision",
        Level::Debug,
        "Ответ OpenAI Vision обработан",
    )
    .data("len", capped.len().to_string())
    .print();
    Ok(capped)
}
