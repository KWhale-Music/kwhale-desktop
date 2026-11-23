# KWhale Desktop — Backend Issues

Баги и находки по backend-ревью (2026-11-24). Крейты: `kwhale-core`, `kwhale-audio`, `kwhale-launcher`.

**Статус:** `FIXED` — исправлено + покрыто unit-тестом (коммиты `84efe39`, `e1e5778`, `f806d06`). `OPEN` — требует доработки.

---

## FIXED (исправлено и покрыто тестами)

| # | Серьёзность | Где | Проблема | Коммит |
|---|-------------|-----|----------|--------|
| 1 | 🔴 High | `network.rs` | Redirect-политика сверяла только host → даунгрейд https→http и кросс-порт переходы проходили | `84efe39` |
| 2 | 🔴 High | `network.rs` | Allowlist редиректов — протухший снапшот, `reconfigure` игнорировался | `84efe39` |
| 3 | 🔴 High | `app.rs` | `reconfigure_api` сохранял URL до валидации → ядовитый URL «кирпичил» следующий старт | `84efe39` |
| 4 | 🔴 High | `cache_proxy.rs` | Ошибки записи в кэш игнорировались → усечённый трек коммитился в кэш навсегда | `84efe39` |
| 5 | 🟠 Security | `storage.rs` | Токены/`llm_api_key` писались с правами 0644 (world-readable) | `e1e5778` |
| 6 | 🟠 Bug | `models.rs` | `ProposedMetadata` без `rename_all=camelCase` → `coverUrl` всегда `None` | `e1e5778` |
| 7 | 🟠 Bug | `events.rs` | `flush` удалял отправленные pop-by-count → терял неотправленные при overflow | `e1e5778` |
| 8 | 🟠 Bug | `downloads.rs` | Общий `{id}.part` → cancel одной попытки удалял файл другой | `e1e5778` |
| 9 | 🟠 Bug | `engine.rs`, `mpris.rs`, `storage.rs` | `file://` без percent-encoding → ломалось на пробелах/`#`/не-ASCII в пути | `f806d06` |
| 10 | 🟡 Robust | `cache_proxy.rs` | `end + 1` overflow в `should_cache_response` (u64::MAX) | `f806d06` |
| 11 | 🟡 Security | `cache_proxy.rs` | Bad Gateway отдавал upstream-код (401 и т.п.) клиенту | `f806d06` |

Тесты: `kwhale-core` 23, `kwhale-audio` 4 — все зелёные.

---

## OPEN (требует доработки)

### Безопасность / корректность — Medium

- **`network.rs` + `deep_link.rs` — неэкранированные сегменты пути.** `format!("/tracks/{id}/stream")` и id из deep-link не percent-кодируются; id с `/?#` меняет путь запроса. *Fix:* `path_segments_mut().push(id)` или валидировать id на `[A-Za-z0-9._-]` на границе `DeepLink`.
- **`storage.rs` (`TokenStore::in_memory`) — не in-memory.** Пишет токены в `/tmp/kwhale-mem-*.json` (world-readable). *Fix:* флаг `disk: bool` (пропускать `write_atomic`), либо переименовать в `temp_file`.
- **`network.rs` (`fetch_image_bytes`, `refresh_blocking`, `is_auth_route`).** `fetch_image_bytes` теряет тело ошибки (`""`); `refresh_blocking` — async вопреки имени; `is_auth_route` матчит `contains("/auth/")` не точно. *Fix:* передавать `&text`; переименовать; `starts_with("/v1/auth/")`.
- **`storage.rs` (`dir_entries_by_mtime`) — мёртвый фильтр `.tmp`.** `extension()` у `<name>.tmp.<uuid>` даёт uuid, фильтр не срабатывает; остатки `.tmp` попадают в LRU. *Fix:* матчить `file_name().contains(".tmp.")`.

### Корректность / блокировки — Medium

- **`downloads.rs` (`run_download`) + `engine.rs` — блокирующий `duration_probe`.** GStreamer-Discoverer (до 5 с) выполняется синхронно на tokio-worker при завершении загрузки. *Fix:* обернуть в `tokio::task::spawn_blocking` на стороне вызова (`downloads.rs`) или сделать probe async.
- **`mpris.rs` + `kwhale-launcher` — гонка single-instance.** `name_has_owner` → `exec` (TOCTOU); проигравший по имени лишь логирует warn и продолжает вторым инстансом. *Fix:* сделать приобретение имени фатальным для проигравшего (raise/open-uri у победителя + выход), launcher-проверку оставить best-effort.
- **`cache_proxy.rs` (`QueuePrefetcher::schedule`) — гонка.** `active` меняется внутри spawn-тасков: старый `schedule()` может отменить более новый; завершённый entry не очищается → повторный prefetch становится no-op. *Fix:* обновлять/cancel `active` синхронно в `schedule()` до spawn (`std::sync::Mutex`), очищать `*g = None` по завершении.
- **`engine.rs` (`handle_eos`) — гонка.** Действует по снапшоту `st.index`; конкурентная мутация очереди ведёт не на тот трек. *Fix:* вычислять/применять advance под одним `state.write()`.

