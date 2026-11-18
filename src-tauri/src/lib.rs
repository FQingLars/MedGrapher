use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use rusqlite::Connection;
use tauri::Manager;

struct DbState(Mutex<Connection>);

const PROCEDURES_CSV: &str = include_str!("../tables/Процедуры.csv");
const LFK_CSV: &str = include_str!("../tables/ЛФК.csv");

/// Встроенная компактная ВБД MedGrapher2 (без эмбеддингов): диагнозы, коды МКФ,
/// противопоказания Физио/ЛФК. Используется для RAG-ретрива прямо в приложении.
const RAG_DB: &[u8] = include_bytes!("../tables/medgrapher_rag.db");

// ─────────────────────────── OpenRouter (OpenAI-compatible) ───────────────────────────
const OPENROUTER_BASE: &str = "https://openrouter.ai/api/v1";
const DEFAULT_MODEL: &str = "baidu/ernie-5.5-flash-preview:free";
// Резервный список (id, отображаемое имя) — если живой запрос к /models недоступен
const FALLBACK_MODELS: &[(&str, &str)] = &[
    ("baidu/ernie-5.5-flash-preview:free", "ERNIE 5.5 Flash (free)"),
    ("google/gemma-4-31b-it:free", "Gemma 4 31B (free)"),
    ("nvidia/nemotron-3-ultra-550b-a55b:free", "Nemotron 3 Ultra 550B (free)"),
    ("deepseek/deepseek-chat", "DeepSeek Chat"),
];

// ─────────────────────────── RAG: модель данных ───────────────────────────
struct Diag {
    klin_diag: String,
    norm: String,
    tokens: Vec<String>,
    mkb: String,
    mkb_compact: String,
    mcf_codes: Vec<String>,
    physio: Vec<String>,
    lfk: Vec<String>,
    scales: Vec<String>,
}

struct Proc {
    name: String,
    contra: Option<String>,
    absolute: Option<String>,
    relative: Option<String>,
    apparatus: Option<String>,
    parameters: Option<String>,
    icf_codes: Option<String>,
    indications: Option<String>,
    effect: Option<String>,
}

struct Card {
    diagnosis: String,
    mkb: String,
    /// строки вида "код (название)"
    mcf: Vec<String>,
    scales: Vec<String>,
    physio: Vec<Proc>,
    lfk: Vec<Proc>,
}

// ── Справочные таблицы (из docx/pdf) ──
struct ProcRow {
    name: String,
    apparatus: String,
    parameters: String,
    time: String,
    course: String,
}
struct ContraRow {
    procedure: String,
    absolute: String,
    relative: String,
}
struct ProcIcfRow {
    procedure: String,
    icf_codes: String,
    indications: String,
    rationale: String,
    effect: String,
}
struct DocumentRow {
    id: i64,
    title: String,
    source: String,
    content: String,
}

/// Данные ВБД, загруженные в память при старте (только чтение).
struct RagStore {
    diags: Vec<Diag>,
    icf_names: HashMap<String, String>,
    physio_contra: HashMap<String, String>,
    lfk_contra: HashMap<String, String>,
    procedures: Vec<ProcRow>,
    contra_full: Vec<ContraRow>,
    proc_icf: Vec<ProcIcfRow>,
    documents: Vec<DocumentRow>,
    /// Нормализованные имена разрешённых процедур (Физио/ЛФК) — для фильтрации.
    allowed_physio: HashSet<String>,
    allowed_lfk: HashSet<String>,
    /// Те же списки в исходном написании — для подстановки в промпт.
    allowed_physio_display: Vec<String>,
    allowed_lfk_display: Vec<String>,
    idf: HashMap<String, f32>,
}

fn normalize(s: &str) -> String {
    let lower = s.to_lowercase().replace('ё', "е");
    let mut out = String::new();
    let mut prev_space = true;
    for ch in lower.chars() {
        if ch.is_alphanumeric() {
            out.push(ch);
            prev_space = false;
        } else if !prev_space {
            out.push(' ');
            prev_space = true;
        }
    }
    out.trim().to_string()
}

fn tokenize(s: &str) -> Vec<String> {
    normalize(s)
        .split(' ')
        .filter(|t| !t.is_empty())
        .map(String::from)
        .collect()
}

fn compact(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .collect::<String>()
        .to_lowercase()
}

/// Нормализация ключа: убираем пробелы, обрамляющие кавычки и ведущий "Bearer ".
fn normalize_api_key(raw: &str) -> String {
    let mut k = raw.trim();
    let b = k.as_bytes();
    if b.len() >= 2 {
        let first = b[0];
        let last = b[b.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            k = k[1..k.len() - 1].trim();
        }
    }
    if k.len() >= 7 && k[..7].eq_ignore_ascii_case("bearer ") {
        k = k[7..].trim();
    }
    k.to_string()
}

