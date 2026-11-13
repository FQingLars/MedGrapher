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

## Настройки

- **API-ключ OpenRouter**: [openrouter.ai/keys](https://openrouter.ai/keys) → `sk-or-...`
- **Модель**: список подтягивается из `GET /api/v1/models` (fallback — встроенный список).

## Разработка

```bash
npm install
npm run tauri dev      # запуск приложения
npm run check          # svelte-check
cd src-tauri && cargo test --lib   # юнит + golden-тесты RAG
```

Golden-тест RAG: `src-tauri/tests/golden/rag_context_pneumonia.txt`.
Перегенерация: `UPDATE_GOLDEN=1 cargo test --lib golden_rag_context`.