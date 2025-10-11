mod config;
mod generator;
mod db;

use anyhow::{Context, Result};
use teloxide::dispatching::UpdateFilterExt;
use teloxide::prelude::*;
use teloxide::types::PhotoSize;
use teloxide::dptree;
use teloxide::requests::Requester;
use teloxide::utils::command::BotCommands as _; // bring trait into scope for descriptions()
use tracing::{debug, error, info, warn};

use crate::config::Config;
use crate::db::Db;
use crate::generator::{analyze_image, generate_caption};
use sha2::{Digest, Sha256};
use tokio::time::{interval, Duration};
use time::OffsetDateTime;
// duplicate imports removed

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    // Init tracing with env filter, e.g. RUST_LOG=info,reqwest=warn
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,reqwest=warn,teloxide=info"));
    tracing_subscriber::fmt().with_env_filter(env_filter).compact().init();

    // teloxide reads TELOXIDE_TOKEN from env by default
    let bot = Bot::from_env();

    // Parse CLI args for --config-json
    let mut config_json_arg: Option<String> = None;
    for arg in std::env::args().skip(1) {
        if let Some(rest) = arg.strip_prefix("--config-json=") {
            config_json_arg = Some(rest.to_string());
            break;
        }
        if arg == "--config-json" {
            // support next-arg form
            config_json_arg = std::env::args().skip_while(|a| a != "--config-json").nth(1);
            break;
        }
    }

    let config = if let Some(json) = config_json_arg {
        info!("Loading config from --config-json");
        Config::from_json_str(&json).context("failed to parse --config-json")?
    } else {
        Config::load().context("failed to load config")?
    };
    let config_storage = std::sync::Arc::new(tokio::sync::RwLock::new(config));

    // Initialize SQLite database
    let db_path = std::env::var("DB_PATH").unwrap_or_else(|_| "bot.db".to_string());
    let db = Db::open(&db_path).await.context("failed to open sqlite db")?;
    let db = std::sync::Arc::new(db);

    // Seed DB channel_id from config if present and DB empty
    if db.get_channel_id().await?.is_none() {
        let cfg = config_storage.read().await;
        if let Some(id) = cfg.channel_id {
            db.set_channel_id(id).await?;
        }
    }

    // Background poster from local folder
    let files_dir = std::env::var("FILES_DIR").unwrap_or_else(|_| "files".to_string());
    let post_interval_secs = std::env::var("POST_INTERVAL_SECS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);
    if post_interval_secs > 0 {
        let bot_bg = bot.clone();
        let db_bg = db.clone();
        tokio::spawn(async move {
            run_periodic_poster(bot_bg, db_bg, files_dir, post_interval_secs).await;
        });
    } else {
        // Use cron from config if provided
        let cron_expr = { config_storage.read().await.post_cron.clone() };
        if let Some(expr) = cron_expr {
            let bot_bg = bot.clone();
            let db_bg = db.clone();
            let files_dir_bg = files_dir.clone();
            tokio::spawn(async move {
                run_cron_poster(bot_bg, db_bg, files_dir_bg, expr).await;
            });
        }
    }

    // Log bot identity
    match bot.get_me().await {
        Ok(me) => {
            info!(
                id = me.id.0,
                username = me.user.username.as_deref().unwrap_or(""),
                "Bot started"
            );
        }
        Err(err) => warn!(error = %err, "Failed to fetch bot info"),
    }

    let handler = Update::filter_message()
        .branch(
            dptree::entry()
                .filter_command::<BotCommand>()
                .endpoint(handle_commands),
        )
        .branch(
            dptree::filter(|msg: Message| msg.photo().is_some())
                .endpoint(handle_photo),
        );

    Dispatcher::builder(bot, handler)
        .dependencies(dptree::deps![config_storage, db])
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;

    Ok(())
}

async fn run_periodic_poster(bot: Bot, db: std::sync::Arc<Db>, files_dir: String, every_secs: u64) {
    let mut ticker = interval(Duration::from_secs(every_secs));
    loop {
        ticker.tick().await;
        if let Err(err) = try_post_from_folder(&bot, &db, &files_dir).await {
            warn!(error = %err, "periodic post: error");
        }
    }
}

async fn run_cron_poster(bot: Bot, db: std::sync::Arc<Db>, files_dir: String, cron: String) {
    // Supported format: "M H * * *" where M and H are either number or '*'
    let spec = match parse_simple_cron(&cron) {
        Ok(s) => s,
        Err(e) => {
            warn!(error = %e, value = %cron, "cron: invalid expression, skipping");
            return;
        }
    };

    let mut last_minute: Option<i32> = None;
    let mut ticker = interval(Duration::from_secs(20));
    loop {
        ticker.tick().await;
        let now_local = OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());
        let m = now_local.minute() as i32;
        let h = now_local.hour() as i32;
        if last_minute == Some(m) { continue; }
        if cron_match_min_hour(&spec, m as u8, h as u8) {
            last_minute = Some(m);
            if let Err(err) = try_post_from_folder(&bot, &db, &files_dir).await {
                warn!(error = %err, "cron post: error");
            }
        }
    }
}