fn split_items(s: &str) -> Vec<String> {
    s.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

/// Названия ЛФК-процедур из ЛФК.csv (колонка «Название»).
fn parse_lfk_names(csv: &str) -> Vec<String> {
    csv.lines()
        .skip(1)
        .filter_map(|line| {
            let cols: Vec<&str> = line.split(';').collect();
            if cols.len() >= 2 {
                let n = cols[1].trim();
                if !n.is_empty() && n != "Название" {
                    Some(n.to_string())
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect()
}

fn load_contra(conn: &Connection, table: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let name_col = if table == "physio_contra" { "proc" } else { "item" };
    if let Ok(mut stmt) = conn.prepare(&format!("SELECT {name_col}, contra FROM {table}")) {
        if let Ok(rows) = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        }) {
            for row in rows.flatten() {
                let (n, c) = row;
                map.insert(n.trim().to_lowercase(), c);
            }
        }
    }
    map
}

/// Универсальная загрузка строк таблицы (без паники при ошибке).
fn load_rows<T, F>(conn: &Connection, sql: &str, map: F) -> Vec<T>
where
    F: FnMut(&rusqlite::Row) -> rusqlite::Result<T>,
{
    let mut out = Vec::new();
    if let Ok(mut stmt) = conn.prepare(sql) {
        if let Ok(rows) = stmt.query_map([], map) {
            for r in rows.flatten() {
                out.push(r);
            }
        }
    }
    out
}

/// Загрузка встроенной ВБД в память. Никогда не падает: при ошибке возвращает
/// пустое хранилище (RAG просто не добавит контекст, запрос к LLM всё равно уйдёт).
fn load_rag() -> RagStore {
    let empty = RagStore {
        diags: Vec::new(),
        icf_names: HashMap::new(),
        physio_contra: HashMap::new(),
        lfk_contra: HashMap::new(),
        procedures: Vec::new(),
        contra_full: Vec::new(),
        proc_icf: Vec::new(),
        documents: Vec::new(),
        allowed_physio: HashSet::new(),
        allowed_lfk: HashSet::new(),
        allowed_physio_display: Vec::new(),
        allowed_lfk_display: Vec::new(),
        idf: HashMap::new(),
    };

    // Уникальное имя: load_rag может вызываться несколько раз (тесты идут
    // параллельно), общий temp-файл дал бы гонку. После загрузки файл удаляем.
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let uniq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "medgrapher_rag_{}_{}.db",
        std::process::id(),
        uniq
    ));
    if let Err(e) = std::fs::write(&path, RAG_DB) {
        eprintln!("RAG: не удалось записать ВБД во временный файл: {e}");
        return empty;
    }
    let conn = match Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("RAG: не удалось открыть ВБД: {e}");
            let _ = std::fs::remove_file(&path);
            return empty;
        }
    };

    let scale_names = [
        "ВАШ",
        "Ватерлоо",
        "МакГилл",
        "Рэнкин",
        "Ашфорт",
        "Бартел",
        "Вейсс (мыш.сила)",
        "Ривермид",
        "Хаузер",
        "Ходьба 6мин",
        "Фрейнчай (FRAIL)",
        "DN4",
        "Шкала ДФ постинсульт",
    ];

    let mut diags = Vec::new();
    let query = "SELECT klin_diag, mkb, mcf, physio, lfk, \
                 Ваш, Ватерлоо, МакГилл, Рэнкин, Ашфорт, Бартел, \
                 Вейсс_мыш_сила, Ривермид, Хаузер, Ходьба_6мин, Фрейнчай, DN4, Шкала_ДФ_постинсульт \
                 FROM diagnoses";
    {
        let mut stmt = match conn.prepare(query) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("RAG: ошибка подготовки запроса диагнозов: {e}");
                let _ = std::fs::remove_file(&path);
                return empty;
            }
        };
        let rows = stmt.query_map([], |r| {
            let klin: String = r.get(0)?;
            let mkb: Option<String> = r.get(1)?;
            let mcf: String = r.get(2)?;
            let physio: String = r.get(3)?;
            let lfk: String = r.get(4)?;
            let flags: Vec<i64> = (5..18).map(|i| r.get(i).unwrap_or(0)).collect();
            Ok((klin, mkb.unwrap_or_default(), mcf, physio, lfk, flags))
        });
        match rows {
            Ok(rows) => {
                for row in rows.flatten() {
                    let (klin, mkb, mcf, physio, lfk, flags) = row;
                    let scales: Vec<String> = scale_names
                        .iter()
                        .zip(flags.iter())
                        .filter(|(_, v)| **v != 0)
                        .map(|(n, _)| n.to_string())
                        .collect();
                    let tokens = tokenize(&klin);
                    let norm = normalize(&klin);
                    let mkb_compact = compact(&mkb);
                    diags.push(Diag {
                        klin_diag: klin,
                        norm,
                        tokens,
                        mkb,
                        mkb_compact,
                        mcf_codes: split_items(&mcf)
                            .into_iter()
                            .map(|c| c.to_lowercase())
                            .collect(),
                        physio: split_items(&physio),
                        lfk: split_items(&lfk),
                        scales,
                    });
                }
            }
            Err(e) => eprintln!("RAG: ошибка чтения диагнозов: {e}"),
        }
    }

    // документная частота токенов → IDF
    let n = diags.len().max(1) as f32;
    let mut df: HashMap<String, i64> = HashMap::new();
    for d in &diags {
        let mut seen: Vec<&String> = Vec::new();
        for t in &d.tokens {
            if !seen.contains(&t) {
                seen.push(t);
                *df.entry(t.clone()).or_insert(0) += 1;
            }
        }
    }
    let idf: HashMap<String, f32> = df
        .into_iter()
        .map(|(t, c)| {
            let v = ((n + 1.0) / (c as f32 + 1.0)).ln() + 1.0;
            (t, v)
        })
        .collect();

    let icf_names = {
        let mut m = HashMap::new();
        if let Ok(mut stmt) = conn.prepare("SELECT code, name FROM icf_codes") {
            if let Ok(rows) = stmt.query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            }) {
                for row in rows.flatten() {
                    let (c, nm) = row;
                    m.insert(c.to_lowercase(), nm);
                }
            }
        }
        m
    };

    let physio_contra = load_contra(&conn, "physio_contra");
    let lfk_contra = load_contra(&conn, "lfk_contra");

    // Справочные таблицы из docx/pdf
    let procedures = load_rows(
        &conn,
        "SELECT name, apparatus, parameters, time_min, course_days FROM procedures ORDER BY id",
        |r| {
            Ok(ProcRow {
                name: r.get(0)?,
                apparatus: r.get(1)?,
                parameters: r.get(2)?,
                time: r.get(3)?,
                course: r.get(4)?,
            })
        },
    );
    let contra_full = load_rows(
        &conn,
        "SELECT procedure, absolute, relative FROM contra_full",
        |r| {
            Ok(ContraRow {
                procedure: r.get(0)?,
                absolute: r.get(1)?,
                relative: r.get(2)?,
            })
        },
    );
    let proc_icf = load_rows(
        &conn,
        "SELECT procedure, icf_codes, indications, rationale, effect FROM proc_icf",
        |r| {
            Ok(ProcIcfRow {
                procedure: r.get(0)?,
                icf_codes: r.get(1)?,
                indications: r.get(2)?,
                rationale: r.get(3)?,
                effect: r.get(4)?,
            })
        },
    );
    let documents = load_rows(
        &conn,
        "SELECT id, title, source, content FROM documents ORDER BY id",
        |r| {
            Ok(DocumentRow {
                id: r.get(0)?,
                title: r.get(1)?,
                source: r.get(2)?,
                content: r.get(3)?,
            })
        },
    );

    // Allow-list существующих процедур Физио/ЛФК (для ограничения генерации).
    let mut allowed_physio: HashSet<String> = HashSet::new();
    let mut allowed_physio_display: Vec<String> = Vec::new();
    {
        let add = |name: &str,
               set: &mut HashSet<String>,
               disp: &mut Vec<String>| {
            let n = normalize(name);
            if !n.is_empty() && set.insert(n) {
                disp.push(name.trim().to_string());
            }
        };
        for p in &procedures {
            add(&p.name, &mut allowed_physio, &mut allowed_physio_display);
        }
        for k in physio_contra.keys() {
            add(k, &mut allowed_physio, &mut allowed_physio_display);
        }
        for d in &diags {
            for it in &d.physio {
                add(it, &mut allowed_physio, &mut allowed_physio_display);
            }
        }
    }

    let mut allowed_lfk: HashSet<String> = HashSet::new();
    let mut allowed_lfk_display: Vec<String> = Vec::new();
    {
        let add = |name: &str,
               set: &mut HashSet<String>,
               disp: &mut Vec<String>| {
            let n = normalize(name);
            if !n.is_empty() && set.insert(n) {
                disp.push(name.trim().to_string());
            }
        };
        for k in lfk_contra.keys() {
            add(k, &mut allowed_lfk, &mut allowed_lfk_display);
        }
        for name in parse_lfk_names(LFK_CSV) {
            add(&name, &mut allowed_lfk, &mut allowed_lfk_display);
        }
        for d in &diags {
            for it in &d.lfk {
                add(it, &mut allowed_lfk, &mut allowed_lfk_display);
            }
        }
    }
    allowed_physio_display.sort();
    allowed_lfk_display.sort();

    drop(conn);
    let _ = std::fs::remove_file(&path);

    eprintln!(
        "RAG: загружено диагнозов {}, кодов МКФ {}, физ. {}, лфк {}; процедур {}, прот. {}, МКФ-таблиц {}, документов {}",
        diags.len(),
        icf_names.len(),
        physio_contra.len(),
        lfk_contra.len(),
        procedures.len(),
        contra_full.len(),
        proc_icf.len(),
        documents.len()
    );
    eprintln!(
        "RAG: allow-list процедур — Физио {}, ЛФК {}",
        allowed_physio.len(),
        allowed_lfk.len()
    );

    RagStore {
        diags,
        icf_names,
        physio_contra,
        lfk_contra,
        procedures,
        contra_full,
        proc_icf,
        documents,
        allowed_physio,
        allowed_lfk,
        allowed_physio_display,
        allowed_lfk_display,
        idf,
    }
}

