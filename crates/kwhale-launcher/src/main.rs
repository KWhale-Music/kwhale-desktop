#![allow(clippy::collapsible_if)]

use kwhale_core::{AppPaths, SettingsStore, UiBackend};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

pub fn detect_backend(
    cli_ui: Option<UiBackend>,
    env_ui: Option<&str>,
    settings_ui: UiBackend,
    xdg_desktop: Option<&str>,
) -> UiBackend {
    if let Some(ui) = cli_ui {
        if ui != UiBackend::Auto {
            return ui;
        }
    }
    if let Some(raw_env) = env_ui {
        let parsed = UiBackend::from_str_lossy(raw_env);
        if parsed != UiBackend::Auto {
            return parsed;
        }
    }
    if settings_ui != UiBackend::Auto {
        return settings_ui;
    }
    let desktop = xdg_desktop.unwrap_or_default().to_ascii_uppercase();
    if desktop.contains("KDE") || desktop.contains("PLASMA") || desktop.contains("LXQT") {
        UiBackend::Qt
    } else {
        // GNOME and neutral desktops default to libadwaita per plan.
        UiBackend::Adw
    }
}

fn find_sibling_bin(name: &str) -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join(name);
            if candidate.exists() {
                return candidate;
            }
        }
    }
    PathBuf::from(name)
}

async fn try_forward_to_running_instance(deep_links: &[String]) -> bool {
    let Ok(conn) = zbus::Connection::session().await else {
        return false;
    };
    let Ok(dbus) = zbus::fdo::DBusProxy::new(&conn).await else {
        return false;
    };
    let Ok(name) = "org.dueattendant149.Kwhale".try_into() else {
        return false;
    };
    if !dbus.name_has_owner(name).await.unwrap_or(false) {
        return false;
    }

    if deep_links.is_empty() {
        conn.call_method(
            Some("org.dueattendant149.Kwhale"),
            "/org/dueattendant149/Kwhale",
            Some("org.dueattendant149.Kwhale.Control"),
            "Raise",
            &(),
        )
        .await
        .is_ok()
    } else {
        let mut ok = false;
        for uri in deep_links {
            if conn
                .call_method(
                    Some("org.dueattendant149.Kwhale"),
                    "/org/dueattendant149/Kwhale",
                    Some("org.dueattendant149.Kwhale.Control"),
                    "OpenUri",
                    &(uri.as_str()),
                )
                .await
                .is_ok()
            {
                ok = true;
            }
        }
        ok
    }
}

fn main() {
    let mut cli_ui: Option<UiBackend> = None;
    let mut pass_args: Vec<String> = Vec::new();
    let mut deep_links: Vec<String> = Vec::new();

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--help" || arg == "-h" {
            println!(
                "KWhale Desktop Launcher\n\nИспользование:\n  kwhale [--ui=auto|adw|qt] [kwhale://...]\n"
            );
            return;
        } else if arg == "--version" || arg == "-V" {
            println!("kwhale {}", env!("CARGO_PKG_VERSION"));
            return;
        } else if let Some(val) = arg.strip_prefix("--ui=") {
            cli_ui = Some(UiBackend::from_str_lossy(val));
        } else if arg == "--ui" {
            if let Some(val) = args.next() {
                cli_ui = Some(UiBackend::from_str_lossy(&val));
            }
        } else {
            if arg.starts_with("kwhale://") {
                deep_links.push(arg.clone());
            }
            pass_args.push(arg);
        }
    }

    // If an instance is already running and no explicit --ui switch was requested,
    // forward deep links / raise the existing window over D-Bus.
    if cli_ui.is_none() {
        if let Ok(rt) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            if rt.block_on(try_forward_to_running_instance(&deep_links)) {
                return;
            }
        }
    }

    let paths = AppPaths::resolve();
    let settings = SettingsStore::load(&paths).snapshot();
    let env_ui = std::env::var("KWHALE_UI").ok();
    let xdg_desktop = std::env::var("XDG_CURRENT_DESKTOP").ok();
    let chosen = detect_backend(
        cli_ui,
        env_ui.as_deref(),
        settings.ui_backend,
        xdg_desktop.as_deref(),
    );
    let target_bin = match chosen {
        UiBackend::Qt => "kwhale-qt",
        UiBackend::Adw | UiBackend::Auto => "kwhale-adw",
    };

    let exe = find_sibling_bin(target_bin);
    let err = Command::new(&exe).args(&pass_args).exec();
    eprintln!("Не удалось запустить {}: {err}", exe.display());
    std::process::exit(1);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_detection_priority_order() {
        // 1. CLI wins over everything
        assert_eq!(
            detect_backend(
                Some(UiBackend::Qt),
                Some("adw"),
                UiBackend::Adw,
                Some("GNOME")
            ),
            UiBackend::Qt
        );
        // 2. KWHALE_UI env wins over settings & XDG_CURRENT_DESKTOP
        assert_eq!(
            detect_backend(None, Some("qt"), UiBackend::Adw, Some("GNOME")),
            UiBackend::Qt
        );
        // 3. Settings wins over XDG_CURRENT_DESKTOP
        assert_eq!(
            detect_backend(None, None, UiBackend::Qt, Some("GNOME")),
            UiBackend::Qt
        );
        // 4. XDG_CURRENT_DESKTOP maps KDE/Plasma/LXQt -> Qt, GNOME & neutral -> Adw
        assert_eq!(
            detect_backend(None, None, UiBackend::Auto, Some("KDE")),
            UiBackend::Qt
        );
        assert_eq!(
            detect_backend(None, None, UiBackend::Auto, Some("ubuntu:GNOME")),
            UiBackend::Adw
        );
        assert_eq!(
            detect_backend(None, None, UiBackend::Auto, Some("sway")),
            UiBackend::Adw
        );
        assert_eq!(
            detect_backend(None, None, UiBackend::Auto, None),
            UiBackend::Adw
        );
    }
}
