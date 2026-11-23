# KWhale Desktop (`org.dueattendant149.Kwhale`)

Нативный десктопный клиент для self-hosted музыкального сервера **KWhale** (`/v1`), поставляемый в **едином All-in-One Flatpak пакете** с двумя нативными графическими движками и общим Rust-ядром:

1. **`kwhale-adw`** — нативный интерфейс для **GNOME** и нейтральных окружений на базе **GTK 4.18 + libadwaita 1.7** (`gtk4-rs` / `libadwaita-rs`).
2. **`kwhale-qt`** — нативный интерфейс для **KDE Plasma / LXQt** на базе **Qt 6.11 Quick / QML + Kirigami** (`cxx-qt`).
3. **`kwhale` (`kwhale-launcher`)** — единая точка входа (`Exec=kwhale %u`), автоматически выбирающая нативный движок по `XDG_CURRENT_DESKTOP` (с возможностью переопределения через `--ui=adw|qt|auto`, переменную `KWHALE_UI` или переключатель в настройках) и пересылающая `kwhale://` deep-link ссылки в уже запущенный экземпляр по D-Bus (`org.dueattendant149.Kwhale`).

## Структура Workspace

```
kwhale-desktop/
├── Cargo.toml
├── assets/
│   ├── fonts/            # Шрифты VK Sans Display и VK Sans Text
│   └── icons/            # Иконки приложения (64..512px)
├── crates/
│   ├── kwhale-core/      # REST API /v1, single-flight refresh, кэши, реакции, загрузки, 129 акцентов
│   ├── kwhale-audio/     # GStreamer playbin3, localhost Range/cache прокси, prefetcher (5 треков), MPRIS2
│   ├── kwhale-adw/       # Нативный GTK4 + libadwaita десктопный интерфейс
│   ├── kwhale-qt/        # Нативный Qt6 Quick / QML + Kirigami десктопный интерфейс (cxx-qt)
│   └── kwhale-launcher/  # Автовыбор UI-бэкенда и D-Bus активация
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
2. Собирает `libsass`, `sassc`, `gtk-4.18.6`, `libadwaita-1.7.12` и Rust-бинарники `kwhale`, `kwhale-adw`, `kwhale-qt` поверх `org.kde.Platform//6.11`.
3. Экспортирует однофайловый бандл `target/kwhale-desktop.flatpak` и устанавливает его в профиль пользователя (`flatpak install --user`).

## Запуск

```bash
# Автовыбор движка по текущему окружению рабочего стола
flatpak run org.dueattendant149.Kwhale

# Принудительный запуск GTK4 + libadwaita
flatpak run org.dueattendant149.Kwhale --ui=adw

# Принудительный запуск Qt6 Quick + Kirigami
flatpak run org.dueattendant149.Kwhale --ui=qt

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
