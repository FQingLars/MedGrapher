#!/usr/bin/env python3
"""Импорт справочных документов MedGrapher в RAG-БД.

Источники (в src-tauri/tables):
  - «Таблица физио общая.docx» → procedures (аппарат, параметры, время, курс)
  - «ИИ3.docx»                 → contra_full (абсолютные/относительные противопоказания)
  - «ИИ2.docx»                 → proc_icf (процедура ↔ коды МКФ, показания, эффект)
  - «МР Инсульт.pdf»           → documents (клинический протокол)

Также перегенерирует Процедуры.csv из docx (источник истины — docx).

Запуск: python3 tools/import_reference.py
"""
import csv
import re
import sqlite3
import subprocess
import sys
import zipfile
from pathlib import Path
from xml.etree import ElementTree as ET

ROOT = Path(__file__).resolve().parent.parent
TABLES = ROOT / "src-tauri" / "tables"
DB = TABLES / "medgrapher_rag.db"
W = "{http://schemas.openxmlformats.org/wordprocessingml/2006/main}"

ZW = dict.fromkeys(map(ord, "\u200b\u200c\u200d\ufeff\u00a0"), None)
SPACES = re.compile(r"\s+")


def clean(s: str) -> str:
    if not s:
        return ""
    s = s.translate(ZW)
    s = s.replace("\u00ad", "")
    s = SPACES.sub(" ", s)
    return s.strip()


def cell_text(tc) -> str:
    return clean(" ".join(t.text for t in tc.iter(W + "t") if t.text))


def docx_rows(path: Path):
    root = ET.fromstring(zipfile.ZipFile(path).read("word/document.xml"))
    for tbl in root.iter(W + "tbl"):
        rows = []
        for tr in tbl.findall(W + "tr"):
            rows.append([cell_text(tc) for tc in tr.findall(W + "tc")])
        yield rows


def norm_name(s: str) -> str:
    s = s.lower().replace("ё", "е")
    s = re.sub(r"\(.*?\)", " ", s)          # убрать скобки
    s = re.sub(r"[^a-zа-я0-9]+", " ", s)
    return SPACES.sub(" ", s).strip()


def is_icf_code(s: str) -> bool:
    return bool(re.fullmatch(r"[bdse]\s?\d{2,4}", s.strip(), re.I))


def join_unique(parts):
    seen, out = set(), []
    for p in parts:
        p = clean(p)
        if not p or p == "-":
            continue
        if p.lower() in seen:
            continue
        seen.add(p.lower())
        out.append(p)
    return "; ".join(out)


# ── Парсинг таблиц ──────────────────────────────────────────────────────────
def parse_fto():
    """Таблица физио общая: № | Название | Аппарат | Параметры | Время | Курс."""
    path = TABLES / "Таблица физио общая.docx"
    out = []
    for rows in docx_rows(path):
        for r in rows[1:]:
            r = (r + [""] * 6)[:6]
            name = r[1]
            if not name:
                continue  # continuation не ожидается (все строки имеют имя)
            out.append({
                "name": name,
                "apparatus": r[2],
                "parameters": r[3],
                "time": r[4],
                "course": r[5],
            })
    return out


def parse_ii3():
    """ИИ3: № | Название | Абсолютные | Относительные."""
    path = TABLES / "ИИ3.docx"
    out = []
    for rows in docx_rows(path):
        cur = None
        for r in rows[1:]:
            r = (r + [""] * 4)[:4]
            name = r[1]
            if name:
                cur = {"procedure": name, "absolute": [], "relative": []}
                out.append(cur)
            if cur is None:
                continue
            if r[2]:
                cur["absolute"].append(r[2])
            if r[3]:
                cur["relative"].append(r[3])
        for e in out:
            e["absolute"] = join_unique(e["absolute"])
            e["relative"] = join_unique(e["relative"])
    return out


def parse_ii2():
    """ИИ2: № | Процедура | Код МКФ | Показания | Обоснование | Эффект."""
    path = TABLES / "ИИ2.docx"
    out = []
    for rows in docx_rows(path):
        cur = None
        for r in rows[1:]:
            r = (r + [""] * 6)[:6]
            name = r[1]
            if name:
                cur = {"procedure": name, "icf": [], "indications": [],
                       "rationale": [], "effect": []}
                out.append(cur)
            if cur is None:
                continue
            # коды МКФ — только если похоже на код (в continuation-строках
            # биоптрона в этой колонке названия цветов — их игнорируем)
            if r[2]:
                cur["icf"].extend(t for t in re.split(r"\s+", r[2]) if is_icf_code(t))
            if r[3]:
                cur["indications"].append(r[3])
            if r[4]:
                cur["rationale"].append(r[4])
            if r[5]:
                cur["effect"].append(r[5])
        for e in out:
            e["icf"] = " ".join(dict.fromkeys(e["icf"]))
            e["indications"] = join_unique(e["indications"])
            e["rationale"] = join_unique(e["rationale"])
            e["effect"] = join_unique(e["effect"])
    return out