fn find_contra(table: &HashMap<String, String>, name: &str) -> Option<String> {
    let key = name.trim().to_lowercase();
    if let Some(v) = table.get(&key) {
        return Some(v.clone());
    }
    table
        .iter()
        .find(|(k, _)| key.contains(k.as_str()))
        .map(|(_, v)| v.clone())
}

/// Пустое/прочерк → None.
fn ne(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() || t == "-" {
        None
    } else {
        Some(t.to_string())
    }
}

/// Поиск строки по имени: точное совпадение (нормализованное), затем подстрока.
fn find_row<'a, T>(rows: &'a [T], name: &str, key: impl Fn(&T) -> &str) -> Option<&'a T> {
    let k = normalize(name);
    if k.is_empty() {
        return None;
    }
    if let Some(r) = rows.iter().find(|r| normalize(key(r)) == k) {
        return Some(r);
    }
    rows.iter().find(|r| {
        let kk = normalize(key(r));
        !kk.is_empty() && (kk.contains(&k) || k.contains(kk.as_str()))
    })
}

/// Обрезка длинного текста для контекста LLM.
fn trunc(s: &str, max: usize) -> String {
    let t = s.trim();
    if t.chars().count() <= max {
        return t.to_string();
    }
    let mut out: String = t.chars().take(max).collect();
    out.push('…');
    out
}

