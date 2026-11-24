use crate::images::load_tray_icon_argb32;
use ksni::menu::{MenuItem, StandardItem};
use ksni::{Handle, Tray, TrayMethods};
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedSender;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayCommand {
    ToggleMainWindow,
    ToggleWidget,
    TogglePlay,
    PrevTrack,
    NextTrack,
    Quit,
}

#[derive(Debug, Clone)]
pub struct TrayState {
    pub now_label: String,
    pub has_track: bool,
    pub is_playing: bool,
    pub main_visible: bool,
    pub widget_visible: bool,
}

impl Default for TrayState {
    fn default() -> Self {
        Self {
            now_label: String::new(),
            has_track: false,
            is_playing: false,
            main_visible: true,
            widget_visible: false,
        }
    }
}

pub struct KwhaleTray {
    pub state: TrayState,
    pub tx: UnboundedSender<TrayCommand>,
    pub icons: Arc<Vec<ksni::Icon>>,
}

impl KwhaleTray {
    fn emit(&self, cmd: TrayCommand) {
        let _ = self.tx.send(cmd);
    }
}

impl Tray for KwhaleTray {
    fn id(&self) -> String {
        "org.dueattendant149.Kwhale".to_string()
    }

    fn category(&self) -> ksni::Category {
        ksni::Category::ApplicationStatus
    }

    fn title(&self) -> String {
        if self.state.has_track && !self.state.now_label.is_empty() {
            format!("KWhale — {}", self.state.now_label)
        } else {
            "KWhale".to_string()
        }
    }

    fn icon_name(&self) -> String {
        "org.dueattendant149.Kwhale".to_string()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        (*self.icons).clone()
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        let desc = if self.state.has_track && !self.state.now_label.is_empty() {
            self.state.now_label.clone()
        } else {
            "Музыка для себя".to_string()
        };
        ksni::ToolTip {
            title: "KWhale".to_string(),
            description: desc,
            ..Default::default()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        self.emit(TrayCommand::ToggleMainWindow);
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let mut items: Vec<MenuItem<Self>> = Vec::new();
        if self.state.has_track && !self.state.now_label.is_empty() {
            items.push(
                StandardItem {
                    label: self.state.now_label.clone(),
                    enabled: false,
                    ..Default::default()
                }
                .into(),
            );
            items.push(MenuItem::Separator);
        }

        items.push(
            StandardItem {
                label: if self.state.main_visible {
                    "Свернуть в трей".to_string()
                } else {
                    "Показать KWhale".to_string()
                },
                activate: Box::new(|this: &mut Self| this.emit(TrayCommand::ToggleMainWindow)),
                ..Default::default()
            }
            .into(),
        );

        items.push(
            StandardItem {
                label: if self.state.widget_visible {
                    "Скрыть мини-виджет".to_string()
                } else {
                    "Открыть мини-виджет".to_string()
                },
                activate: Box::new(|this: &mut Self| this.emit(TrayCommand::ToggleWidget)),
                ..Default::default()
            }
            .into(),
        );

        items.push(MenuItem::Separator);

        items.push(
            StandardItem {
                label: if self.state.is_playing {
                    "Пауза".to_string()
                } else {
                    "Играть".to_string()
                },
                icon_name: if self.state.is_playing {
                    "media-playback-pause".to_string()
                } else {
                    "media-playback-start".to_string()
                },
                activate: Box::new(|this: &mut Self| this.emit(TrayCommand::TogglePlay)),
                ..Default::default()
            }
            .into(),
        );
        items.push(
            StandardItem {
                label: "Предыдущий трек".to_string(),
                icon_name: "media-skip-backward".to_string(),
                activate: Box::new(|this: &mut Self| this.emit(TrayCommand::PrevTrack)),
                ..Default::default()
            }
            .into(),
        );
        items.push(
            StandardItem {
                label: "Следующий трек".to_string(),
                icon_name: "media-skip-forward".to_string(),
                activate: Box::new(|this: &mut Self| this.emit(TrayCommand::NextTrack)),
                ..Default::default()
            }
            .into(),
        );

        items.push(MenuItem::Separator);
        items.push(
            StandardItem {
                label: "Выход".to_string(),
                icon_name: "application-exit".to_string(),
                activate: Box::new(|this: &mut Self| this.emit(TrayCommand::Quit)),
                ..Default::default()
            }
            .into(),
        );
        items
    }
}

pub async fn spawn_tray(tx: UnboundedSender<TrayCommand>) -> Option<Handle<KwhaleTray>> {
    let icons = Arc::new(load_tray_icon_argb32());
    let sandboxed = ashpd::is_sandboxed();
    let tray = KwhaleTray {
        state: TrayState::default(),
        tx,
        icons,
    };
    match tray
        .disable_dbus_name(sandboxed)
        .assume_sni_available(true)
        .spawn()
        .await
    {
        Ok(handle) => Some(handle),
        Err(err) => {
            tracing::warn!(error = ?err, "system tray unavailable");
            None
        }
    }
}
