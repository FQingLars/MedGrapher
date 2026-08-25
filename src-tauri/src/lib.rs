use std::sync::Mutex;
use std::time::SystemTime;
use rusqlite::Connection;
use tauri::Manager;

struct DbState(Mutex<Connection>);

const CSV_CONTENT: &str = include_str!("../tables/МКФвКод.csv");
const BASE: &str = "https://chat.qwen.ai";
const MODEL: &str = "qwen3.5-plus";

fn uuid_v4() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn ts_secs() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn make_headers(token: &str, chat_id: Option<&str>) -> Result<reqwest::header::HeaderMap, String> {
    let mut h = reqwest::header::HeaderMap::new();

    let mut insert = |key: &str, value: String| -> Result<(), String> {
        let name = reqwest::header::HeaderName::from_bytes(key.as_bytes())
            .map_err(|e| format!("Недопустимое имя заголовка {key}: {e}"))?;
        let val = reqwest::header::HeaderValue::from_str(&value)
            .map_err(|e| format!("Недопустимое значение заголовка {key}: {e}"))?;
        h.insert(name, val);
        Ok(())
    };

    insert("Authorization", format!("Bearer {}", token))?;
    insert("Content-Type", "application/json".to_string())?;
    insert("Accept", "application/json".to_string())?;
    insert(
        "User-Agent",
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36".to_string(),
    )?;
    insert("Origin", BASE.to_string())?;
    insert("Referer", format!("{BASE}/"))?;
    insert("source", "h5".to_string())?;
    insert("X-Request-Id", uuid_v4())?;
    insert("Cookie", format!("token={token}"))?;
    if let Some(cid) = chat_id {
        insert("Referer", format!("{BASE}/c/{cid}"))?;
        insert("x-accel-buffering", "no".to_string())?;
    }
    Ok(h)
}

async fn create_chat(client: &reqwest::Client, token: &str) -> Result<String, String> {
    let body = serde_json::json!({
        "title": "MedGrapher",
        "models": [MODEL],
        "chat_mode": "temporal",
        "chat_type": "t2t",
        "timestamp": ts_secs() * 1000,
    });

    let resp = client
        .post(format!("{BASE}/api/v2/chats/new"))
        .headers(make_headers(token, None)?)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Ошибка создания чата: {e}"))?;

    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Ошибка создания чата ({}): {}", status, text));
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Ошибка парсинга создания чата: {e}"))?;

    if json.get("success").and_then(|s| s.as_bool()) == Some(false) {
        let code = json["data"]["code"].as_str().unwrap_or("unknown");
        let details = json["data"]["details"].as_str().unwrap_or("неизвестная ошибка");
        return Err(format!("Ошибка API: {code} — {details}"));
    }

    json["data"]["id"]
        .as_str()
        .map(String::from)
        .ok_or_else(|| "Не удалось получить chat_id".into())
}

async fn send_message(
    client: &reqwest::Client,
    token: &str,
    chat_id: &str,
    prompt: &str,
) -> Result<String, String> {
    let ts = ts_secs();
    let user_fid = uuid_v4();

    let body = serde_json::json!({
        "stream": true,
        "incremental_output": true,
        "chat_id": chat_id,
        "chat_mode": "temporal",
        "model": MODEL,
        "parent_id": null,
        "messages": [{
            "fid": user_fid,
            "parentId": null,
            "childrenIds": [],
            "role": "user",
            "content": prompt,
            "user_action": "chat",
            "files": [],
            "timestamp": ts,
            "models": [MODEL],
            "chat_type": "t2t",
            "feature_config": {
                "thinking_enabled": false,
                "output_schema": "phase",
            },
            "extra": {
                "meta": {
                    "subChatType": "t2t"
                }
            },
            "sub_chat_type": "t2t",
            "parent_id": null,
        }],
        "timestamp": ts,
        "size": "1:1",
    });

    let resp = client
        .post(format!("{BASE}/api/v2/chat/completions?chat_id={chat_id}"))
        .headers(make_headers(token, Some(chat_id))?)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Ошибка отправки сообщения: {e}"))?;

    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Ошибка API ({}): {}", status, text));
    }

    // Читаем SSE-поток: data: {...}
    let mut full_text = String::new();
    let mut stream = resp.bytes_stream();

    use futures_util::StreamExt;
    let mut buf = Vec::new();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Ошибка чтения потока: {e}"))?;
        buf.extend_from_slice(&chunk);

        loop {
            // Ищем разделитель строк SSE (\n\n)
            let delim = match buf.windows(2).position(|w| w == b"\n\n") {
                Some(p) => p,
                None => break,
            };

            let line = String::from_utf8_lossy(&buf[..delim]).trim().to_string();
            buf.drain(..delim + 2);

            if !line.starts_with("data: ") {
                continue;
            }
            let data = &line[6..];

            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(data) {
                // Пропускаем response.created
                if parsed.get("response.created").is_some() {
                    continue;
                }
                // Проверяем choices
                if let Some(choices) = parsed["choices"].as_array() {
                    for choice in choices {
                        let delta = &choice["delta"];
                        // Проверяем статус finished
                        if delta.get("status").and_then(|s| s.as_str()) == Some("finished") {
                            // Поток закончен, возвращаем накопленный текст
                            if full_text.is_empty() {
                                // Если ответ пустой — пробуем взять последний content
                                if let Some(c) = delta["content"].as_str() {
                                    if !c.is_empty() {
                                        full_text = c.to_string();
                                    }
                                }
                            }
                            return Ok(full_text);
                        }
                        // Накапливаем content фазы "answer"
                        if let Some(content) = delta["content"].as_str() {
                            if !content.is_empty() {
                                full_text.push_str(content);
                            }
                        }
                    }
                }
            }
        }
    }

    if full_text.is_empty() {
        Err("Пустой ответ от API".into())
    } else {
        Ok(full_text)
    }
}

