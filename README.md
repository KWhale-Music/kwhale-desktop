# KWhale Desktop (`org.dueattendant149.Kwhale`)

Нативный кроссплатформенный десктопный клиент для self-hosted музыкального сервера **KWhale** (`/v1`), построенный на едином Rust-ядре и интерфейсе **Material You 3** (`kwhale-ui` / Slint), адаптирующемся под окружение рабочего стола (**GNOME**, **KDE Plasma**, нейтральные WM) и **Windows**:

1. **`kwhale-ui`** — единый интерфейс в стиле Material You 3 (тональные поверхности, 129 акцентов + системный акцент DE/ОС, шрифты `VK Sans Display` / `VK Sans Text`, адаптивный верхний бар и метрики контролов под GNOME / KDE / Windows).
2. **`kwhale` (`kwhale-launcher`)** — единая точка входа (`Exec=kwhale %u`), запускающая `kwhale-ui` (с поддержкой `--ui=slint|auto`) и пересылающая `kwhale://` deep-link ссылки в уже запущенный экземпляр по D-Bus (`org.dueattendant149.Kwhale`).

## Структура Workspace

```
kwhale-desktop/
├── Cargo.toml
├── assets/
│   ├── fonts/            # Шрифты VK Sans Display и VK Sans Text
│   └── icons/            # Иконки приложения (64..512px)
├── crates/
│   ├── kwhale-core/      # REST API /v1, single-flight refresh, кэши, реакции, загрузки, 129 акцентов + PlatformStyle
│   ├── kwhale-audio/     # GStreamer playbin3, localhost Range/cache прокси, prefetcher (5 треков), MPRIS2
│   ├── kwhale-ui/        # Единый Adaptive Material You 3 интерфейс на Slint (Linux DE + Windows)
│   └── kwhale-launcher/  # Точка входа и D-Bus активация
└── packaging/flatpak/
    ├── org.dueattendant149.Kwhale.yml
    ├── org.dueattendant149.Kwhale.desktop
    ├── org.dueattendant149.Kwhale.metainfo.xml
    └── build-flatpak.sh
```

## Сборка и установка Flatpak

Требуются установленные `org.flatpak.Builder`, `org.kde.Platform//6.11`, `org.kde.Sdk//6.11` и `org.freedesktop.Sdk.Extension.rust-stable//25.08`:

```bash
./packaging/flatpak/build-flatpak.sh
```

Скрипт:
1. Подготавливает чистый snapshot исходников и выполняет `cargo vendor --locked`.
2. Собирает Rust-бинарники `kwhale` и `kwhale-ui` поверх `org.kde.Platform//6.11`.
3. Экспортирует однофайловый бандл `target/kwhale-desktop.flatpak` и устанавливает его в профиль пользователя (`flatpak install --user`).

## Запуск

```bash
# Запуск Adaptive Material You 3 клиента (автоопределение GNOME / KDE / Windows)
flatpak run org.dueattendant149.Kwhale

# Открытие deep-link ссылки
flatpak run org.dueattendant149.Kwhale kwhale://track/<id>
```

## Горячие клавиши

- `Space` — Воспроизведение / Пауза
- `Ctrl+Left` / `Ctrl+Right` — Предыдущий / Следующий трек
- `Shift+Left` / `Shift+Right` — Перемотка `-5 сек` / `+5 сек`
- `Ctrl+Up` / `Ctrl+Down` — Громкость `+5%` / `-5%`
- `M` — Выключить / включить звук
- `Ctrl+F` / `Ctrl+K` — Фокус на строку поиска
- `Ctrl+L` — Боковая панель синхронизированного текста песни (караоке)
- `Ctrl+Q` — Боковая панель очереди воспроизведения
- `F11` — Полноэкранный режим `Now Playing` с размытым фоном обложки и караоке