struct CronMinHour {
    minute: Option<u8>, // None = any
    hour: Option<u8>,   // None = any
}

fn parse_simple_cron(s: &str) -> Result<CronMinHour> {
    let parts: Vec<_> = s.split_whitespace().collect();
    if parts.len() != 5 { anyhow::bail!("cron must have 5 fields"); }
    let minute = parse_field_minute_hour(parts[0])?;
    let hour = parse_field_minute_hour(parts[1])?;
    if parts[2] != "*" || parts[3] != "*" || parts[4] != "*" {
        anyhow::bail!("only formats like 'M H * * *' are supported");
    }
    Ok(CronMinHour { minute, hour })
}

fn parse_field_minute_hour(v: &str) -> Result<Option<u8>> {
    if v == "*" { return Ok(None); }
    let n: u8 = v.parse().context("invalid number in cron")?;
    Ok(Some(n))
}

fn cron_match_min_hour(spec: &CronMinHour, minute: u8, hour: u8) -> bool {
    (spec.minute.map_or(true, |m| m == minute)) && (spec.hour.map_or(true, |h| h == hour))
}

async fn try_post_from_folder(bot: &Bot, db: &std::sync::Arc<Db>, files_dir: &str) -> Result<()> {
    // Ensure channel configured
    let Some(channel_id) = db.get_channel_id().await? else {
        debug!("periodic post: channel not configured, skipping");
        return Ok(());
    };

    // List files
    let mut entries = Vec::new();
    match tokio::fs::read_dir(files_dir).await {
        Ok(mut rd) => {
            while let Ok(Some(e)) = rd.next_entry().await {
                entries.push(e);
            }
        }
        Err(err) => {
            warn!(dir = files_dir, error = %err, "periodic post: cannot read dir");
            return Ok(());
        }
    }

    if entries.is_empty() { return Ok(()); }

    // Sort by path name for determinism
    entries.sort_by_key(|e| e.path());

    // Allowed extensions
    fn is_image(p: &std::path::Path) -> bool {
        match p.extension().and_then(|s| s.to_str()).map(|s| s.to_lowercase()) {
            Some(ext) if matches!(ext.as_str(), "jpg"|"jpeg"|"png"|"webp"|"gif"|"bmp"|"tiff") => true,
            _ => false,
        }
    }

    for e in entries {
        let path = e.path();
        if !is_image(&path) { continue; }

        let bytes = match tokio::fs::read(&path).await {
            Ok(b) => b,
            Err(err) => { warn!(file = ?path, error = %err, "periodic post: read failed"); continue; }
        };
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let hash = format!("{:x}", hasher.finalize());

        if db.has_file_hash(&hash).await? {
            debug!(file = ?path, "periodic post: already posted, skip");
            continue;
        }

        // Prepare caption
        let stats = analyze_image(&bytes)?;
        let caption = match generate_caption(&stats, Some(&bytes)).await {
            Ok(c) => c,
            Err(err) => { warn!(error = %err, "periodic post: caption failed, using empty"); String::new() }
        };

        // Send to channel
        let sent = bot
            .send_photo(
                teloxide::types::ChatId(channel_id),
                teloxide::types::InputFile::file(path.clone()),
            )
            .caption(caption.clone())
            .await?;

        // Extract Telegram file_id if present
        let file_id = sent
            .photo()
            .and_then(|v| v.last())
            .map(|p| p.file.id.to_string());

        // Log and store hash
        db.log_post(channel_id, Some(sent.id.0 as i64), file_id, Some(caption)).await?;
        db.insert_file_hash(&hash, path.to_string_lossy().as_ref()).await?;

        info!(?path, "periodic post: posted one file");
        // Post only one per tick
        break;
    }

    Ok(())
}

#[derive(Debug, teloxide::macros::BotCommands, Clone)]
#[command(description = "Доступные команды:")]
enum BotCommand {
    #[command(description = "Показать помощь")] 
    Help,
    #[command(description = "Проверка состояния")] 
    Start,
    #[command(description = "Установить канал: /set_channel -1001234567890")] 
    SetChannel(String),
    #[command(description = "Показать текущие настройки")] 
    Settings,
}

