use cxx_qt_build::{CxxQtBuilder, QmlModule};

fn main() {
    CxxQtBuilder::new_qml_module(
        QmlModule::new("org.dueattendant149.kwhale")
            .qml_file("qml/Main.qml"),
    )
    .files(["src/main.rs"])
    .qt_module("QuickControls2")
    .build();
}