impl RagStore {
    fn build_card(&self, idx: usize) -> Card {
        let d = &self.diags[idx];
        let mcf = d
            .mcf_codes
            .iter()
            .map(|c| match self.icf_names.get(c) {
                Some(name) => format!("{c} ({name})"),
                None => c.clone(),
            })
            .collect();
        let mk_proc = |items: &[String], table: &HashMap<String, String>| -> Vec<Proc> {
            items
                .iter()
                .map(|it| {
                    let mut p = Proc {
                        name: it.clone(),
                        contra: find_contra(table, it),
                        absolute: None,
                        relative: None,
                        apparatus: None,
                        parameters: None,
                        icf_codes: None,
                        indications: None,
                        effect: None,
                    };
                    if let Some(c) = find_row(&self.contra_full, it, |r| &r.procedure) {
                        p.absolute = ne(&c.absolute);
                        p.relative = ne(&c.relative);
                    }
                    if let Some(pi) = find_row(&self.procedures, it, |r| &r.name) {
                        p.apparatus = ne(&pi.apparatus);
                        p.parameters = ne(&pi.parameters);
                    }
                    if let Some(picf) = find_row(&self.proc_icf, it, |r| &r.procedure) {
                        p.icf_codes = ne(&picf.icf_codes);
                        p.indications = ne(&picf.indications);
                        p.effect = ne(&picf.effect);
                    }
                    p
                })
                .collect()
        };
        Card {
            diagnosis: d.klin_diag.clone(),
            mkb: d.mkb.clone(),
            mcf,
            scales: d.scales.clone(),
            physio: mk_proc(&d.physio, &self.physio_contra),
            lfk: mk_proc(&d.lfk, &self.lfk_contra),
        }
    }
}

/// Лексический ретрив: для каждого фрагмента запроса (через ';' / перевод строки)
/// считаем взвешенное пересечение токенов с диагнозами + бонус за МКБ-код и
/// точное вхождение. Возвращаем top-N карточек, дедуплицированных по диагнозу.
fn retrieve(store: &RagStore, query: &str, top: usize) -> Vec<Card> {
    if store.diags.is_empty() || top == 0 {
        return Vec::new();
    }
    let fragments: Vec<String> = query
        .split(|c| c == ';' || c == '\n')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();

    let mut best: Vec<f32> = vec![0.0; store.diags.len()];

    for frag in &fragments {
        let qtokens = tokenize(frag);
        let qnorm = normalize(frag);
        let qcompact = compact(frag);
        if qtokens.is_empty() && qcompact.is_empty() {
            continue;
        }
        for (i, diag) in store.diags.iter().enumerate() {
            let mut score = 0.0f32;
            for qt in &qtokens {
                if qt.len() < 3 {
                    continue;
                }
                let idf = store.idf.get(qt).copied().unwrap_or(1.0);
                let exact = diag.tokens.iter().any(|t| t == qt);
                if exact {
                    score += idf;
                } else {
                    let prefix = diag.tokens.iter().any(|t| {
                        (t.len() >= 4 && qt.len() >= 4 && t.starts_with(qt.as_str()))
                            || (t.len() >= 4 && qt.len() >= 4 && qt.starts_with(t.as_str()))
                    });
                    if prefix {
                        score += idf * 0.6;
                    }
                }
            }
            // точный МКБ-код
            if !diag.mkb_compact.is_empty()
                && diag.mkb_compact.len() >= 3
                && qcompact.contains(&diag.mkb_compact)
            {
                score += 8.0;
            }
            // фразовое вхождение
            if !qnorm.is_empty() {
                if diag.norm == qnorm {
                    score += 12.0;
                } else if diag.norm.contains(&qnorm) || qnorm.contains(&diag.norm) {
                    score += 6.0;
                }
            }
            if score > best[i] {
                best[i] = score;
            }
        }
    }

    let mut idx: Vec<usize> = (0..best.len()).filter(|&i| best[i] > 0.0).collect();
    idx.sort_by(|&a, &b| {
        best[b]
            .partial_cmp(&best[a])
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    idx.truncate(top);
    idx.into_iter().map(|i| store.build_card(i)).collect()
}

/// Одна процедура в контексте LLM: имя + аппаратура/параметры/МКФ/противопоказания.
fn fmt_proc(p: &Proc) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(a) = &p.apparatus {
        parts.push(format!("аппарат: {}", trunc(a, 150)));
    }
    if let Some(pm) = &p.parameters {
        parts.push(format!("параметры: {}", trunc(pm, 150)));
    }
    if let Some(icf) = &p.icf_codes {
        parts.push(format!("МКФ: {}", trunc(icf, 120)));
    }
    if let Some(c) = &p.absolute {
        parts.push(format!("противопоказания (абс): {}", trunc(c, 220)));
    } else if let Some(c) = &p.contra {
        parts.push(format!("противопоказания: {}", trunc(c, 220)));
    }
    if let Some(c) = &p.relative {
        parts.push(format!("относит: {}", trunc(c, 160)));
    }
    if parts.is_empty() {
        p.name.clone()
    } else {
        format!("{} — {}", p.name, parts.join("; "))
    }
}

/// Разбиение строки markdown-таблицы на ячейки (с учётом `\|`).
fn split_md_row(line: &str) -> Vec<String> {
    let t = line.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = t.strip_suffix('|').unwrap_or(t);
    let mut cells = Vec::new();
    let mut cur = String::new();
    let mut escaped = false;
    for ch in t.chars() {
        if escaped {
            cur.push(ch);
            escaped = false;
        } else if ch == '\\' {
            cur.push(ch);
            escaped = true;
        } else if ch == '|' {
            cells.push(cur.trim().to_string());
            cur.clear();
        } else {
            cur.push(ch);
        }
    }
    cells.push(cur.trim().to_string());
    cells
}

/// Строка-разделитель markdown-таблицы (`|---|---|`).
fn is_separator_row(line: &str) -> bool {
    let t = line.trim();
    t.starts_with('|') && t.chars().all(|c| c == '|' || c == '-' || c == ':' || c == ' ')
}

/// Оставляет в ячейке только процедуры, входящие в allow-list.
fn filter_cell_procedures(cell: &str, allowed: &HashSet<String>) -> String {
    let items: Vec<&str> = cell
        .split([',', ';'])
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    if items.is_empty() {
        return cell.to_string();
    }
    let mut kept: Vec<&str> = Vec::new();
    for it in items {
        let n = normalize(it);
        if n.is_empty() {
            continue;
        }
        let ok = allowed.contains(&n)
            || allowed
                .iter()
                .any(|a| a.contains(n.as_str()) || n.contains(a.as_str()));
        if ok {
            kept.push(it);
        }
    }
    if kept.is_empty() {
        "-".to_string()
    } else {
        kept.join(", ")
    }
}

/// Фильтрует процедуры Физио/ЛФК в markdown-таблицах ответа LLM: удаляет
/// процедуры, которых нет в allow-list (чтобы модель не выдумывала, напр.
/// «электросон»). Обрабатывается только часть после `<tables_start>`.
fn filter_markdown_procedures(
    md: &str,
    allowed_physio: &HashSet<String>,
    allowed_lfk: &HashSet<String>,
) -> String {
    let marker = "<tables_start>";
    let (head, body) = match md.find(marker) {
        Some(i) => {
            let split = i + marker.len();
            (&md[..split], &md[split..])
        }
        None => ("", md),
    };

    let lines: Vec<&str> = body.split('\n').collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    // (индекс колонки, это ЛФК?)
    let mut proc_cols: Vec<(usize, bool)> = Vec::new();
    let mut in_table = false;

    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();

        if !trimmed.starts_with('|') {
            proc_cols.clear();
            in_table = false;
            out.push(line.to_string());
            i += 1;
            continue;
        }

        let next_is_sep = i + 1 < lines.len() && is_separator_row(lines[i + 1]);
        if next_is_sep {
            // header row → определяем колонки процедур
            proc_cols.clear();
            for (idx, h) in split_md_row(trimmed).iter().enumerate() {
                let hl = h.to_lowercase();
                if hl.contains("физио") && !hl.contains("против") {
                    proc_cols.push((idx, false));
                } else if hl.contains("лфк") && !hl.contains("против") {
                    proc_cols.push((idx, true));
                }
            }
            in_table = true;
            out.push(line.to_string());
            i += 1;
            continue;
        }

        if is_separator_row(trimmed) {
            out.push(line.to_string());
            i += 1;
            continue;
        }

        if in_table && !proc_cols.is_empty() {
            let mut cells = split_md_row(trimmed);
            for (idx, is_lfk) in &proc_cols {
                if *idx < cells.len() {
                    let allowed = if *is_lfk { allowed_lfk } else { allowed_physio };
                    cells[*idx] = filter_cell_procedures(&cells[*idx], allowed);
                }
            }
            out.push(format!("| {} |", cells.join(" | ")));
        } else {
            out.push(line.to_string());
        }
        i += 1;
    }

    format!("{head}{}", out.join("\n"))
}

