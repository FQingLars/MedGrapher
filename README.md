# MedGrapher

Desktop-приложение (Tauri + SvelteKit) для подбора кодов МКФ, процедур Физио/ЛФК
и шкал оценки по клиническим диагнозам. Работает через **OpenRouter**
(OpenAI-совместимый API) и использует **RAG** — ретрив релевантных карточек из
встроенной медицинской ВБД (собрана из проекта
[MedGrapher2](https://github.com/FQingLars/MedGrapher2)).

## Как это работает

1. Пользователь вводит клинические диагнозы и выбирает шкалы.
2. `query_openrouter` (Rust, `src-tauri/src/lib.rs`):
   - **RAG-ретрив**: для каждого диагноза ищет top-N релевантных карточек во
     встроенной SQLite-ВБД (`src-tauri/tables/medgrapher_rag.db`, ~1.7 МБ:
     3021 диагноз, 1495 кодов МКФ, противопоказания Физио/ЛФК). Поиск —
     лексический (взвешенное пересечение токенов + МКБ-коды + фразовое вхождение),
     без внешних процессов и ONNX-моделей.
   - Собирает компактный контекст из карточек (коды МКФ с названиями, шкалы,
     процедуры с противопоказаниями) и подставляет его в промпт.
   - Отправляет запрос в OpenRouter `POST /api/v1/chat/completions` (Bearer-ключ,
     SSE-стриминг), парсит `choices[].delta.content`.
3. Ответ парсится на фронтенде (`parseResult`) и рендерится как Markdown-таблицы.

## Справочник

Панель «📚 Справочник» (кнопка в шапке) показывает справочные материалы из
`src-tauri/tables`:

- **Процедуры** — аппаратура, параметры, время, курс (из «Таблица физио общая.docx»);
- **Противопоказания** — абсолютные и относительные (из «ИИ3.docx»);
- **Процедуры ↔ МКФ** — коды МКФ, показания, ожидаемый эффект (из «ИИ2.docx»);
- **Протокол** — клинический протокол по инсульту (из «МР Инсульт.pdf»).

Данные загружаются в `medgrapher_rag.db` (таблицы `procedures`, `contra_full`,
`proc_icf`, `documents`) и дополнительно обогащают RAG-контекст.

Пересобрать справочник из docx/pdf:

```bash
python3 tools/import_reference.py   # нужен pdftotext (poppler-utils)
```

## Настройки

- **API-ключ OpenRouter**: [openrouter.ai/keys](https://openrouter.ai/keys) → `sk-or-...`
- **Модель**: реестр моделей — добавляйте ID с
  [openrouter.ai/models](https://openrouter.ai/models) (например,
  `anthropic/claude-3.5-sonnet`) и выбирайте активную. Список хранится в настройках.

## Разработка

```bash
npm install
npm run tauri dev      # запуск приложения
npm run check          # svelte-check
cd src-tauri && cargo test --lib   # юнит + golden-тесты RAG
```

Golden-тест RAG: `src-tauri/tests/golden/rag_context_pneumonia.txt`.
Перегенерация: `UPDATE_GOLDEN=1 cargo test --lib golden_rag_context`.