def parse_pdf():
    """МР Инсульт.pdf → текст клинического протокола."""
    path = TABLES / "МР Инсульт.pdf"
    if not path.exists() or not subprocess.run(["which", "pdftotext"],
                                               capture_output=True).returncode == 0:
        return None
    res = subprocess.run(["pdftotext", "-layout", str(path), "-"],
                         capture_output=True, text=True, encoding="utf-8")
    lines = [SPACES.sub(" ", ln).strip() for ln in res.stdout.splitlines()]
    text = "\n".join(ln for ln in lines if ln)
    title = "Клинический протокол «Медицинская реабилитация пациентов с инсультом (взрослое население)»"
    return {"title": title, "source": path.name, "content": text}


# ── Запись в БД ─────────────────────────────────────────────────────────────
def main():
    fto = parse_fto()
    ii3 = parse_ii3()
    ii2 = parse_ii2()
    pdf = parse_pdf()
    print(f"ФТО: {len(fto)} процедур; ИИ3: {len(ii3)}; ИИ2: {len(ii2)}; PDF: {'да' if pdf else 'нет'}")

    con = sqlite3.connect(DB)
    cur = con.cursor()
    cur.executescript("""
    DROP TABLE IF EXISTS procedures;
    DROP TABLE IF EXISTS contra_full;
    DROP TABLE IF EXISTS proc_icf;
    DROP TABLE IF EXISTS documents;
    CREATE TABLE procedures (
      id INTEGER PRIMARY KEY, name TEXT NOT NULL, apparatus TEXT,
      parameters TEXT, time_min TEXT, course_days TEXT);
    CREATE TABLE contra_full (
      procedure TEXT PRIMARY KEY, absolute TEXT, relative TEXT);
    CREATE TABLE proc_icf (
      procedure TEXT PRIMARY KEY, icf_codes TEXT, indications TEXT,
      rationale TEXT, effect TEXT);
    CREATE TABLE documents (
      id INTEGER PRIMARY KEY, title TEXT NOT NULL, source TEXT, content TEXT);
    """)

    cur.executemany(
        "INSERT INTO procedures VALUES (?,?,?,?,?,?)",
        [(i, p["name"], p["apparatus"], p["parameters"], p["time"], p["course"])
         for i, p in enumerate(fto, 1)],
    )
    cur.executemany(
        "INSERT INTO contra_full VALUES (?,?,?)",
        [(e["procedure"], e["absolute"], e["relative"]) for e in ii3],
    )
    cur.executemany(
        "INSERT INTO proc_icf VALUES (?,?,?,?,?)",
        [(e["procedure"], e["icf"], e["indications"], e["rationale"], e["effect"])
         for e in ii2],
    )
    if pdf:
        cur.execute("INSERT INTO documents (title, source, content) VALUES (?,?,?)",
                    (pdf["title"], pdf["source"], pdf["content"]))
    con.commit()

    n = cur.execute("SELECT count(*) FROM procedures").fetchone()[0]
    print(f"procedures: {n}; contra_full: {len(ii3)}; proc_icf: {len(ii2)}; documents: {1 if pdf else 0}")

    # ── Перегенерация Процедуры.csv из docx (источник истины) ──
    csv_path = TABLES / "Процедуры.csv"
    with open(csv_path, "w", encoding="utf-8", newline="") as f:
        w = csv.writer(f, delimiter=";")
        w.writerow(["Идентификатор_процедуры", "Название", "Устройство",
                    "Параметры", "Время", "Курс_в_днях"])
        for i, p in enumerate(fto, 1):
            w.writerow([i, p["name"], p["apparatus"], p["parameters"],
                        p["time"], p["course"]])
    print("Процедуры.csv перегенерирован:", csv_path)

    con.execute("VACUUM")
    con.close()
    print("Готово:", DB)


if __name__ == "__main__":
    sys.exit(main())