/// Компактный текстовый контекст из карточек для подстановки в промпт LLM.
fn cards_to_context(cards: &[Card]) -> String {
    let mut s = String::new();
    for (i, c) in cards.iter().enumerate() {
        s.push_str(&format!(
            "{}. {}{}\n",
            i + 1,
            c.diagnosis,
            if c.mkb.is_empty() {
                String::new()
            } else {
                format!("  [МКБ: {}]", c.mkb)
            }
        ));
        if !c.mcf.is_empty() {
            s.push_str(&format!("   МКФ: {}\n", c.mcf.join("; ")));
        }
        s.push_str(&format!(
            "   Шкалы: {}\n",
            if c.scales.is_empty() {
                "—".to_string()
            } else {
                c.scales.join(", ")
            }
        ));
        let physio: Vec<String> = c.physio.iter().map(fmt_proc).collect();
        s.push_str(&format!(
            "   Физио: {}\n",
            if physio.is_empty() {
                "—".to_string()
            } else {
                physio.join("; ")
            }
        ));
        let lfk: Vec<String> = c.lfk.iter().map(fmt_proc).collect();
        s.push_str(&format!(
            "   ЛФК: {}\n",
            if lfk.is_empty() {
                "—".to_string()
            } else {
                lfk.join("; ")
            }
        ));
    }
    s
}

