use kwhale_core::{PlatformStyle, Rgb};

/// Resolved platform identity and live OS/DE appearance signals.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SystemAppearance {
    pub platform: PlatformStyle,
    pub system_is_dark: bool,
    pub system_accent: Option<Rgb>,
}

pub fn resolve_platform_style(configured: PlatformStyle) -> PlatformStyle {
    let xdg = std::env::var("XDG_CURRENT_DESKTOP").ok();
    configured.resolve(xdg.as_deref())
}

#[cfg(target_os = "linux")]
fn portal_color_to_rgb(c: ashpd::desktop::Color) -> Option<Rgb> {
    let (r, g, b) = (c.red(), c.green(), c.blue());
    if (0.0..=1.0).contains(&r) && (0.0..=1.0).contains(&g) && (0.0..=1.0).contains(&b) {
        Some(Rgb {
            r: (r * 255.0).round().clamp(0.0, 255.0) as u8,
            g: (g * 255.0).round().clamp(0.0, 255.0) as u8,
            b: (b * 255.0).round().clamp(0.0, 255.0) as u8,
        })
    } else {
        None
    }
}

#[cfg(target_os = "linux")]
fn color_scheme_is_dark(scheme: ashpd::desktop::settings::ColorScheme) -> bool {
    matches!(scheme, ashpd::desktop::settings::ColorScheme::PreferDark)
}

#[cfg(target_os = "linux")]
pub async fn detect_system_appearance(configured: PlatformStyle) -> SystemAppearance {
    let platform = resolve_platform_style(configured);
    let (mut system_is_dark, mut system_accent) = (true, None);

    if let Ok(settings) = ashpd::desktop::settings::Settings::new().await {
        if let Ok(scheme) = settings.color_scheme().await {
            system_is_dark = color_scheme_is_dark(scheme);
        }
        if let Ok(accent) = settings.accent_color().await {
            system_accent = portal_color_to_rgb(accent);
        }
    }

    SystemAppearance {
        platform,
        system_is_dark,
        system_accent,
    }
}

#[cfg(target_os = "linux")]
pub fn spawn_appearance_watcher<F>(
    rt: &tokio::runtime::Handle,
    initial_dark: bool,
    initial_accent: Option<Rgb>,
    on_change: F,
) where
    F: Fn(bool, Option<Rgb>) + Send + Sync + 'static,
{
    use futures_util::StreamExt;
    rt.spawn(async move {
        let Ok(settings) = ashpd::desktop::settings::Settings::new().await else {
            return;
        };
        let Ok(mut scheme_stream) = settings.receive_color_scheme_changed().await else {
            return;
        };
        let mut accent_stream = settings.receive_accent_color_changed().await.ok();
        let mut cur_dark = initial_dark;
        let mut cur_accent = initial_accent;

        loop {
            tokio::select! {
                maybe_scheme = scheme_stream.next() => {
                    match maybe_scheme {
                        Some(scheme) => {
                            let next_dark = color_scheme_is_dark(scheme);
                            if next_dark != cur_dark {
                                cur_dark = next_dark;
                                on_change(cur_dark, cur_accent);
                            }
                        }
                        None => break,
                    }
                }
                maybe_accent = async {
                    match accent_stream.as_mut() {
                        Some(s) => s.next().await,
                        None => std::future::pending().await,
                    }
                } => {
                    if let Some(col) = maybe_accent {
                        let next_acc = portal_color_to_rgb(col);
                        if next_acc != cur_accent {
                            cur_accent = next_acc;
                            on_change(cur_dark, cur_accent);
                        }
                    } else {
                        accent_stream = None;
                    }
                }
            }
        }
    });
}

#[cfg(not(target_os = "linux"))]
pub async fn detect_system_appearance(configured: PlatformStyle) -> SystemAppearance {
    SystemAppearance {
        platform: resolve_platform_style(configured),
        system_is_dark: true,
        system_accent: None,
    }
}

#[cfg(not(target_os = "linux"))]
pub fn spawn_appearance_watcher<F>(
    _rt: &tokio::runtime::Handle,
    _initial_dark: bool,
    _initial_accent: Option<Rgb>,
    _on_change: F,
) where
    F: Fn(bool, Option<Rgb>) + Send + Sync + 'static,
{
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gnome_default_scheme_maps_to_light_and_prefer_dark_maps_to_dark() {
        #[cfg(target_os = "linux")]
        {
            use ashpd::desktop::settings::ColorScheme;
            assert!(!color_scheme_is_dark(ColorScheme::NoPreference));
            assert!(color_scheme_is_dark(ColorScheme::PreferDark));
            assert!(!color_scheme_is_dark(ColorScheme::PreferLight));
        }
    }
}