### MPRIS / D-Bus — Medium

- **`mpris.rs` (`spawn_mpris`) — volume/muted не эмитят `PropertiesChanged`.** Фильтр изменений их не отслеживает; `volume_changed` не вызывается. *Fix:* трекать `last_volume`/`last_muted`/`last_duration`, добавить в условие, вызывать `volume_changed`/`can_seek_changed`.
- **`mpris.rs` (`spawn_mpris`) — обложка блокирует сигналы.** `get_or_fetch_image(...).await` инлайн в цикле → сигналы `PlaybackStatus`/`metadata` ждут сетевой запрос. *Fix:* `tokio::spawn` загрузки, эмитить `metadata_changed` по завершении.
- **`engine.rs` (`spawn_bus_and_ticker`) — bus-поллинг 250 мс + утечка таска.** `bus.pop()` в бесконечном таске: до 250 мс латентности EOS/ошибки, таск не завершается (утечка `AudioEngine`). *Fix:* `bus.stream()` (event-driven) + хранить `JoinHandle`/`CancellationToken` и abort на shutdown.

### Производительность — Medium

- **`engine.rs` (`publish`) — клонирует всю очередь 4 Гц.** `PlaybackState` с `Vec<UiTrack>` (17 полей/трек) клонируется на каждое обновление позиции. *Fix:* `queue: Arc<Vec<UiTrack>>` (клон = refcount), либо отдельный лёгкий канал для позиции.
- **`engine.rs` (`set_volume`/`set_muted`, события) — синхронный диск на UI-потоке.** Каждый тик громкости пишет settings JSON; события пишут файл инлайн. *Fix:* debounce записи (idle-N-mс), `persist_best_effort` через очередь.
- **`downloads.rs` / `storage.rs` — тихие no-ops вне tokio-runtime.** `Handle::try_current()` → `return` без ошибки: `enqueue` оставляет `Queued` навсегда, `save` пропускает keyring. *Fix:* `tracing::warn!`; `enqueue` ставить `Failed`/возвращать `Result`.

### Мелочь / чистка — Low

- **`Cargo.toml` (`kwhale-core`) — неиспользуемые зависимости:** `sha2`, `tracing`, `bytes` (нет ссылок в `src/`). Кучевой `#![allow(clippy::collapsible_if)]`. *Fix:* удалить неиспользуемое (или реально задействовать `tracing` для глушеных ошибок), сузить allow.
- **`cache_proxy.rs` (`handle_connection`) — unbounded `read_line`.** Строка запроса/заголовки без лимита (memory-exhaustion для локального процесса). *Fix:* кап 8 КиБ (reject 431/400).
- **`engine.rs` (`uri_for_track`/логи) — nonce анти-CSRF утекает в логи.** `info!("loading track ... uri={}")` печатает `http://127.0.0.1:<port>/track/<nonce>/...`. *Fix:* логировать `track.remote_id`.
- **`cache_proxy.rs` / `engine.rs` — мёртвый код и пред-локализованная строка.** `let _ = &mut head;`, `split('?').next().unwrap_or(...)` с недостижимым fallback, `PlaybackState.error` с русской строкой (UI не может локализовать). *Fix:* удалить; `error` → структурированный код.
- **`cache_proxy.rs` (`serve_local_file` и др.) — синхронный `fs` на tokio-worker.** `remove_file`, `commit_temp_file` (O(n) LRU) инлайн; при исчезновении файла между `get_cached_path` и `File::open` соединение рвётся без ответа. *Fix:* `spawn_blocking`; отдавать 404/500.
- **`engine.rs` — нет gapless при playbin3.** Каждая смена трека рушит pipeline в `Null`; механизм `about-to-finish` не используется. *Fix (фича, не баг):* задавать next-URI на `about-to-finish` для seamless-перехода.

---

## Заметки по окружению сборки/тестов

Системных openssl/gstreamer dev-заголовков нет, flathub недоступен, sudo нет. Тесты гонялись против заголовков из KDE Sdk + host-библиотек (чистая temp-директория ссылок без Sdk-libc). Команда:

```bash
SDK=$HOME/.local/share/flatpak/runtime/org.kde.Sdk/x86_64/6.11/active/files
export PKG_CONFIG_PATH=/tmp/opencode/gstpc OPENSSL_LIB_DIR=/tmp/opencode/ssllib OPENSSL_INCLUDE_DIR=$SDK/include
export CFLAGS="-I$SDK/lib/x86_64-linux-gnu/include -I$SDK/include/x86_64-linux-gnu"
cargo test -p kwhale-core -p kwhale-audio
```