fn build_prompt(system: &str, user_content: &str) -> String {
    format!("{system}\n\nUser: {user_content}")
}

#[tauri::command]
async fn save_history(
    state: tauri::State<'_, DbState>,
    diagnoses: String,
    scales: Vec<String>,
    response: String,
) -> Result<(), String> {
    let db = state.0.lock().map_err(|e| format!("Ошибка блокировки БД: {e}"))?;
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let scales_str = scales.join(", ");
    db.execute(
        "INSERT INTO query_history (diagnoses, scales, response, created_at) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![diagnoses, scales_str, response, now],
    )
    .map_err(|e| format!("Ошибка сохранения истории: {e}"))?;
    Ok(())
}

#[derive(serde::Serialize)]
struct HistoryEntry {
    id: i64,
    diagnoses: String,
    scales: String,
    created_at: String,
    preview: String,
}

#[tauri::command]
async fn get_history(state: tauri::State<'_, DbState>) -> Result<Vec<HistoryEntry>, String> {
    let db = state.0.lock().map_err(|e| format!("Ошибка блокировки БД: {e}"))?;
    let mut stmt = db
        .prepare("SELECT id, diagnoses, scales, response, created_at FROM query_history ORDER BY id DESC LIMIT 100")
        .map_err(|e| format!("Ошибка запроса: {e}"))?;

    let rows = stmt
        .query_map([], |row| {
            let response: String = row.get(3)?;
            let preview = response.chars().take(120).collect::<String>();
            Ok(HistoryEntry {
                id: row.get(0)?,
                diagnoses: row.get(1)?,
                scales: row.get(2)?,
                created_at: row.get(4)?,
                preview,
            })
        })
        .map_err(|e| format!("Ошибка чтения: {e}"))?;

    let mut entries = Vec::new();
    for row in rows {
        entries.push(row.map_err(|e| format!("Ошибка строки: {e}"))?);
    }
    Ok(entries)
}

#[tauri::command]
async fn clear_old_history(state: tauri::State<'_, DbState>, retention_days: i64) -> Result<u64, String> {
    let db = state.0.lock().map_err(|e| format!("Ошибка блокировки БД: {e}"))?;
    let cutoff = chrono::Local::now() - chrono::Duration::days(retention_days);
    let cutoff_str = cutoff.format("%Y-%m-%d %H:%M:%S").to_string();
    let deleted = db
        .execute(
            "DELETE FROM query_history WHERE created_at < ?1",
            rusqlite::params![cutoff_str],
        )
        .map_err(|e| format!("Ошибка очистки: {e}"))?;
    Ok(deleted as u64)
}

#[tauri::command]
async fn get_history_by_id(state: tauri::State<'_, DbState>, id: i64) -> Result<String, String> {
    let db = state.0.lock().map_err(|e| format!("Ошибка блокировки БД: {e}"))?;
    let response: String = db
        .query_row(
            "SELECT response FROM query_history WHERE id = ?1",
            rusqlite::params![id],
            |row| row.get(0),
        )
        .map_err(|e| format!("Запись не найдена: {e}"))?;
    Ok(response)
}