// ─────────────────────────── История ───────────────────────────
#[tauri::command]
async fn save_history(
    state: tauri::State<'_, DbState>,
    diagnoses: String,
    scales: Vec<String>,
    response: String,
) -> Result<(), String> {
    let db = state
        .0
        .lock()
        .map_err(|e| format!("Ошибка блокировки БД: {e}"))?;
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
    let db = state
        .0
        .lock()
        .map_err(|e| format!("Ошибка блокировки БД: {e}"))?;
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
async fn clear_old_history(
    state: tauri::State<'_, DbState>,
    retention_days: i64,
) -> Result<u64, String> {
    let db = state
        .0
        .lock()
        .map_err(|e| format!("Ошибка блокировки БД: {e}"))?;
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
    let db = state
        .0
        .lock()
        .map_err(|e| format!("Ошибка блокировки БД: {e}"))?;
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
fn get_procedures_table() -> String {
    PROCEDURES_CSV.to_string()
}

#[tauri::command]
fn get_lfk_table() -> String {
    LFK_CSV.to_string()
}

// ─────────────────────────── RAG-поиск (для отладки/превью) ───────────────────────────
#[derive(serde::Serialize)]
struct RagResult {
    context: String,
    count: usize,
}

#[tauri::command]
fn rag_search(store: tauri::State<'_, RagStore>, query: String, top: Option<u32>) -> RagResult {
    let top = top.unwrap_or(5).clamp(1, 20) as usize;
    let cards = retrieve(&store, &query, top);
    let count = cards.len();
    RagResult {
        context: cards_to_context(&cards),
        count,
    }
}

// ─────────────────────────── Справочник (docx/pdf) ───────────────────────────
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct RefProcedure {
    name: String,
    apparatus: String,
    parameters: String,
    time: String,
    course: String,
}
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct RefContra {
    procedure: String,
    absolute: String,
    relative: String,
}
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct RefProcIcf {
    procedure: String,
    icf_codes: String,
    indications: String,
    rationale: String,
    effect: String,
}
#[derive(serde::Serialize)]
struct RefDocument {
    id: i64,
    title: String,
    source: String,
    content: String,
}
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ReferenceData {
    procedures: Vec<RefProcedure>,
    contraindications: Vec<RefContra>,
    proc_icf: Vec<RefProcIcf>,
    documents: Vec<RefDocument>,
}

#[tauri::command]
fn get_reference_data(state: tauri::State<'_, RagStore>) -> ReferenceData {
    ReferenceData {
        procedures: state
            .procedures
            .iter()
            .map(|p| RefProcedure {
                name: p.name.clone(),
                apparatus: p.apparatus.clone(),
                parameters: p.parameters.clone(),
                time: p.time.clone(),
                course: p.course.clone(),
            })
            .collect(),
        contraindications: state
            .contra_full
            .iter()
            .map(|c| RefContra {
                procedure: c.procedure.clone(),
                absolute: c.absolute.clone(),
                relative: c.relative.clone(),
            })
            .collect(),
        proc_icf: state
            .proc_icf
            .iter()
            .map(|p| RefProcIcf {
                procedure: p.procedure.clone(),
                icf_codes: p.icf_codes.clone(),
                indications: p.indications.clone(),
                rationale: p.rationale.clone(),
                effect: p.effect.clone(),
            })
            .collect(),
        documents: state
            .documents
            .iter()
            .map(|d| RefDocument {
                id: d.id,
                title: d.title.clone(),
                source: d.source.clone(),
                content: d.content.clone(),
            })
            .collect(),
    }
}

// ─────────────────────────── OpenRouter: запрос ───────────────────────────
#[tauri::command]
async fn query_openrouter(
    state: tauri::State<'_, RagStore>,
    diagnoses: String,
    scales: Vec<String>,
    api_key: String,
    model: Option<String>,
) -> Result<String, String> {
    let token = normalize_api_key(&api_key);
    let diagnoses = diagnoses.trim();

    if token.is_empty() {
        return Err("Не указан API-ключ OpenRouter".into());
    }
    if !token.starts_with("sk-or-") {
        return Err(
            "Неверный формат ключа OpenRouter: ожидается ключ вида 'sk-or-...' из openrouter.ai/keys \
             (не токен Qwen и без слова 'Bearer')."
                .into(),
        );
    }
    if diagnoses.is_empty() {
        return Err("Не указаны клинические диагнозы".into());
    }
    if scales.is_empty() {
        return Err("Не выбрана ни одна шкала".into());
    }

    let scales_str = scales.join(", ");

    let model = model
        .map(|m| m.trim().to_string())
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| DEFAULT_MODEL.to_string());

    // RAG: релевантные карточки из ВБД вместо дампа всего справочника МКФ
    let cards = retrieve(&state, diagnoses, 6);
    let rag_context = {
        let ctx = cards_to_context(&cards);
        if ctx.is_empty() {
            "(релевантные карточки не найдены — используй свои знания)".to_string()
        } else {
            ctx
        }
    };

    let client = reqwest::Client::builder()
        .user_agent("MedGrapher/1.0 (+https://openrouter.ai)")
        .build()
        .map_err(|e| format!("Ошибка HTTP-клиента: {e}"))?;

    let system = "Ты — медицинский ассистент, эксперт по МКФ, физиотерапии и ЛФК. Отвечай строго по запросу, без лишнего текста.";
    let physio_allowed = state.allowed_physio_display.join(", ");
    let lfk_allowed = state.allowed_lfk_display.join(", ");
    let user = format!(
        "Даны клинические диагнозы: {diagnoses}.

Список доступных шкал для оценки: {scales_str}.

Ниже — релевантные карточки из медицинской базы данных (RAG), подобранные по диагнозам. Используй их как основу: бери коды МКФ, процедуры Физио/ЛФК и противопоказания из этих карточек. Не выдумывай оборудование или процедуры, которых нет в карточках.

=== КАРТОЧКИ RAG ===
{rag_context}
=== КОНЕЦ КАРТОЧЕК ===

=== РАЗРЕШЁННЫЕ ПРОЦЕДУРЫ (используй ТОЛЬКО точные названия отсюда; НЕ придумывай новых — например, «электросон» и любые процедуры, которых нет в списке, ЗАПРЕЩЕНЫ) ===
Физио: {physio_allowed}
ЛФК: {lfk_allowed}
=== КОНЕЦ СПИСКА ===

Ты можешь включить свои рассуждения и анализ ПЕРЕД основным ответом. Если ты это делаешь, ОБЯЗАТЕЛЬНО отдели свои рассуждения от данных разделителем &&& (три амперсанда на отдельной строке). После &&& должен идти ТОЛЬКО структурированный ответ, без каких-либо комментариев.

ШАГ 1. Изучи коды МКФ из карточек выше, которые подходят к данным диагнозам. Для каждого клинического диагноза подбери 4-7 кодов МКФ из разных категорий (b, d, s, e). Для каждого кода МКФ определи, применима ли к нему каждая из доступных шкал. Используй точные названия шкал из списка выше.

ФОРМАТ ВЫВОДА:
- Таблица 1 должна содержать ровно 4-7 строк с кодами МКФ для каждого диагноза
- Столбец Код МКФ должен содержать коды в формате: b28016, b7100, b7302, b7352, d450 (по одному коду из каждой подходящей категории)
- Столбцы Процедуры Физио и ЛФК должны заполняться ТОЛЬКО процедурами из списка РАЗРЕШЁННЫХ (выше). Придумывать процедуры/аппараты запрещено.
- Если ни одна из разрешённых процедур не подходит к коду МКФ, поставь прочерк (-)
- Противопоказания от одного диагноза применяются ко всем диагнозам пациента

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
Для каждого кода МКФ, который ты использовал в таблицах выше, выведи его код и полное название из карточек RAG (или из своих знаний, если кода нет в карточках).
Столбцы: Код МКФ, Название МКФ.

Учитывай, что все предоставленные диагнозы относятся к одному пациенту: противопоказания от одного диагноза применяются ко всем.

Прежде чем формировать таблицы, удостоверься в их правильности (корректный Markdown, чтобы правильно парсился)."
    );
    let body = serde_json::json!({
        "model": model,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": user }
        ],
        "stream": true,
    });

    let resp = client
        .post(format!("{OPENROUTER_BASE}/chat/completions"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .header("X-Title", "MedGrapher")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Ошибка запроса к OpenRouter: {e}"))?;

    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(
                "OpenRouter отклонил ключ (401). Проверьте, что вставлен корректный ключ из \
                 openrouter.ai/keys (формат sk-or-...), без слова 'Bearer' и лишних символов."
                    .into(),
            );
        }
        return Err(format!("Ошибка OpenRouter ({status}): {text}"));
    }

    // SSE-поток OpenAI-совместимого формата: data: {...}
    use futures_util::StreamExt;
    let mut full_text = String::new();
    let mut stream = resp.bytes_stream();
    let mut buf: Vec<u8> = Vec::new();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Ошибка чтения потока: {e}"))?;
        buf.extend_from_slice(&chunk);

        loop {
            let delim = match buf.windows(2).position(|w| w == b"\n\n") {
                Some(p) => p,
                None => break,
            };
            let line = String::from_utf8_lossy(&buf[..delim]).trim().to_string();
            buf.drain(..delim + 2);

            let Some(data) = line.strip_prefix("data:") else {
                continue;
            };
            let data = data.trim();
            if data == "[DONE]" {
                return if full_text.is_empty() {
                    Err("Пустой ответ от модели".into())
                } else {
                    Ok(filter_markdown_procedures(
                        &full_text,
                        &state.allowed_physio,
                        &state.allowed_lfk,
                    ))
                };
            }
            let Ok(parsed) = serde_json::from_str::<serde_json::Value>(data) else {
                continue;
            };
            if let Some(err) = parsed.get("error") {
                return Err(format!(
                    "Ошибка модели: {}",
                    err["message"].as_str().unwrap_or("неизвестная ошибка")
                ));
            }
            if let Some(choices) = parsed["choices"].as_array() {
                for choice in choices {
                    if let Some(content) = choice["delta"]["content"].as_str() {
                        if !content.is_empty() {
                            full_text.push_str(content);
                        }
                    }
                }
            }
        }
    }

    if full_text.is_empty() {
        Err("Пустой ответ от модели".into())
    } else {
        Ok(filter_markdown_procedures(
            &full_text,
            &state.allowed_physio,
            &state.allowed_lfk,
        ))
    }
}

