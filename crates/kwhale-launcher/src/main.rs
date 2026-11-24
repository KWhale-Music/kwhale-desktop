#![allow(clippy::collapsible_if)]

use kwhale_core::{AppPaths, SettingsStore, UiBackend};
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

pub fn detect_backend(
    cli_ui: Option<UiBackend>,
    env_ui: Option<&str>,
    settings_ui: UiBackend,
    _xdg_desktop: Option<&str>,
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
    UiBackend::Slint
}

fn sibling_bin_exists(name: &str) -> bool {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            return dir.join(name).exists();
        }
    }
    false
}

pub fn resolve_target_bin(chosen: UiBackend, has_sibling: impl Fn(&str) -> bool) -> &'static str {
    match chosen {
        UiBackend::Qt if has_sibling("kwhale-qt") => "kwhale-qt",
        UiBackend::Adw if has_sibling("kwhale-adw") => "kwhale-adw",
        _ => "kwhale-ui",
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
    let mut screenshot_mode = false;
    let mut pass_args: Vec<String> = Vec::new();
    let mut deep_links: Vec<String> = Vec::new();

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--help" || arg == "-h" {
            println!(
                "KWhale Desktop Launcher\n\nИспользование:\n  kwhale [--ui=auto|slint|adw|qt] [--widget] [kwhale://...]\n"
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
            if arg == "--screenshot" || arg.starts_with("--screenshot=") {
                screenshot_mode = true;
            }
            if arg.starts_with("kwhale://") {
                deep_links.push(arg.clone());
            }
            pass_args.push(arg);
        }
    }

    // If an instance is already running and no explicit --ui / --screenshot switch was requested,
    // forward deep links / raise the existing window over D-Bus.
    if cli_ui.is_none() && !screenshot_mode {
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
    let target_bin = resolve_target_bin(chosen, sibling_bin_exists);

    let exe = find_sibling_bin(target_bin);
    #[cfg(unix)]
    {
        let err = Command::new(&exe).args(&pass_args).exec();
        eprintln!("Не удалось запустить {}: {err}", exe.display());
    }
    #[cfg(not(unix))]
    {
        match Command::new(&exe).args(&pass_args).status() {
            Ok(st) => std::process::exit(st.code().unwrap_or(0)),
            Err(err) => eprintln!("Не удалось запустить {}: {err}", exe.display()),
        }
    }
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
        // 4. Auto defaults to unified Slint Adaptive UI on all desktops
        assert_eq!(
            detect_backend(None, None, UiBackend::Auto, Some("KDE")),
            UiBackend::Slint
        );
        assert_eq!(
            detect_backend(None, None, UiBackend::Auto, Some("ubuntu:GNOME")),
            UiBackend::Slint
        );
        assert_eq!(
            detect_backend(None, None, UiBackend::Auto, Some("sway")),
            UiBackend::Slint
        );
        assert_eq!(
            detect_backend(None, None, UiBackend::Auto, None),
            UiBackend::Slint
        );
    }

    #[test]
    fn falls_back_to_kwhale_ui_when_legacy_bin_missing() {
        // Simulates old persisted settings ("qt" / "adw") running in a unified kwhale-ui install
        assert_eq!(resolve_target_bin(UiBackend::Qt, |_| false), "kwhale-ui");
        assert_eq!(resolve_target_bin(UiBackend::Adw, |_| false), "kwhale-ui");
        assert_eq!(resolve_target_bin(UiBackend::Slint, |_| false), "kwhale-ui");
        assert_eq!(resolve_target_bin(UiBackend::Qt, |b| b == "kwhale-qt"), "kwhale-qt");
    }
}