async fn handle_commands(
    bot: Bot,
    msg: Message,
    cmd: BotCommand,
    cfg: std::sync::Arc<tokio::sync::RwLock<Config>>,
    db: std::sync::Arc<Db>,
) -> Result<()> {
    info!(chat_id = %msg.chat.id, from = ?msg.from.as_ref().map(|u| u.id.0), command = ?cmd, "Command received");
    match cmd {
        BotCommand::Help => {
            let text = BotCommand::descriptions().to_string();
            debug!(len = text.len(), "Sending help");
            bot.send_message(msg.chat.id, text)
                .await?;
        }
        BotCommand::Start => {
            bot.send_message(msg.chat.id, "Бот готов. Отправьте фото акварельной картины — я создам продающий текст и опубликую в канал.")
                .await?;
        }
        BotCommand::SetChannel(raw) => {
            let trimmed = raw.trim();
            // Require numeric chat id for reliability (-100...) 
            let parsed = trimmed.parse::<i64>();
            match parsed {
                Ok(id) => {
                    {
                        let mut guard = cfg.write().await;
                        guard.channel_id = Some(id);
                        guard.save()?;
                    }
                    // Persist to SQLite as well
                    db.set_channel_id(id).await?;
                    info!(chat_id = %msg.chat.id, channel_id = id, "Channel set");
                    bot.send_message(msg.chat.id, format!("Канал установлен: {}", trimmed))
                        .await?;
                }
                Err(_) => {
                    warn!(chat_id = %msg.chat.id, value = trimmed, "Invalid channel id format");
                    bot.send_message(
                        msg.chat.id,
                        "Укажите числовой ID канала (например -1001234567890).",
                    )
                    .await?;
                }
            }
        }
        BotCommand::Settings => {
            let from_db = db.get_channel_id().await?;
            let fallback = if from_db.is_none() { cfg.read().await.channel_id } else { None };
            let effective = from_db.or(fallback);
            let text = match effective {
                Some(id) => format!("Канал: {}", id),
                None => "Канал не настроен. Используйте /set_channel <id>".to_string(),
            };
            debug!(chat_id = %msg.chat.id, "Sending settings");
            bot.send_message(msg.chat.id, text).await?;
        }
    }
    Ok(())
}

async fn handle_photo(
    bot: Bot,
    msg: Message,
    cfg: std::sync::Arc<tokio::sync::RwLock<Config>>,
    db: std::sync::Arc<Db>,
) -> Result<()> {
    let Some(photos) = msg.photo() else { return Ok(()); };

    // Choose the biggest photo variant
    let best: &PhotoSize = photos
        .iter()
        .max_by_key(|p| p.width as i64 * p.height as i64)
        .expect("photo list not empty");

    info!(
        chat_id = %msg.chat.id,
        from = ?msg.from.as_ref().map(|u| u.id.0),
        count = photos.len(),
        chosen_w = best.width,
        chosen_h = best.height,
        file_id = %best.file.id,
        "Photo received"
    );

    // Ensure channel configured
    let channel_id = db.get_channel_id().await?;
    if channel_id.is_none() {
        warn!(chat_id = %msg.chat.id, "Channel not configured");
        bot.send_message(
            msg.chat.id,
            "Сначала настройте канал: /set_channel -1001234567890",
        )
        .await?;
        return Ok(());
    }
    let channel_id = channel_id.unwrap();

    // Download file bytes for analysis
    let file = bot.get_file(best.file.id.clone()).await?;
    let token = std::env::var("TELOXIDE_TOKEN").unwrap_or_default();
    let file_url = format!(
        "https://api.telegram.org/file/bot{}/{}",
        token, file.path
    );
    debug!(%file_url, "Downloading image");
    let bytes = reqwest::Client::new()
        .get(file_url)
        .send()
        .await
        .context("failed to download image")?
        .bytes()
        .await
        .context("failed to read image bytes")?;
    debug!(size = bytes.len(), "Image downloaded");

    // Analyze image and generate caption (Vision if enabled)
     let stats = analyze_image(&bytes)?;
     debug!(w = stats.width, h = stats.height, colors = stats.dominant_hex.len(), "Image analyzed");
     let caption = match generate_caption(&stats, Some(&bytes)).await {
         Ok(c) => {
             info!("out = {}", c);
             info!(len = c.len(), "Caption generated");
             c
         }
         Err(err) => {
             error!(error = %err, "Caption generation failed, using empty");
             String::new()
         }
     };

    // Post to the channel using the same file_id to avoid re-upload
    info!(channel_id, "Posting to channel");
    // let caption = String::from("HEllow");
    let sent = bot.send_photo(teloxide::types::ChatId(channel_id), teloxide::types::InputFile::file_id(best.file.id.clone()))
        .caption(caption.clone())
        .await?;
    info!(channel_id, "Posted to channel");

    // Log publication to SQLite
    let msg_id = sent.id.0;
    let file_id = Some(best.file.id.to_string());
    db.log_post(channel_id, Some(msg_id as i64), file_id, Some(caption)).await?;

    // Confirm to the user
    bot.send_message(
        msg.chat.id,
        "Пост опубликован в канал. Спасибо!",
    )
    .await?;

    Ok(())
}