// ─────────────────────────── OpenRouter: список моделей ───────────────────────────
#[derive(serde::Serialize)]
struct ModelInfo {
    id: String,
    name: String,
}

fn fallback_models() -> Vec<ModelInfo> {
    FALLBACK_MODELS
        .iter()
        .map(|(id, name)| ModelInfo {
            id: id.to_string(),
            name: name.to_string(),
        })
        .collect()
}

#[tauri::command]
async fn get_available_models() -> Vec<ModelInfo> {
    let client = match reqwest::Client::builder()
        .user_agent("MedGrapher/1.0 (+https://openrouter.ai)")
        .timeout(std::time::Duration::from_secs(15))
        .build()
    {
        Ok(c) => c,
        Err(_) => return fallback_models(),
    };

    let resp = match client
        .get(format!("{OPENROUTER_BASE}/models"))
        .header("X-Title", "MedGrapher")
        .send()
        .await
    {
        Ok(r) => r,
        Err(_) => return fallback_models(),
    };
    if !resp.status().is_success() {
        return fallback_models();
    }
    let json: serde_json::Value = match resp.json().await {
        Ok(j) => j,
        Err(_) => return fallback_models(),
    };

    let mut models: Vec<ModelInfo> = Vec::new();
    if let Some(items) = json["data"].as_array() {
        for m in items {
            let Some(id) = m["id"].as_str() else {
                continue;
            };
            let id = id.trim();
            if id.is_empty() || id.ends_with(":batch") {
                continue;
            }
            let name = m["name"].as_str().unwrap_or(id).trim();
            let name = if name.is_empty() { id } else { name };
            if !models.iter().any(|x| x.id == id) {
                models.push(ModelInfo {
                    id: id.to_string(),
                    name: name.to_string(),
                });
            }
        }
    }

    if models.is_empty() {
        fallback_models()
    } else {
        models
    }
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

    // ВБД для RAG грузим один раз при старте (из встроенных байтов).
    let rag = load_rag();

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
        .manage(rag)
        .invoke_handler(tauri::generate_handler![
            query_openrouter,
            rag_search,
            get_reference_data,
            save_history,
            get_history,
            clear_old_history,
            get_history_by_id,
            get_procedures_table,
            get_lfk_table,
            get_available_models
        ])
        .run(tauri::generate_context!())
    {
        eprintln!("Ошибка запуска приложения: {e}");
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenize_and_normalize() {
        assert_eq!(normalize("  Коксартроз, 2 ст.! "), "коксартроз 2 ст");
        assert_eq!(tokenize("Инфаркт головного мозга"), vec!["инфаркт", "головного", "мозга"]);
        assert_eq!(compact("A01.0"), "a010");
    }

    #[test]
    fn api_key_normalization() {
        assert_eq!(normalize_api_key("  sk-or-abc  "), "sk-or-abc");
        assert_eq!(normalize_api_key("Bearer sk-or-abc"), "sk-or-abc");
        assert_eq!(normalize_api_key("bearer   sk-or-abc"), "sk-or-abc");
        assert_eq!(normalize_api_key("\"sk-or-abc\""), "sk-or-abc");
        assert_eq!(normalize_api_key("'sk-or-abc'"), "sk-or-abc");
        assert_eq!(normalize_api_key("  "), "");
        // токен Qwen не должен пройти как ключ OpenRouter
        assert!(!normalize_api_key("eyJhbGciOiJIUzI1NiJ9").starts_with("sk-or-"));
    }

    #[test]
    fn rag_finds_pneumonia() {
        let store = load_rag();
        assert!(!store.diags.is_empty(), "ВБД не загрузилась");
        let cards = retrieve(&store, "внебольничная пневмония", 5);
        assert!(!cards.is_empty(), "ничего не найдено");
        assert!(
            cards.iter().any(|c| c.diagnosis.to_lowercase().contains("пневмон")),
            "нет пневмонии в топе: {:?}",
            cards.iter().map(|c| &c.diagnosis).collect::<Vec<_>>()
        );
        // у карточек должны быть коды МКФ и шкалы
        assert!(cards.iter().any(|c| !c.mcf.is_empty()));
        let ctx = cards_to_context(&cards);
        assert!(ctx.contains("МКФ:"));
    }

    #[test]
    fn rag_finds_by_mkb_code() {
        let store = load_rag();
        let cards = retrieve(&store, "J12", 3);
        assert!(!cards.is_empty(), "по коду J12 ничего не найдено");
        assert!(cards.iter().any(|c| c.mkb.eq_ignore_ascii_case("J12")));
    }

    #[test]
    fn rag_handles_multiple_fragments() {
        let store = load_rag();
        let cards = retrieve(&store, "коксоартроз; инфаркт головного мозга", 8);
        assert!(cards.len() >= 2, "ожидалось ≥2 карточки, получено {}", cards.len());
    }

    #[test]
    fn filters_unknown_procedures() {
        let mut physio = HashSet::new();
        physio.insert(normalize("Магнитотерапия общая"));
        physio.insert(normalize("Электрофорез"));
        let mut lfk = HashSet::new();
        lfk.insert(normalize("Дыхательная гимнастика"));

        let md = "| Клинический диагноз | Код МКФ | Процедуры Физио | Противопоказания Физио | ЛФК | Противопоказания ЛФК |\n\
|---|---|---|---|---|---|\n\
| Инсульт | b440 | Магнитотерапия общая, Электросон | нет | Дыхательная гимнастика, Аквааэробика | нет |\n";
        let out = filter_markdown_procedures(md, &physio, &lfk);
        assert!(out.contains("Магнитотерапия общая"), "разрешённая осталась: {out}");
        assert!(!out.contains("Электросон"), "выдуманная удалена: {out}");
        assert!(out.contains("Дыхательная гимнастика"), "ЛФК осталась: {out}");
        assert!(!out.contains("Аквааэробика"), "ЛФК выдуманная удалена: {out}");
        // противопоказания не трогаем
        assert!(out.contains("Противопоказания Физио"));

        // таблица 3: Клинический диагноз | Код МКФ | ЛФК | Физио
        let md2 = "| Клинический диагноз | Код МКФ | ЛФК | Физио |\n|---|---|---|---|\n\
| Инсульт | b440 | Аквааэробика | Электросон |\n";
        let out2 = filter_markdown_procedures(md2, &physio, &lfk);
        assert!(out2.contains("| - | - |"), "обе колонки стали '-': {out2}");
    }

    #[test]
    fn reference_data_loaded() {
        let store = load_rag();
        assert!(!store.procedures.is_empty(), "процедуры не загружены");
        assert!(!store.contra_full.is_empty(), "противопоказания не загружены");
        assert!(!store.proc_icf.is_empty(), "МКФ-таблицы не загружены");
        assert!(!store.documents.is_empty(), "документы не загружены");
    }

    /// Golden-тест: точный RAG-контекст для фиксированного запроса.
    /// Перегенерация: UPDATE_GOLDEN=1 cargo test --lib golden_rag_context
    #[test]
    fn golden_rag_context() {
        let store = load_rag();
        let cards = retrieve(&store, "внебольничная пневмония", 2);
        let ctx = cards_to_context(&cards);
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/golden/rag_context_pneumonia.txt");
        if std::env::var("UPDATE_GOLDEN").is_ok() {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &ctx).unwrap();
            return;
        }
        let expected = std::fs::read_to_string(&path).unwrap_or_else(|_| {
            panic!("нет golden-файла {}; запусти UPDATE_GOLDEN=1 cargo test --lib golden_rag_context", path.display())
        });
        assert_eq!(expected, ctx);
    }
}
