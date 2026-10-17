# Voxa: ребрендинг форка Handy, статистика и Liquid Glass оверлей

Тело PR `claude/magical-ritchie-lg9k96 → main`. Структура соответствует `.github/PULL_REQUEST_TEMPLATE.md`; раздел «Human Written Description» оставлен владельцу.

## Human Written Description

TODO (владелец): своими словами, зачем форк и что вы хотели получить.

## Related Issues/Discussions

Нет. Это личный форк; upstream-репозиторий не затрагивается.

## Community Feedback

Не применимо: изменения живут в форке `Dudu2I3/voxa`, а не в cjpais/Handy.

## Testing

- `cargo test`: 339 прошло, 18 игнорируются (52 новых).
- `bun run test:unit`, `bunx tsc --noEmit`, `bun run lint`, `bunx prettier --check .`, `bun run check:translations`.
- Playwright (`tests/voxa-preview.spec.ts`): 12 проверок и 7 генераторов скриншотов, `docs/screenshots`.
- CI `voxa-macos` на момент написания не запускался (BLOCKERS B-03). После push он должен позеленеть и выложить артефакт `Voxa-macos-arm64`.

## Screenshots/Videos

См. [docs/FINAL_REPORT.md](FINAL_REPORT.md) и `docs/screenshots/`.

## AI Assistance

- [x] AI was used. Код, тесты и документация написаны Claude (Claude Code) в автономной сессии по `PLAN.md`; проверено автоматическими тестами и скриншотами, на реальном Mac не запускалось. Владелец проверяет вручную по чек-листу из отчёта.

## Что внутри

- Ребрендинг: Voxa, `com.daniyar.voxa`, иконки, апдейтер выключен, атрибуция Handy.
- `src-tauri/src/voxa/`: статистика (токенизатор, миграции, агрегаты, кандидаты в словарь), импорт слов из Handy, настройки по умолчанию.
- `src/voxa/`: экран статистики, капсула Liquid Glass, настройки, страница `/voxa-preview`.
- Точечные правки upstream: `actions.rs`, `managers/transcription.rs`, `audio_toolkit/text.rs`, `overlay.rs`, `settings.rs`, `lib.rs`, `Sidebar.tsx`, `RecordingOverlay.tsx` (список с обоснованиями: [ARCHITECTURE_NOTES.md](ARCHITECTURE_NOTES.md)).
- Решения: [DECISIONS.md](DECISIONS.md). Блокеры: [BLOCKERS.md](BLOCKERS.md).
