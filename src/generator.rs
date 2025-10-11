use anyhow::{anyhow, Context, Result};
use image::io::Reader as ImageReader;
use image::{DynamicImage, GenericImageView};
use serde_json::json;
use base64::{engine::general_purpose, Engine as _};
use image::ImageFormat;
use tracing::{debug, info, warn};

/// Небольшой набор метрик изображения для генерации подписи.
/// - `width`/`height` — габариты изображения
/// - `dominant_hex` — несколько доминирующих оттенков в HEX
#[derive(Debug, Clone)]
pub struct ImageStats {
    pub width: u32,
    pub height: u32,
    pub dominant_hex: Vec<String>,
}

/// Декодирует bytes в изображение, вычисляет базовую статистику
/// и компактную палитру доминирующих цветов.
/// Функция анализирует изображение и возвращает `ImageStats`.
pub fn analyze_image(bytes: &[u8]) -> Result<ImageStats> {
    let img = ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| anyhow!("image format error: {e}"))?
        .decode()
        .map_err(|e| anyhow!("image decode error: {e}"))?;

    let (w, h) = img.dimensions();
    let palette = dominant_colors(&img, 3);
    debug!(width = w, height = h, colors = palette.len(), "analyze_image: рассчитана статистика");

    Ok(ImageStats {
        width: w,
        height: h,
        dominant_hex: palette,
    })
}

/// Строит палитру доминирующих цветов: даунскейлим изображение,
/// квантованием снижаем шум цветового пространства и считаем частоты.
/// Функция вычисляет список доминирующих цветов изображения в HEX.
fn dominant_colors(img: &DynamicImage, take: usize) -> Vec<String> {
    let mut counts = std::collections::HashMap::<(u8, u8, u8), u32>::new();

    // Downscale for speed
    let thumb = img.thumbnail(128, 128).to_rgb8();
    for (_x, _y, p) in thumb.enumerate_pixels() {
        // Simple quantization to reduce color space noise
        let r = (p[0] & 0xF8);
        let g = (p[1] & 0xF8);
        let b = (p[2] & 0xF8);
        *counts.entry((r, g, b)).or_default() += 1;
    }
    let mut items: Vec<((u8, u8, u8), u32)> = counts.into_iter().collect();
    items.sort_by_key(|(_, c)| std::cmp::Reverse(*c));

    items
        .into_iter()
        .take(take)
        .map(|((r, g, b), _)| format!("#{:02X}{:02X}{:02X}", r, g, b))
        .collect()
}

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
pub async fn generate_caption_openai_vision(stats: &ImageStats, bytes: &[u8]) -> Result<String> {
    let api_key = std::env::var("OPENAI_API_KEY").context("переменная OPENAI_API_KEY не задана")?;
    let model = std::env::var("OPENAI_VISION_MODEL").unwrap_or_else(|_| std::env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string()));
    let base = std::env::var("OPENAI_BASE").unwrap_or_else(|_| "https://api.openai.com".to_string());
    debug!(model = %model, base = %base, "Запрос к OpenAI Vision");

    // Сформируем небольшой контекст по тонам (если есть)
    let tones = if stats.dominant_hex.is_empty() { "неопределены".to_string() } else { stats.dominant_hex.join(", ") };
    let system = "Ты генерируешь описание для поста в соцсеть.
Ты професиональный составитель текстов для описания картин, выдающийся эксперт в этой области.
Картины написаны акварелью.
Пишешь от имени юной художницы, девушки. Она рисует картины и открытки акварелью.
Стиль легкий и нежный. Слог простой и вдохновляющий. Ориентируйся на стиль Пушкина.
Описывай что нарисовано во вложении.
Пост должен быть продающим, сделай акциент на том какая это замечательная, эксклющивная и индивидуальная работа.
В конце каждого поста добавляй добрые и радостные пожелания подписчикам, завершенные тремя разными самайликами подходящих под описание.
";

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
        warn!(status = %status, body = %val, "Ошибка OpenAI Vision");
        return Err(anyhow!("openai error: {}", val));
    }
    // Достаём текст ассистента и ограничиваем ~1000 символов
    let content = val["choices"][0]["message"]["content"].as_str()
        .ok_or_else(|| anyhow!("openai response missing content"))?;
    let capped = content.chars().take(1000).collect::<String>();
    debug!(len = capped.len(), "Ответ OpenAI Vision обработан");
    Ok(capped)
}