#[tauri::command]
async fn query_qwen(diagnoses: String, scales: Vec<String>, api_key: String) -> Result<String, String> {
    let token = api_key.trim();
    let diagnoses = diagnoses.trim();

    if token.is_empty() {
        return Err("Не указан токен".into());
    }
    if diagnoses.is_empty() {
        return Err("Не указаны клинические диагнозы".into());
    }
    if scales.is_empty() {
        return Err("Не выбрана ни одна шкала".into());
    }

    let scales_str = scales.join(", ");

    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/120.0.0.0")
        .build()
        .map_err(|e| format!("Ошибка HTTP-клиента: {e}"))?;

    let system = "Ты — медицинский ассистент, эксперт по МКФ, физиотерапии и ЛФК. Отвечай строго по запросу, без лишнего текста.";
    let user = format!(
        "Даны клинические диагнозы: {diagnoses}.

Список доступных шкал для оценки: {scales_str}.

Ты можешь включить свои рассуждения и анализ ПЕРЕД основным ответом. Если ты это делаешь, ОБЯЗАТЕЛЬНО отдели свои рассуждения от данных разделителем &&& (три амперсанда на отдельной строке). После &&& должен идти ТОЛЬКО структурированный ответ, без каких-либо комментариев.

ШАГ 1. Изучи все коды МКФ, которые ты подберёшь к данным диагнозам, и для каждого кода МКФ определи, применима ли к нему каждая из доступных шкал. Используй точные названия шкал из списка выше.

ШАГ 2. После &&& (или сразу, если рассуждений нет) на отдельной строке выведи только список шкал, которые оказались применимы хотя бы к одному коду МКФ (через запятую, без лишнего текста). Например: ВАШ, Мак-Гилла...

ШАГ 3. Сразу после списка на новой строке напиши разделитель: <tables_start>

ШАГ 4. После разделителя напиши четыре Markdown-таблицы подряд:

**Таблица 1 — Основная таблица МКФ**
Столбцы: Клинический диагноз, Код МКФ, Процедуры Физио, Противопоказания Физио, ЛФК, Противопоказания ЛФК.
Ячейки Физио и ЛФК с явными противопоказаниями окрашивай красным цветом (умеренный оттенок, текст читаем).

**Таблица 2 — Применимость шкал к кодам МКФ**
Для каждого кода МКФ укажи + (применима) или - (неприменима) для каждой шкалы из списка применимых.
Столбцы: Код МКФ, <для каждой применимой шкалы отдельный столбец с её точным названием>.

**Таблица 3 — Итоговая таблица**
Столбцы: Клинический диагноз, Код МКФ, ЛФК, Физио.

**Таблица 4 — Справочник кодов МКФ**
Для каждого кода МКФ, который ты использовал в таблицах выше, выведи его код и полное название из справочника.
Столбцы: Код МКФ, Название МКФ.

Учитывай, что все предоставленные диагнозы относятся к одному пациенту: противопоказания от одного диагноза применяются ко всем.

Прежде чем формировать таблицы, удостоверься в их правильности (корректный Markdown, чтобы правильно парсился).

Таблица кодов МКФ и их названий:
\n\n{CSV_CONTENT}"
    );
    let prompt = build_prompt(system, &user);

    let chat_id = create_chat(&client, token).await?;
    send_message(&client, token, &chat_id, &prompt).await
}

fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let msg = match info.payload().downcast_ref::<&str>() {
            Some(s) => (*s).to_string(),
            None => match info.payload().downcast_ref::<String>() {
                Some(s) => s.clone(),
                None => "неизвестная ошибка".to_string(),
            },
        };
        let loc = info
            .location()
            .map(|l| l.to_string())
            .unwrap_or_else(|| "?".to_string());
        let line = format!("MedGrapher panic at {loc}: {msg}");
        eprintln!("{line}");
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(std::env::temp_dir().join("medgrapher_panics.log"))
        {
            use std::io::Write;
            let _ = writeln!(f, "{line}");
        }
    }));
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    install_panic_hook();

    if let Err(e) = tauri::Builder::default()
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let app_data_dir = match app.path().app_data_dir() {
                Ok(dir) => dir,
                Err(e) => {
                    eprintln!("Не удалось определить директорию данных: {e}");
                    return Ok(());
                }
            };
            if let Err(e) = std::fs::create_dir_all(&app_data_dir) {
                eprintln!("Не удалось создать директорию данных: {e}");
            }
            let db_path = app_data_dir.join("history.db");
            let conn = match Connection::open(&db_path) {
                Ok(conn) => conn,
                Err(e) => {
                    eprintln!("Не удалось открыть БД ({db_path:?}): {e}. Использую временную БД.");
                    match Connection::open_in_memory() {
                        Ok(conn) => conn,
                        Err(e2) => {
                            eprintln!("Не удалось создать временную БД: {e2}");
                            return Ok(());
                        }
                    }
                }
            };
            if let Err(e) = conn.execute(
                "CREATE TABLE IF NOT EXISTS query_history (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    diagnoses TEXT NOT NULL,
                    scales TEXT NOT NULL,
                    response TEXT NOT NULL,
                    created_at TEXT NOT NULL
                )",
                [],
            ) {
                eprintln!("Не удалось создать таблицу истории: {e}");
                return Ok(());
            }
            app.manage(DbState(Mutex::new(conn)));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            query_qwen,
            save_history,
            get_history,
            clear_old_history,
            get_history_by_id
        ])
        .run(tauri::generate_context!())
    {
        eprintln!("Ошибка запуска приложения: {e}");
    }
}
